import AppKit
import Combine
import SwiftUI

// MARK: - 第 3 步的判定（R-101）

/// 试说一句的成功判据（DESIGN §12.1）：≥4 个字符，且与例句的字符多重集合交集 ÷ 例句字数 ≥ 0.5。
/// 纯函数，`--onboarding-selftest` 直接断言，不依赖麦克风。
enum ProbeCriterion {
    static let example = "今天天气不错，我们开始吧"
    static let minChars = 4
    static let minRatio = 0.5
    /// R-102「明显声音」阈值：core 的 silent 档是 0.003，0.01 ≈ -40 dBFS（D-112）
    static let loudRms = 0.01
    static let quietSeconds: Double = 3

    /// 去掉空白与标点，只留「说的字」；多说的字不扣分，所以分母固定用例句字数。
    static func normalized(_ text: String) -> [Character] {
        text.filter { !$0.isWhitespace && !$0.isNewline && !$0.isPunctuation && !$0.isSymbol }
    }

    /// 字符多重集合交集的大小（重复字按较少的一边算）
    static func overlapCount(_ a: [Character], _ b: [Character]) -> Int {
        var pool: [Character: Int] = [:]
        for ch in b { pool[ch, default: 0] += 1 }
        var hit = 0
        for ch in a {
            guard let n = pool[ch], n > 0 else { continue }
            pool[ch] = n - 1
            hit += 1
        }
        return hit
    }

    static func ratio(_ text: String) -> Double {
        let target = normalized(example)
        guard !target.isEmpty else { return 0 }
        return Double(overlapCount(normalized(text), target)) / Double(target.count)
    }

    static func passes(_ text: String) -> Bool {
        normalized(text).count >= minChars && ratio(text) >= minRatio
    }
}

/// 判定函数自测（不碰麦克风）：`--onboarding-selftest` 的第一段。
enum ProbeSelftest {
    static func run() -> Bool {
        let cases: [(String, Bool, String)] = [
            (ProbeCriterion.example, true, "整句照读"),
            ("今天天气不错", true, "只读前半句（6/11=0.55）"),
            ("我们开始吧", false, "只读后半句（5/11=0.45）"),
            ("今天", false, "不足 4 个字"),
            ("", false, "空结果"),
            ("今天天气不错，我们开始吧！", true, "标点不影响"),
            ("啊今天天气不错我们开始吧今天天气不错", true, "多说字不扣分"),
            ("hello world", false, "完全无关"),
        ]
        var ok = true
        for (text, want, note) in cases {
            let got = ProbeCriterion.passes(text)
            let ratio = String(format: "%.2f", ProbeCriterion.ratio(text))
            print("PROBE \(got == want ? "OK" : "FAIL") 「\(text)」→ \(got ? "通过" : "不通过") 重合率=\(ratio) 期望=\(want ? "通过" : "不通过")（\(note)）")
            if got != want { ok = false }
        }
        print("PROBE 例句去标点 \(ProbeCriterion.normalized(ProbeCriterion.example).count) 字，阈值 \(ProbeCriterion.minRatio)")
        return ok
    }
}

// MARK: - 引导状态

/// 第 3 步的实时状态：由事件驱动，页面只读它渲染。
final class OnboardingProbe: ObservableObject {
    @Published var active = false
    @Published var phase = "idle"
    @Published var partial = ""
    @Published var finalText = ""
    @Published var passed = false
    @Published var failure: String?

    func begin() {
        active = true
        phase = "idle"
        partial = ""
        finalText = ""
        failure = nil
    }

    func end() { active = false }

    /// 壳收到 state 事件时调用：新会话开始时清掉上一轮的文字/原因。
    func notePhase(_ p: String) {
        guard active else { return }
        phase = p
        if p == "starting" || p == "listening" {
            partial = ""
            finalText = ""
            failure = nil
            passed = false
        }
    }

    func notePartial(_ text: String) {
        guard active else { return }
        partial = text
    }

    func noteFailure(_ message: String) {
        guard active else { return }
        failure = message
        phase = "idle"
    }

    /// 返回 true 表示这一条文本被引导页吞掉了（不注入）；用户可照常重试。
    func accept(_ text: String) -> Bool {
        guard active else { return false }
        finalText = text
        failure = nil
        phase = "idle"
        passed = ProbeCriterion.passes(text)
        return true
    }
}

// MARK: - 控制器

/// 首次启动引导（R-101/102/103）：五步把「模型 / 麦克风与权限 / 上屏方式 / 润色 / 试一句」讲清楚。
/// 顺序是「先设置、最后验证」：试说放在最后一步，没出字也能完成（用户 2026-11-22 反馈）。
final class OnboardingController {
    private var window: NSWindow?
    private let model: AppModel
    let probe = OnboardingProbe()
    var startStep = 0
    var onFinish: (() -> Void)?
    /// 试说成功/失败的通知（只给 `--onboarding-selftest` 用）
    var onProbeVerdict: ((Bool, String) -> Void)?
    /// 页面里的话筒按钮：按下/松开走真实录音链路
    var onTalkDown: (() -> Void)?
    var onTalkUp: (() -> Void)?

    init(model: AppModel) { self.model = model }

    var probeActive: Bool { probe.active }

    func present() {
        guard window == nil else {
            window?.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 640, height: 480),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        w.title = "欢迎使用字在"
        w.isReleasedWhenClosed = false
        w.center()
        w.contentView = NSHostingView(rootView: OnboardingView(
            model: model,
            probe: probe,
            startStep: startStep,
            talkDown: { [weak self] in self?.onTalkDown?() },
            talkUp: { [weak self] in self?.onTalkUp?() },
            finish: { [weak self] in
                self?.stopMeterIfNeeded()
                self?.model.finishOnboarding()
                self?.onFinish?()
                self?.dismiss()
            },
            cancel: { [weak self] in
                self?.stopMeterIfNeeded()
                self?.model.finishOnboarding()
                self?.dismiss()
            }
        ))
        window = w
        w.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func dismiss() {
        probe.end()
        model.stopMeter()
        window?.close()
        window = nil
    }

    private func stopMeterIfNeeded() {
        probe.end()
        model.stopMeter()
    }

    // MARK: 事件入口（AppDelegate 调用）

    /// 第 3 步的文本被吞掉：只显示、不注入。返回 true = 已消费。
    func offerProbeResult(_ text: String) -> Bool {
        guard probe.active else { return false }
        guard probe.accept(text) else { return false }
        if probe.passed { model.markProbePassed() }
        onProbeVerdict?(probe.passed, text)
        return true
    }

    func notePhase(_ phase: String) { probe.notePhase(phase) }

    func notePartial(_ text: String) { probe.notePartial(text) }

    func noteFailure(code: String, message: String) {
        guard probe.active else { return }
        let text = message.isEmpty ? "识别失败（\(code)）" : message
        probe.noteFailure(text)
        onProbeVerdict?(false, text)
    }
}

// MARK: - 五步

private enum Step: Int, CaseIterable {
    // 顺序就是引导顺序：先把该给的权限、该装的模型、该选的方式都设好，最后才试说。
    case model, mic, output, polish, probe

    var title: String {
        switch self {
        case .model: return "选一个识别模型"
        case .mic: return "先确认麦克风和权限"
        case .output: return "选上屏方式"
        case .polish: return "要不要润色"
        case .probe: return "最后，试一句"
        }
    }

    var tip: String {
        switch self {
        case .model: return "语音只在本机识别。默认 SenseVoice 体积小、速度快；Qwen3 短句更准但吃内存。装好一个再往下走。"
        case .mic: return "这一步确认能收到声音，也把上屏要用的三项系统权限一次看完。不保存也不上传音频。"
        case .output: return "粘贴最快最稳；逐字打字慢一点，但在终端、安全输入框里更可靠。"
        case .polish: return "润色会把这段文字发到你填的服务（默认关）。不开也完全能用。"
        case .probe: return "模型、麦克风、上屏方式都设好了。按住说话键（或页面里的话筒按钮）读完这句，松开后文字出现在右边，整条链路就通了。"
        }
    }
}

private struct OnboardingView: View {
    @ObservedObject var model: AppModel
    @ObservedObject var probe: OnboardingProbe
    let startStep: Int
    let talkDown: () -> Void
    let talkUp: () -> Void
    let finish: () -> Void
    let cancel: () -> Void

    @State private var step: Step = .model
    @State private var mirrorDraft = ""
    @State private var showMirror = false
    @State private var quietWarn = false
    @State private var lastLoudAt = Date()

    private let tick = Timer.publish(every: 0.25, on: .main, in: .common).autoconnect()

    private var sense: ModelInfo? { model.models.first { $0.id == "sense" } }
    private var qwen: ModelInfo? { model.models.first { $0.id == "qwen3" } }
    /// 当前选中的引擎装好没有（没装好就不让从模型页往下走，否则试说注定没字）
    private var engineReady: Bool {
        model.models.contains { $0.active && $0.installed && $0.id != "stub" }
    }

    private var probeText: String {
        probe.finalText.isEmpty ? probe.partial : probe.finalText
    }

    private var micName: String {
        Permission.microphone.state.text
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .center) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("欢迎使用字在").font(.system(size: 20, weight: .semibold))
                    Text("先花一分钟把模型、权限和上屏方式设好，最后再试一句。")
                        .font(.system(size: 12)).foregroundStyle(.secondary)
                }
                Spacer()
                HStack(spacing: 6) {
                    ForEach(Step.allCases, id: \.rawValue) { s in
                        Circle()
                            .fill(s.rawValue <= step.rawValue ? Color.accentColor : Color.secondary.opacity(0.25))
                            .frame(width: 7, height: 7)
                    }
                }
            }
            .padding(.horizontal, 24).padding(.top, 22).padding(.bottom, 16)

            Divider()

            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text(step.title).font(.system(size: 15, weight: .semibold))
                    Text(step.tip).font(.system(size: 12)).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)

                    switch step {
                    case .model: modelStep
                    case .mic: micStep
                    case .probe: probeStep
                    case .output: outputStep
                    case .polish: polishStep
                    }
                }
                .padding(24)
            }

            Divider()

            HStack {
                Button("稍后再说") { cancel() }
                    .buttonStyle(.link)
                Spacer()
                if step != .model {
                    Button("上一步") { go(to: Step(rawValue: step.rawValue - 1) ?? .model) }
                }
                if step == .model && !engineReady {
                    Text("先装一个模型才能继续").font(.system(size: 11)).foregroundStyle(.orange)
                }
                let last = step == .probe
                Button(last ? "完成" : "下一步") {
                    if last { finish() } else { go(to: Step(rawValue: step.rawValue + 1) ?? .probe) }
                }
                .defaultActionUnless(step == .model && !engineReady)
                .disabled(step == .model && !engineReady)
            }
            .padding(.horizontal, 24).padding(.vertical, 14)
        }
        .frame(width: 640, height: 480)
        .onAppear {
            // 截图/自测用：ERYU_ONBOARDING_STEP=0..4 直接落到某一步（命令行 --onboarding-step 优先）
            if let v = ProcessInfo.processInfo.environment["ERYU_ONBOARDING_STEP"], let n = Int(v),
               let s = Step(rawValue: n) {
                apply(s, initial: true)
            } else {
                apply(Step(rawValue: startStep) ?? .model, initial: true)
            }
            mirrorDraft = model.config.model_mirror
            model.refreshModels()
            model.refreshConfig()
            model.refreshDevices()
        }
        .onDisappear {
            probe.end()
            model.stopMeter()
        }
        .onChange(of: model.meterRms) { v in
            guard step == .mic else { return }
            if v >= ProbeCriterion.loudRms {
                lastLoudAt = Date()
                quietWarn = false
            }
        }
        .onReceive(tick) { now in
            guard step == .mic else { return }
            if model.meterRms >= ProbeCriterion.loudRms {
                lastLoudAt = now
                quietWarn = false
            } else if now.timeIntervalSince(lastLoudAt) >= ProbeCriterion.quietSeconds {
                quietWarn = true
            }
        }
    }

    /// 切步：麦克风页进入/离开时起停电平，试说页进入/离开时起停探针。
    private func apply(_ s: Step, initial: Bool) {
        step = s
        if s == .mic {
            lastLoudAt = Date()
            quietWarn = false
            model.startMeter()
            model.refreshDevices()
        } else if !initial {
            model.stopMeter()
        }
        if s == .probe {
            probe.begin()
        } else if !initial {
            probe.end()
        }
    }

    private func go(to s: Step) { apply(s, initial: false) }

    // MARK: - 第 1 步：模型

    private var modelStep: some View {
        VStack(alignment: .leading, spacing: 12) {
            modelRow(sense, fallbackName: "SenseVoice", note: "默认。快、省内存，中英混合够用")
            modelRow(qwen, fallbackName: "Qwen3-0.6B", note: "短句更准，长录音吃资源，按需下载")

            HStack(spacing: 8) {
                Button(showMirror ? "收起下载地址" : "下载慢？换下载地址") { showMirror.toggle() }
                    .buttonStyle(.link).font(.system(size: 12))
                Spacer()
            }
            if showMirror {
                HStack(spacing: 8) {
                    TextField("https://your-host/eryu-models", text: $mirrorDraft)
                        .textFieldStyle(.roundedBorder)
                        .font(.system(size: 12, design: .monospaced))
                    Button("保存") { model.setModelMirror(mirrorDraft) }
                }
                Text("留空 = GitHub 官方发布。填自己的服务器后，下载会走「地址 + 文件名」。")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
            }
        }
    }

    private func modelRow(_ m: ModelInfo?, fallbackName: String, note: String) -> some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(m?.name ?? fallbackName).font(.system(size: 13, weight: .semibold))
                    if let m, m.active {
                        Text("使用中").font(.system(size: 10))
                            .padding(.horizontal, 6).padding(.vertical, 1)
                            .background(Color.accentColor.opacity(0.15), in: Capsule())
                    }
                    if let m, !m.installed { Text("未安装").font(.system(size: 10)).foregroundStyle(.orange) }
                }
                Text(note).font(.system(size: 11)).foregroundStyle(.secondary)
            }
            Spacer()
            if let m {
                Text(m.sizeText).font(.system(size: 11)).monospacedDigit().foregroundStyle(.secondary)
                if !m.installed, m.id != "stub" {
                    if let p = model.pulling[m.id] {
                        ProgressView(value: p.fraction).frame(width: 70)
                        Text(p.total > 0 ? String(format: "%.0f%%", p.fraction * 100) : "…")
                            .font(.system(size: 11)).monospacedDigit().frame(width: 34, alignment: .trailing)
                    } else {
                        Button("下载 \(m.sizeHintMB)MB") { model.pullModel(m.id) }
                            .buttonStyle(.bordered)
                    }
                }
                Button(m.active ? "当前" : "用它") { model.setEngine(m.id) }
                    .disabled(m.active || !m.installed)
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.04)))
    }

    // MARK: - 第 2 步：麦克风自检（R-102）

    private var micStep: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 8) {
                Text("输入设备").font(.system(size: 12)).foregroundStyle(.secondary)
                Picker("", selection: Binding(
                    get: { model.config.device },
                    set: { model.setDevice($0) }
                )) {
                    Text("系统默认").tag("")
                    ForEach(model.deviceList.devices) { d in
                        Text(d.name + (d.default ? "（默认）" : "")).tag(d.name)
                    }
                }
                .labelsHidden()
                .frame(width: 250)
                Button("刷新") {
                    model.refreshDevices()
                    model.refreshConfig()
                }
                .buttonStyle(.link).font(.system(size: 12))
            }

            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 8) {
                    Text("输入电平").font(.system(size: 12)).foregroundStyle(.secondary)
                    Spacer()
                    Text(quietWarn ? "未检测到明显声音" : "对着麦克风说句话")
                        .font(.system(size: 11))
                        .foregroundStyle(quietWarn ? Color.orange : Color.secondary)
                }
                MeterBar(rms: model.meterRms, peak: model.meterPeak)
                    .frame(height: 14)
            }
            .padding(12)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.04)))

            if quietWarn {
                Label("未检测到明显声音，换个麦克风或靠近一点", systemImage: "exclamationmark.triangle.fill")
                    .font(.system(size: 12)).foregroundStyle(.orange)
            }

            if model.deviceList.devices.isEmpty {
                Text("没有检测到麦克风。插上麦克风后点「刷新」。")
                    .font(.system(size: 12)).foregroundStyle(.orange)
            }

            VStack(alignment: .leading, spacing: 5) {
                Text("上屏要用的权限").font(.system(size: 11)).foregroundStyle(.secondary)
                ForEach(Permission.allCases, id: \.self) { p in
                    HStack(spacing: 7) {
                        Image(systemName: p.state.ok ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                            .font(.system(size: 11))
                            .foregroundStyle(p.state.ok ? Color.green : Color.orange)
                        Text(p.title).font(.system(size: 11))
                        Text(p.state.text).font(.system(size: 11)).foregroundStyle(.secondary)
                        Spacer()
                        if !p.state.ok {
                            Button("去开启") { Permission.openSettings(p) }
                                .buttonStyle(.link).font(.system(size: 11))
                        }
                    }
                }
                HStack(spacing: 8) {
                    Button("一次请求全部") { Permission.requestAll(openPanes: true) }
                        .buttonStyle(.link).font(.system(size: 11))
                    Button("刷新") { model.refreshDevices() }
                        .buttonStyle(.link).font(.system(size: 11))
                }
            }

            Text("这条电平只算音量，不保存音频；离开这一步会自动停止占用麦克风。")
                .font(.system(size: 11)).foregroundStyle(.secondary)
        }
    }

    // MARK: - 第 3 步：试说一句（R-101）

    private var probeStep: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 14) {
                setupChip("模型", engineReady)
                setupChip("麦克风", Permission.microphone.state.ok)
                setupChip("辅助功能", Permission.accessibility.state.ok)
                setupChip("输入监控", Permission.inputMonitoring.state.ok)
            }
            Text("这四项都就绪，试说才有意义；没就绪的先回上一步补，或直接点「完成」稍后再试。")
                .font(.system(size: 10)).foregroundStyle(.secondary)
            HStack(alignment: .top, spacing: 18) {
            VStack(alignment: .leading, spacing: 10) {
                Text("请朗读这句话").font(.system(size: 11)).foregroundStyle(.secondary)
                Text(ProbeCriterion.example)
                    .font(.system(size: 19, weight: .semibold))
                    .fixedSize(horizontal: false, vertical: true)

                HStack(spacing: 10) {
                    HoldToTalkButton(
                        active: probe.phase == "starting" || probe.phase == "listening",
                        onDown: { talkDown() },
                        onUp: { talkUp() }
                    )
                    VStack(alignment: .leading, spacing: 2) {
                        Text("按住说话").font(.system(size: 12, weight: .medium))
                        Text("或按住 \(HotkeySpec.displayName(for: model.config.hotkey))")
                            .font(.system(size: 11)).foregroundStyle(.secondary)
                    }
                }

                Divider()

                Text("换一个按住键").font(.system(size: 11)).foregroundStyle(.secondary)
                HStack(spacing: 6) {
                    ForEach(HotkeySpec.quickPicks, id: \.0) { pick in
                        Button(pick.1) { model.setHotkey(pick.0) }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(model.config.hotkey == pick.0)
                    }
                }
                Text("装好之后想换别的键：设置 → 通用 → 快捷键。")
                    .font(.system(size: 10)).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(width: 248, alignment: .leading)

            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 6) {
                    Text("识别结果").font(.system(size: 11)).foregroundStyle(.secondary)
                    Spacer()
                    if probe.passed {
                        Label("已成功识别", systemImage: "checkmark.circle.fill")
                            .font(.system(size: 11)).foregroundStyle(.green)
                    }
                }
                ZStack(alignment: .topLeading) {
                    RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.04))
                    if probeText.isEmpty {
                        Text("按住说话，松手后文字会出现在这里")
                            .font(.system(size: 12)).foregroundStyle(.tertiary)
                            .padding(10)
                    } else {
                        ScrollView {
                            Text(probeText)
                                .font(.system(size: 14))
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(10)
                                .textSelection(.enabled)
                        }
                    }
                }
                .frame(height: 132)

                statusLine
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    private func setupChip(_ name: String, _ ok: Bool) -> some View {
        Label(name, systemImage: ok ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
            .font(.system(size: 11, weight: .medium))
            .foregroundStyle(ok ? Color.green : Color.orange)
    }

    private var statusLine: some View {
        Group {
            if let failure = probe.failure {
                Label(failure, systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
            } else if probe.passed {
                Text("读到了，点「完成」进入字在。")
                    .foregroundStyle(.green)
            } else if probe.phase == "starting" || probe.phase == "listening" {
                Text("正在听…松开后出字")
                    .foregroundStyle(.secondary)
            } else if !probe.partial.isEmpty {
                Text("听到了，正在识别…")
                    .foregroundStyle(.secondary)
            } else {
                Text("还没读到字也可以点「完成」；之后在设置里随时能再试。")
                    .foregroundStyle(.secondary)
            }
        }
        .font(.system(size: 11))
        .fixedSize(horizontal: false, vertical: true)
    }

    // MARK: - 第 4 步：上屏

    private var outputStep: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach([("paste", "粘贴（推荐）", "写剪贴板再合成 ⌘V，最快最稳；原剪贴板会被恢复"),
                    ("keystroke", "逐字打字", "一个字一个字打，慢一点，但在终端/安全输入里更可靠")], id: \.0) { opt in
                Button {
                    model.setInsertMode(opt.0)
                } label: {
                    HStack(alignment: .top, spacing: 10) {
                        Image(systemName: model.config.insert_mode == opt.0 ? "largecircle.fill.circle" : "circle")
                            .foregroundStyle(model.config.insert_mode == opt.0 ? Color.accentColor : Color.secondary)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(opt.1).font(.system(size: 13, weight: .medium))
                            Text(opt.2).font(.system(size: 11)).foregroundStyle(.secondary)
                        }
                        Spacer()
                    }
                    .padding(10)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.04)))
                }
                .buttonStyle(.plain)
            }
        }
    }

    // MARK: - 第 5 步：润色

    private var polishStep: some View {
        VStack(alignment: .leading, spacing: 12) {
            Toggle("开启润色", isOn: Binding(
                get: { model.config.polish_enabled },
                set: { model.setPolishEnabled($0) }
            ))
            Text(model.config.polish_has_key
                 ? "已保存 API Key，开启即可用。"
                 : "还没填 API Key：到「设置 → 润色」里填服务地址和 Key 之后才生效。")
                .font(.system(size: 11)).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            Text("润色会把识别到的文字发到你配置的服务（默认 DeepSeek）。在意隐私就别开——不开也能正常听写。")
                .font(.system(size: 11)).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            Text("服务地址 / Key / 力度档位都在「设置 → 润色」里改，随时可以关掉。")
                .font(.system(size: 11)).foregroundStyle(.secondary)
        }
    }
}

// MARK: - 按住说话按钮

private struct HoldToTalkButton: View {
    let active: Bool
    let onDown: () -> Void
    let onUp: () -> Void

    @State private var held = false

    var body: some View {
        ZStack {
            Circle()
                .fill(active ? Color.accentColor : Color.primary.opacity(0.08))
            Image(systemName: active ? "waveform" : "mic.fill")
                .font(.system(size: 22, weight: .medium))
                .foregroundStyle(active ? Color.white : Color.accentColor)
        }
        .frame(width: 64, height: 64)
        .contentShape(Circle())
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { _ in
                    guard !held else { return }
                    held = true
                    onDown()
                }
                .onEnded { _ in
                    guard held else { return }
                    held = false
                    onUp()
                }
        )
        .onDisappear {
            if held { held = false; onUp() }
        }
    }
}


private extension View {
    /// 默认按钮（回车键）在 disabled 时仍然会被 AppKit 画成高亮蓝色，看起来像可点；
    /// 第 3 步未成功时去掉 defaultAction，让按钮回到灰色禁用外观。
    @ViewBuilder
    func defaultActionUnless(_ blocked: Bool) -> some View {
        if blocked { self } else { self.keyboardShortcut(.defaultAction) }
    }
}
