import AppKit
import SwiftUI

// MARK: - 窗口

/// 设置窗口：一个 NSWindow + SwiftUI 内容（无 Xcode 也能跑；DESIGN §5.1）
final class SettingsWindowController: NSWindowController, NSWindowDelegate {
    private let model: AppModel

    init(model: AppModel, initialTab: SettingsTab = .general) {
        self.model = model
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 760, height: 580),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "字在 设置"
        window.isReleasedWhenClosed = false
        window.center()
        super.init(window: window)
        window.delegate = self
        let root = SettingsRootView(model: model, initialTab: initialTab)
        window.contentView = NSHostingView(rootView: root)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) 未实现") }

    func present() {
        model.refreshAll()
        NSApp.activate(ignoringOtherApps: true)
        showWindow(nil)
        window?.makeKeyAndOrderFront(nil)
    }

    /// 关掉设置窗后回到「只有菜单栏」的状态（LSUIElement 的 App 不该在 Dock/切窗里留窗口）
    func windowWillClose(_ notification: Notification) {
        DispatchQueue.main.async { NSApp.deactivate() }
    }

    func presentHistory() {
        present()
    }
}

extension SettingsTab {
    /// `--show-settings=engine` 这类调试入口用
    init?(cliName: String) {
        switch cliName.lowercased() {
        case "general", "通用": self = .general
        case "engine", "引擎": self = .engine
        case "audio", "音频": self = .audio
        case "hotwords", "热词": self = .hotwords
        case "polish", "润色": self = .polish
        case "history", "历史与统计": self = .history
        case "diagnose", "诊断": self = .diagnose
        default: return nil
        }
    }
}

enum SettingsTab: String, CaseIterable, Identifiable {
    case general = "通用"
    case engine = "引擎"
    case audio = "音频"
    case hotwords = "热词"
    case polish = "润色"
    case history = "历史与统计"
    case diagnose = "诊断"

    var id: String { rawValue }
    var icon: String {
        switch self {
        case .general: return "keyboard"
        case .engine: return "cpu"
        case .audio: return "mic"
        case .hotwords: return "text.badge.checkmark"
        case .polish: return "wand.and.stars"
        case .history: return "clock.arrow.circlepath"
        case .diagnose: return "stethoscope"
        }
    }
}

struct SettingsRootView: View {
    @ObservedObject var model: AppModel
    @State private var tab: SettingsTab

    init(model: AppModel, initialTab: SettingsTab = .general) {
        self.model = model
        _tab = State(initialValue: initialTab)
    }

    var body: some View {
        VStack(spacing: 0) {
            TabView(selection: $tab) {
                GeneralSection(model: model).tabItem { Label(SettingsTab.general.rawValue, systemImage: SettingsTab.general.icon) }.tag(SettingsTab.general)
                EngineSection(model: model).tabItem { Label(SettingsTab.engine.rawValue, systemImage: SettingsTab.engine.icon) }.tag(SettingsTab.engine)
                AudioSection(model: model).tabItem { Label(SettingsTab.audio.rawValue, systemImage: SettingsTab.audio.icon) }.tag(SettingsTab.audio)
                HotwordsSection(model: model).tabItem { Label(SettingsTab.hotwords.rawValue, systemImage: SettingsTab.hotwords.icon) }.tag(SettingsTab.hotwords)
                PolishSection(model: model).tabItem { Label(SettingsTab.polish.rawValue, systemImage: SettingsTab.polish.icon) }.tag(SettingsTab.polish)
                HistorySection(model: model).tabItem { Label(SettingsTab.history.rawValue, systemImage: SettingsTab.history.icon) }.tag(SettingsTab.history)
                DiagnoseSection(model: model).tabItem { Label(SettingsTab.diagnose.rawValue, systemImage: SettingsTab.diagnose.icon) }.tag(SettingsTab.diagnose)
            }
            .padding(12)

            if let toast = model.toast {
                Text(toast)
                    .font(.callout)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                    .background(.thinMaterial, in: Capsule())
                    .padding(.bottom, 10)
            }
        }
        .frame(minWidth: 700, minHeight: 520)
        .onAppear { model.refreshAll() }
    }
}

// MARK: - 通用

struct GeneralSection: View {


    @ObservedObject var model: AppModel
    @State private var newAppName = ""
    @State private var recording = false
    @State private var minMs: Double = 400
    @State private var maxSecs: Double = 120

    var body: some View {
        Form {
            if let warning = model.config.load_warning {
                Section {
                    Label(warning, systemImage: "exclamationmark.triangle.fill")
                        .font(.callout)
                        .foregroundStyle(.orange)
                        .textSelection(.enabled)
                }
            }
            Section("快捷键") {
                HStack {
                    Text("按住说话")
                    Spacer()
                    Text(recording ? "按下任意键…" : HotkeySpec.displayName(for: model.config.hotkey))
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(recording ? .orange : .primary)
                    Text(model.config.hotkey)
                        .font(.caption2).monospaced()
                        .foregroundStyle(.secondary)
                    Button(recording ? "取消" : "修改") {
                        recording.toggle()
                    }
                }
                HStack(spacing: 8) {
                    Text("快捷选择").font(.caption).foregroundStyle(.secondary)
                    ForEach(HotkeySpec.quickPicks, id: \.0) { pick in
                        Button(pick.1) { model.setHotkey(pick.0) }
                            .buttonStyle(.borderless)
                            .font(.caption)
                    }
                    Spacer()
                }
                HStack {
                    Text("单键热键需要「辅助功能」权限")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                }
                .help("右 Option / Fn / F13 这类键不会输入到当前应用；A、; 这种键仍会打出来（只旁听不拦截）。组合键不会和打字冲突。")
            }

            Section("录音判定") {
                HStack {
                    Text("最短录音")
                    Slider(value: $minMs, in: 200...1500, step: 50)
                    Text("\(Int(minMs)) ms").monospacedDigit().frame(width: 70, alignment: .trailing)
                }
                HStack {
                    Text("单次上限")
                    Slider(value: $maxSecs, in: 30...300, step: 10)
                    Text("\(Int(maxSecs)) s").monospacedDigit().frame(width: 70, alignment: .trailing)
                }
                Text("低于最短录音按「太短」丢弃")
                    .font(.caption).foregroundStyle(.secondary)
                    .help("超过单次上限会自动结束并识别（按段处理）。")
            }

            Section("上屏方式") {
                Picker("方式", selection: Binding(
                    get: { model.config.insert_mode },
                    set: { model.setInsertMode($0) }
                )) {
                    Text("粘贴（快，默认）").tag("paste")
                    Text("逐字打字（慢，兼容性最好）").tag("keystroke")
                }
                .pickerStyle(.radioGroup)
                Text("粘贴快；逐字打字适合终端和密码框")
                    .font(.caption).foregroundStyle(.secondary)
                    .help("粘贴：写剪贴板后合成 ⌘V，750ms 后恢复原剪贴板。逐字打字：不发剪贴板，慢一些；安全输入状态下会明确失败。")

                if !model.config.insert_per_app.isEmpty {
                    ForEach(model.config.insert_per_app.keys.sorted(), id: \.self) { app in
                        HStack {
                            Text(app).font(.callout)
                            Spacer()
                            Picker("", selection: Binding(
                                get: { model.config.insert_per_app[app] ?? "paste" },
                                set: { newValue in
                                    var map = model.config.insert_per_app
                                    map[app] = newValue
                                    model.setInsertPerApp(map)
                                }
                            )) {
                                Text("粘贴").tag("paste")
                                Text("打字").tag("keystroke")
                            }
                            .labelsHidden()
                            .frame(width: 110)
                            Button {
                                var map = model.config.insert_per_app
                                map.removeValue(forKey: app)
                                model.setInsertPerApp(map)
                            } label: { Image(systemName: "trash") }
                                .buttonStyle(.borderless)
                        }
                    }
                }
                HStack {
                    TextField("应用名（如 Terminal、Xcode）", text: $newAppName)
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 220)
                    Button("按应用覆盖") {
                        let name = newAppName.trimmingCharacters(in: .whitespaces)
                        guard !name.isEmpty else { return }
                        var map = model.config.insert_per_app
                        map[name] = map[name] == "keystroke" ? "paste" : "keystroke"
                        model.setInsertPerApp(map)
                        newAppName = ""
                    }
                    .disabled(newAppName.trimmingCharacters(in: .whitespaces).isEmpty)
                    Text("应用名按包含匹配，大小写不敏感")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }

            Section("录音时") {
                Toggle("背景降噪", isOn: Binding(
                    get: { model.config.denoise },
                    set: { model.setDenoise($0) }
                ))
                Text("压掉风扇、空调这类稳态噪声")
                    .font(.caption).foregroundStyle(.secondary)
                    .help("用 GTCRN（1MB 流式模型）在采音阶段降噪；旁边有人说话时它会连人声一起压，那种场合关掉更准，或打开「音源锁定」。")
                if let m = model.models.first(where: { $0.id == "gtcrn" }), !m.installed {
                    HStack(spacing: 8) {
                        Text("降噪模型还没装，开了也不生效").font(.caption).foregroundStyle(.orange)
                        Button("下载 1MB") { model.pullModel("gtcrn") }
                            .buttonStyle(.bordered)
                    }
                }
                sourceLockSection
                Toggle("暂停正在播放的音乐", isOn: Binding(
                    get: { model.config.pause_media },
                    set: { model.setPauseMedia($0) }
                ))
                Text("按住说话时暂停 Music / Spotify，松手后恢复（只恢复它们本来在播的情况）。其他播放器不受影响。")
                    .font(.caption).foregroundStyle(.secondary)
                Toggle("补标点（需要 65MB 标点模型）", isOn: Binding(
                    get: { model.config.punct },
                    set: { model.setPunct($0) }
                ))
                HStack(spacing: 8) {
                    Text("没装模型时只在句末补句号")
                        .font(.caption).foregroundStyle(.secondary)
                        .help("识别完先过一遍标点模型（常驻内存约多 90MB），把「文档请大家」变成「文档，请大家」。")
                    Spacer()
                    if !model.config.punct_ready {
                        if let p = model.pulling["punct"] {
                            ProgressView(value: p.fraction).frame(width: 70)
                        } else {
                            Button("下载标点模型 65MB") { model.pullModel("punct") }
                                .buttonStyle(.bordered)
                        }
                    }
                }
            }

            Section("隐私") {
                Toggle("隐私模式", isOn: Binding(
                    get: { model.config.privacy_mode },
                    set: { model.setPrivacyMode($0) }
                ))
                Text("开启后：不写转写历史、识别失败的音频只留内存（本次运行内可重试）、浮层不显示识别文本。上屏和热键照常。")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section("启动") {
                Toggle("开机自动启动", isOn: Binding(
                    get: { model.config.launch_at_login },
                    set: { model.setLaunchAtLogin($0) }
                ))
                Text("开机启动需要应用在「应用程序」文件夹里；首次注册可能会被系统拒绝一次，再点一次通常就成。")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .background(KeyRecorder(isActive: $recording) { event in
            // 松手（keyUp）不要；修饰键的按下走 flagsChanged 分支
            if event.modifierFlags.contains(.function) == false, event.keyCode == 63, !event.modifierFlags.isEmpty {
                return
            }
            let isModifierOnly = HotkeySpec.isModifierKeyCode(CGKeyCode(event.keyCode))
            let spec: String?
            if isModifierOnly {
                spec = HotkeySpec.modifierToken(CGKeyCode(event.keyCode))
            } else {
                spec = HotkeySpec.format(keyCode: UInt32(event.keyCode), modifiers: event.modifierFlags)
                    ?? HotkeySpec.name(for: UInt32(event.keyCode))
            }
            guard let spec else {
                model.showToast("这个键不认识，换一个试试")
                return
            }
            model.setHotkey(spec)
            recording = false
        })
        .onAppear {
            minMs = Double(model.config.min_record_ms)
            maxSecs = Double(model.config.max_record_secs)
        }
        .onChange(of: minMs) { v in model.setMinRecordMs(Int(v)) }
        .onChange(of: maxSecs) { v in model.setMaxRecordSecs(Int(v)) }
    }

    private var sourceLockSection: some View {
        VStack(alignment: .leading, spacing: 4) {
            Toggle("音源锁定（实验）", isOn: Binding(
                get: { model.config.source_lock },
                set: { model.setSourceLock($0) }
            ))
            Text("谁先说话就锁定谁")
                .font(.caption)
                .foregroundStyle(.secondary)
                .help("不用注册声纹。两人同时说话分不开；连续 8 秒都不像他会解锁，以免吞掉你自己的话。")
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

// MARK: - 引擎

struct EngineSection: View {
    @ObservedObject var model: AppModel
    @State private var mirrorDraft = ""
    @State private var proxyDraft = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Text("识别引擎")
                    .font(.headline)
                Text("切换后下次录音生效")
                    .font(.caption).foregroundStyle(.secondary)
                    .help("SenseVoice 快且省内存；Qwen3 短句更准（中文 1.6% vs 2.9%，英文 3.5% vs 12.4%），但长录音吃资源（75 秒解码 27 秒、峰值 2.2G）。日常建议 SenseVoice。")

                ForEach(model.models.filter { $0.id == "sense" || $0.id == "qwen3" || $0.id == "stub" }) { m in
                    HStack(alignment: .top, spacing: 12) {
                        VStack(alignment: .leading, spacing: 3) {
                            HStack(spacing: 6) {
                                Text(m.name).font(.system(size: 13, weight: .semibold))
                                if m.active {
                                    Text("使用中").font(.caption2)
                                        .padding(.horizontal, 6).padding(.vertical, 1)
                                        .background(Color.accentColor.opacity(0.15), in: Capsule())
                                }
                                if !m.installed {
                                    Text("未安装").font(.caption2).foregroundStyle(.orange)
                                }
                            }
                            Text(m.note).font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Text(m.sizeText).font(.caption).monospacedDigit().foregroundStyle(.secondary)
                        if !m.installed && m.id != "stub" {
                            if let p = model.pulling[m.id] {
                                ProgressView(value: p.fraction)
                                    .frame(width: 80)
                                Text(p.total > 0 ? String(format: "%.0f%%", p.fraction * 100) : "…")
                                    .font(.caption).monospacedDigit()
                                    .frame(width: 36, alignment: .trailing)
                            } else {
                                Button("下载 \(m.sizeHintMB)MB") { model.pullModel(m.id) }
                                    .buttonStyle(.bordered)
                            }
                        }
                        Button(m.active ? "当前" : "切换") { model.setEngine(m.id) }
                            .disabled(m.active || !m.installed)
                    }
                    .padding(10)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.04)))
                }

                Divider()

                Text("模型装在 ~/.eryu/models")
                    .font(.caption).foregroundStyle(.secondary)
                    .help("没装的直接点下载：用系统 curl 拉官方发布包，约 30 秒到 2 分钟，视网络。")

                Divider()

                // 默认走内置镜像，界面不再要求用户理解一个 URL（R-130）
                HStack(spacing: 6) {
                    Text("模型已经默认从这台机器的镜像下载；想自己下也可以：")
                        .font(.caption).foregroundStyle(.secondary)
                    Link("模型来源", destination: URL(string: "https://officeace.harmonyos.fun/eryu-models/")!)
                        .font(.caption)
                        .help("打开镜像目录，挑自己需要的模型文件；下好放进 ~/.eryu/models 对应目录即可。")
                }
                DisclosureGroup("自定义下载地址（一般不用改）") {
                    Text("留空 = 内置镜像；填了就从你指定的目录拉（发布文件平铺在目录下即可）。")
                        .font(.caption).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    HStack(spacing: 8) {
                        TextField("默认：内置镜像", text: $mirrorDraft)
                            .textFieldStyle(.roundedBorder)
                            .font(.system(size: 12, design: .monospaced))
                        Button("保存") { model.setModelMirror(mirrorDraft) }
                            .disabled(mirrorDraft == model.config.model_mirror)
                    }
                }
                .font(.caption)
                HStack(spacing: 4) {
                    Text("下载代理（内网用）").font(.system(size: 12, weight: .semibold))
                    Image(systemName: "info.circle").font(.caption).foregroundStyle(.tertiary)
                        .help("留空 = 自动读系统代理（环境变量或系统设置里的都算）。内网需要单独代理时按 http://127.0.0.1:7890 这样填；不支持 SOCKS。")
                }.padding(.top, 4)
                HStack(spacing: 8) {
                    TextField("留空 = 自动跟随系统代理，例如 http://127.0.0.1:7890", text: $proxyDraft)
                        .textFieldStyle(.roundedBorder)
                        .font(.system(size: 12, design: .monospaced))
                    Button("保存") { model.setModelProxy(proxyDraft) }
                        .disabled(proxyDraft == model.config.model_proxy)
                }
            }
            .padding(4)
            .onAppear {
                mirrorDraft = model.config.model_mirror
                proxyDraft = model.config.model_proxy
            }
            .onChange(of: model.config.model_mirror) { mirrorDraft = $0 }
            .onChange(of: model.config.model_proxy) { proxyDraft = $0 }
        }
    }
}

// MARK: - 音频

struct AudioSection: View {
    @ObservedObject var model: AppModel
    @State private var lanTokenDraft = ""
    private var voiceToggle: some View {
        Toggle("只识别我的声音", isOn: Binding(
            get: { model.config.voice_lock },
            set: { model.setVoiceLock($0) }
        ))
        .disabled(!model.config.voice_enrolled || !model.config.voice_model_ready)
    }

    @ViewBuilder
    private var voiceControls: some View {
        HStack(spacing: 10) {
            voiceButtons
            Spacer()
            Text(voiceStatusLine)
                .font(.caption)
                .foregroundStyle(model.config.voice_enrolled ? Color.secondary : Color.orange)
        }
    }

    @ViewBuilder
    private var voiceButtons: some View {
        if !model.config.voice_model_ready {
            Button("下载声纹模型（39MB）") { model.pullModel("speaker") }
        } else {
            Button(model.voiceBusy ? "正在录…" : "录 8 秒当声纹") { model.enrollVoice(seconds: 8) }
                .disabled(model.voiceBusy)
            if model.config.voice_enrolled {
                Button("删除声纹", role: .destructive) { model.clearVoice() }
            }
        }
    }
    private var voiceStatusLine: String {
        if !model.config.voice_model_ready { return "声纹模型未下载" }
        if model.config.voice_enrolled {
            return "已注册（\(model.voiceDim) 维 \(model.voiceCreated)）"
        }
        return "还没录声纹"
    }
    @ViewBuilder
    private var voiceSection: some View {
        Section("只录我的声音") {
            voiceToggle
            Text("不像你自己的声音就跳过")
                .font(.caption).foregroundStyle(.secondary)
                .help("每段录音先比对声纹，不像你的就不上屏、不进历史。公放/会议里可能误判，可随时关掉。")
            voiceControls
            Text("声纹存在本机（~/.eryu/voice_print.bin，仅你自己可读）：512 维向量，不上传。")
                .font(.caption2).foregroundStyle(.secondary)
            Text("阈值 " + String(format: "%.2f", model.config.voice_lock_threshold) + "：实测同一人 0.92–0.93，不同人 0.49–0.57。")
                .font(.caption2).foregroundStyle(.secondary)
        }
    }


    /// 场景指引：四个开关的组合建议（与 docs/USAGE.md 同口径，改一处要同步另一处）
    private var scenarioSection: some View {
        Section("什么场景开什么") {
            VStack(alignment: .leading, spacing: 6) {
                scenarioRow("office", "办公室/家里，正常环境", "默认即可：降噪开、补标点开")
                scenarioRow("person.2", "旁边有人说话/电视在响", "关掉降噪（竞争人声会更差）；或麦克风离嘴 10cm")
                scenarioRow("headphones", "外放音乐/视频时说话", "戴耳机；或打开「录音时暂停播放」")
                scenarioRow("person.crop.circle.badge.checkmark", "只想录自己的声音（多人会议）", "先录 8 秒声纹再开「只识别我的声音」；插话多时再加「音源锁定」")
                scenarioRow("text.bubble", "旁边人声一直插话、你也想记全", "关掉音源锁定（它按段丢，宁可全留）")
                scenarioRow("lock.shield", "输密码框", "逐字上屏会自动拒绝；粘贴模式也建议先切走")
            }
        }
    }

    private func scenarioRow(_ icon: String, _ title: String, _ advice: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: icon).frame(width: 16).foregroundStyle(.secondary)
            Text(title).font(.callout)
            Text("→").foregroundStyle(.tertiary)
            Text(advice).font(.caption).foregroundStyle(.secondary)
        }
    }

    /// 局域网听写接口（M5 路线②）：别的设备发 WAV 过来换文字，不上屏、不写历史
    private var lanSection: some View {
        Section("局域网听写接口（实验）") {
            Toggle("允许其它设备把音频发过来识别", isOn: Binding(
                get: { model.config.lan_enabled },
                set: { model.setLanEnabled($0) }
            ))
            Text("只做「转文字」：不上屏、不写历史，保存后立刻生效")
                .font(.caption).foregroundStyle(.secondary)
            HStack(spacing: 10) {
                Text("端口").font(.callout)
                TextField("8765", value: Binding(
                    get: { model.config.lan_port },
                    set: { model.setLanPort($0) }
                ), format: .number)
                .frame(width: 80)
                .textFieldStyle(.roundedBorder)
                Text("默认只监听 127.0.0.1（本机）").font(.caption).foregroundStyle(.secondary)
            }
            Toggle("允许局域网内其它设备访问（0.0.0.0）", isOn: Binding(
                get: { model.config.lan_allow_lan },
                set: { model.setLanAllowLan($0) }
            ))
            HStack(spacing: 10) {
                SecureField(model.config.lan_has_token ? "已设置 token（留空不改）" : "token（建议设一个）",
                            text: $lanTokenDraft)
                    .textFieldStyle(.roundedBorder)
                Button("生成") {
                    // R-132：点一下就有 token，不用用户自己想一个
                    _ = model.generateLanToken()
                    lanTokenDraft = ""
                }
                .help("生成一个随机 token 并立刻保存；其它设备请求时带 x-eryu-token 头")
                Button("保存 token") {
                    let t = lanTokenDraft.trimmingCharacters(in: .whitespaces)
                    if !t.isEmpty { model.setLanToken(t); lanTokenDraft = "" }
                }
                .disabled(lanTokenDraft.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            if model.config.lan_allow_lan {
                let host = model.localIPv4() ?? "<本机 IP>"
                let port = model.config.lan_port
                let url = "http://\(host):\(port)/transcribe"
                VStack(alignment: .leading, spacing: 4) {
                    Text("怎么连：").font(.caption).foregroundStyle(.secondary)
                    HStack(spacing: 6) {
                        Text(url).font(.system(size: 12, design: .monospaced)).textSelection(.enabled)
                        Button {
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(url, forType: .string)
                        } label: { Image(systemName: "doc.on.doc") }
                            .buttonStyle(.borderless)
                            .help("复制地址")
                    }
                    Text(#"""
                    curl -X POST \#(url) \
                      -H "x-eryu-token: <你的 token>" \
                      -H "Content-Type: audio/wav" \
                      --data-binary @一段16k单声道.wav
                    """#)
                        .font(.system(size: 11, design: .monospaced))
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                    if !model.config.lan_has_token {
                        Text("还没设 token：先点「生成」，不然别的设备请求会被拒绝。")
                            .font(.caption2).foregroundStyle(.orange)
                    }
                }
                .padding(.top, 2)
            } else {
                Text("现在只监听本机 127.0.0.1；勾上「允许局域网内其它设备访问」后这里会显示连接地址。")
                    .font(.caption2).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    var body: some View {
        Form {
            scenarioSection
            lanSection
            voiceSection

            Section("输入设备") {
                Picker("麦克风", selection: Binding(
                    get: { model.config.device },
                    set: { model.setDevice($0) }
                )) {
                    Text("系统默认\(model.deviceList.default.map { "（\($0)）" } ?? "")").tag("")
                    ForEach(model.deviceList.devices) { d in
                        Text(d.name + (d.default ? "（默认）" : "")).tag(d.name)
                    }
                }
                Text("找不到指定设备时会报错，不会悄悄换回默认设备。")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section("输入电平") {
                MeterBar(rms: model.meterRms, peak: model.meterPeak)
                    .frame(height: 18)
                HStack {
                    Button(model.meterActive ? "停止测试" : "开始测试") {
                        model.meterActive ? model.stopMeter() : model.startMeter()
                    }
                    Text(model.meterActive
                         ? (model.meterRms < 0.003 ? "几乎无声：检查系统声音输入是否选对、麦克风是否被静音" : "有声音，正常")
                         : "说话时条会动；只算音量，不保存音频")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
        .onAppear { model.refreshDevices() }
        .onDisappear { model.stopMeter() }
    }
}

struct MeterBar: View {
    let rms: Double
    let peak: Double

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 4).fill(Color.primary.opacity(0.08))
                RoundedRectangle(cornerRadius: 4)
                    .fill(LinearGradient(colors: [.green, .yellow, .red], startPoint: .leading, endPoint: .trailing))
                    .frame(width: max(2, geo.size.width * min(1, rms * 1.6)))
                Rectangle()
                    .fill(Color.primary.opacity(0.35))
                    .frame(width: 1.5, height: geo.size.height)
                    .offset(x: max(0, geo.size.width * min(1, peak * 1.6)))
            }
        }
    }
}

// MARK: - 润色

struct PolishSection: View {
    @ObservedObject var model: AppModel
    @State private var keyDraft = ""
    @State private var endpoint = ""
    @State private var modelName = ""
    @State private var level = "off"
    @State private var timeout: Double = 3000
    @State private var profilesDraft: [PolishProfile] = []
    @State private var dirtyProfiles = false
    @State private var editingIndex: Int?
    @State private var nameText = ""
    @State private var slotText = ""
    @State private var promptText = ""
    @State private var levelText = "light"
    @State private var iconText = "sparkles"
    @State private var keyPasteHint = ""
    @State private var presetId = "custom"


    @ViewBuilder
    private func profileRow(_ p: PolishProfile) -> some View {
        HStack(spacing: 10) {
            Button {
                model.activateProfile(p.name)
            } label: {
                Image(systemName: p.active ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(p.active ? Color.accentColor : Color.secondary)
            }
            .buttonStyle(.plain)
            .help(p.active ? "当前生效" : "切到这一档")

            Text(p.name.isEmpty ? "（未命名）" : p.name)
                .font(.title3.weight(p.active ? .semibold : .regular))
            Text(levelShort(p.level))
                .font(.caption)
                .padding(.horizontal, 7).padding(.vertical, 2)
                .background(Capsule().fill(Color.secondary.opacity(0.14)))
            Spacer(minLength: 12)
            Text(p.slot.trimmingCharacters(in: .whitespaces).isEmpty
                 ? "未绑快捷键"
                 : HotkeySpec.displayName(for: p.slot))
                .font(.callout)
                .foregroundStyle(.secondary)
            Button(role: .destructive) {
                profilesDraft.removeAll { $0.id == p.id }
                dirtyProfiles = true
                editingIndex = nil
            } label: { Image(systemName: "trash") }
            .buttonStyle(.plain)
            .opacity(0.6)
        }
        .padding(.vertical, 5)
        .contentShape(Rectangle())
        .onTapGesture {
            let i = indexOf(p)
            if editingIndex == i { editingIndex = nil } else { loadEditor(i) }
        }
    }

    private func indexOf(_ p: PolishProfile) -> Int {
        profilesDraft.firstIndex(where: { $0.id == p.id }) ?? 0
    }

    private func levelShort(_ v: String) -> String {
        switch v {
        case "light": return "轻度"
        case "medium": return "中度"
        case "strong": return "重度"
        default: return "关闭"
        }
    }

    private func loadEditor(_ i: Int) {
        guard i < profilesDraft.count else { return }
        editingIndex = i
        nameText = profilesDraft[i].name
        slotText = profilesDraft[i].slot
        promptText = profilesDraft[i].prompt
        levelText = profilesDraft[i].level
        iconText = profilesDraft[i].icon
    }

    private func composedProfile(_ i: Int) -> PolishProfile {
        var d = profilesDraft[i]
        d.name = nameText
        d.slot = slotText
        d.prompt = promptText
        d.level = levelText
        d.icon = iconText.isEmpty ? "sparkles" : iconText
        return d
    }

    private var draftIsChanged: Bool {
        guard let i = editingIndex, i < profilesDraft.count else { return false }
        return composedProfile(i) != profilesDraft[i]
    }

    private func commitDraft() {
        guard let i = editingIndex, i < profilesDraft.count else { return }
        let d = composedProfile(i)
        if d != profilesDraft[i] { profilesDraft[i] = d }
    }

    @ViewBuilder
    private func profileEditor(index: Int) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .bottom, spacing: 12) {
                labeled("图标（浮层上显示）") {
                    HStack(spacing: 8) {
                        Image(systemName: iconText.isEmpty ? "sparkles" : iconText)
                            .font(.system(size: 15, weight: .semibold))
                            .frame(width: 22)
                        Menu {
                            ForEach(Self.profileIcons, id: \.0) { item in
                                Button {
                                    iconText = item.0
                                } label: { Label(item.1, systemImage: item.0) }
                            }
                        } label: { Text("选图标") }
                        .frame(width: 96)
                    }
                }
                Spacer()
            }
            labelWithInfo("名称", "这一档叫什么，只显示在列表和浮层里") {
                TextField("", text: $nameText)
                    .textFieldStyle(.roundedBorder)
                    .labelsHidden()
                    .multilineTextAlignment(.leading)
            }
            labeled("力度") {
                Picker("", selection: $levelText) {
                    ForEach(levels.filter { $0.0 != "off" }, id: \.0) { lv in
                        Text(levelShort(lv.0)).tag(lv.0)
                    }
                }
                .labelsHidden()
                .pickerStyle(.segmented)
                .fixedSize()
            }
            labelWithInfo("快捷键", "留空 = 不绑。写法如 CTRL+ALT+1、ALT_R、F13") {
                TextField("", text: $slotText)
                    .textFieldStyle(.roundedBorder)
                    .labelsHidden()
                    .multilineTextAlignment(.leading)
                    .frame(width: 150, alignment: .leading)
            }
            labelWithInfo("附加要求", "留空就只按力度润色。可以写自己的要求，例如「保留原话，不要改称呼」「公司名写成华为云」") {
                // TextField(axis: .vertical) 在 Form 里会把文字排到右边（macOS 26 的已知行为，
                // 试过 multilineTextAlignment/layoutDirection 都压不住），这里用 TextEditor
                // 自己画边框：文字左对齐、多行、⌘V 和输入法都走系统原生路径。
                TextEditor(text: $promptText)
                    .font(.body)
                    .scrollContentBackground(.hidden)
                    .frame(minHeight: 44, maxHeight: 72)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 5)
                    .background(
                        RoundedRectangle(cornerRadius: 6, style: .continuous)
                            .fill(Color(nsColor: .textBackgroundColor))
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: 6, style: .continuous)
                            .stroke(Color.secondary.opacity(0.35), lineWidth: 1)
                    )
                    .multilineTextAlignment(.leading)
                    .accessibilityLabel("附加要求")
            }
        }
        .padding(.vertical, 2)
    }

    /// 标题 + ⓘ：说明性文字一律放悬停提示里，界面上不摆长句（用户反复强调过）
    private func labelWithInfo<Content: View>(
        _ title: String, _ help: String, @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 5) {
                Text(title).font(.callout).foregroundStyle(.secondary)
                Image(systemName: "info.circle")
                    .font(.callout)
                    .foregroundStyle(.tertiary)
                    .help(help)
            }
            content().frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// Form 里给输入框用：标签在上、输入框占满一行、文字自然靠左。
    /// 直接用 Form 的默认行样式会把控件挤到右边（用户反馈「输入框都靠右，很丑」，m16347）。
    @ViewBuilder
    private func fieldLabeled<Content: View>(
        _ title: String, help: String? = nil, @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 5) {
                Text(title).font(.callout).foregroundStyle(.secondary)
                if let help {
                    Image(systemName: "info.circle")
                        .font(.callout)
                        .foregroundStyle(.tertiary)
                        .help(help)
                }
            }
            content().frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func labeled<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.caption).foregroundStyle(.secondary)
            content()
        }
    }

    /// 润色档可选的图标（浮层胶囊左侧显示，学 AutoGLM 用 logo 区分档位）
    static let profileIcons: [(String, String)] = [
        ("sparkles", "闪光"),
        ("text.bubble", "气泡"),
        ("envelope", "信封"),
        ("note.text", "便签"),
        ("doc.text", "文档"),
        ("pencil.line", "铅笔"),
        ("quote.bubble", "引用"),
        ("character.book.closed", "书籍"),
        ("bolt", "闪电"),
        ("wand.and.stars", "魔法棒"),
        ("globe", "地球"),
        ("briefcase", "公文包"),
        ("graduationcap", "学术"),
        ("heart.text.square", "笔记"),
    ]

    private let levels: [(String, String)] = [
        ("off", "关闭（全程本地）"),
        ("light", "轻度：去口头禅、补标点"),
        ("medium", "中度：断句、顺口、统一叫法"),
        ("strong", "重度：可调整句序，信息不增减"),
    ]

    var body: some View {
        Form {
            Section {
                VStack(alignment: .leading, spacing: 10) {
                    VStack(spacing: 0) {
                        ForEach(profilesDraft) { p in
                            let editing = editingIndex == indexOf(p)
                            VStack(spacing: 0) {
                                profileRow(p)
                                if editing {
                                    profileEditor(index: indexOf(p))
                                        .id(p.id)
                                        .padding(.leading, 34)
                                        .padding(.trailing, 10)
                                        .padding(.top, 8)
                                        .padding(.bottom, 10)
                                }
                            }
                            .background(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .fill(editing ? Color.accentColor.opacity(0.08) : Color.clear)
                            )
                            .overlay(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .stroke(editing ? Color.accentColor.opacity(0.35) : Color.clear, lineWidth: 1)
                            )
                            if p.id != profilesDraft.last?.id {
                                Divider().padding(.leading, 26)
                            }
                        }
                    }
                    Divider()
                    HStack(spacing: 10) {
                        Button {
                            let fresh = PolishProfile(name: "新档", icon: "sparkles", level: "light")
                            profilesDraft.append(fresh)
                            dirtyProfiles = true
                            loadEditor(profilesDraft.count - 1)
                        } label: { Label("新建一档", systemImage: "plus") }
                        Spacer()
                        if draftIsChanged { Text("未保存").font(.caption).foregroundStyle(.orange) }
                        if editingIndex != nil {
                            Button("还原") { loadEditor(editingIndex ?? 0) }
                        }
                        Button("保存") {
                            commitDraft()
                            model.saveProfiles(profilesDraft)
                            dirtyProfiles = false
                            model.applyProfileSlots()
                        }
                        .keyboardShortcut("s")
                        .disabled(!dirtyProfiles && !draftIsChanged)
                        .help("存到 \(model.profilesPath)")
                    }

                }
                .listRowInsets(EdgeInsets(top: 8, leading: 10, bottom: 8, trailing: 10))
            } header: {
                Text("润色档")
            }
            .onAppear {
                profilesDraft = model.profiles
                if editingIndex == nil, let i = profilesDraft.firstIndex(where: { $0.active }) {
                    loadEditor(i)
                }
            }
            .onChange(of: model.profiles) { new in
                if !dirtyProfiles { profilesDraft = new }
            }

            Section {
                Toggle("启用润色", isOn: Binding(
                    get: { model.config.polish_enabled },
                    set: { model.setPolishEnabled($0) }
                ))
                if model.config.polish_enabled && !model.config.polish_has_key {
                    Text("还没填 API Key，润色不会生效")
                        .font(.caption).foregroundStyle(Color.orange)
                }
            } header: {
                HStack(spacing: 4) {
                    Text("润色")
                    Image(systemName: "info.circle")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .help("每段文字会先发到下面的服务端整理，网络不通或服务端报错就用原文，不会改坏原话")
                }
            }

            Section {
                Picker("服务商", selection: $presetId) {
                    ForEach(ProviderPreset.all) { p in Text(p.name).tag(p.id) }
                }
                .onChange(of: presetId) { new in
                    guard let p = ProviderPreset.all.first(where: { $0.id == new }), p.id != "custom" else { return }
                    endpoint = p.endpoint
                    modelName = p.model
                    model.setPolishEndpoint(p.endpoint)
                    model.setPolishModel(p.model)
                    model.polishModelsHint = ""
                }
                fieldLabeled("服务地址", help: "填到 /v1 之前即可，例如 https://api.deepseek.com；选上面的服务商会自动填好") {
                    TextField("", text: $endpoint)
                        .textFieldStyle(.roundedBorder)
                        .labelsHidden()
                        .multilineTextAlignment(.leading)
                        .onSubmit { model.setPolishEndpoint(endpoint) }
                        .help("填到 /v1 之前即可；选服务商会自动填好")
                }
                fieldLabeled("模型") {
                    HStack {
                    TextField("模型", text: $modelName, prompt: Text("deepseek-chat"))
                        .textFieldStyle(.roundedBorder)
                        .labelsHidden()
                        .multilineTextAlignment(.leading)
                        .onSubmit { model.setPolishModel(modelName) }
                    if !model.polishModels.isEmpty {
                        Menu("选") {
                            ForEach(model.polishModels, id: \.self) { m in
                                Button(m) {
                                    modelName = m
                                    model.setPolishModel(m)
                                }
                            }
                        }
                        .frame(width: 50)
                    }
                }
                }
                fieldLabeled("API Key", help: model.config.polish_has_key ? "已保存。留空则不改；要换就填新的再点「保存 Key」" : "一般以 sk- 开头；有些输入法会挡住 ⌘V，可以用右边的「粘贴」按钮") {
                    HStack(spacing: 8) {
                        SecureField("", text: $keyDraft)
                            .textFieldStyle(.roundedBorder)
                            .labelsHidden()
                            .multilineTextAlignment(.leading)
                        Button("粘贴") {
                            if let s = NSPasteboard.general.string(forType: .string) {
                                keyDraft = s.trimmingCharacters(in: .whitespacesAndNewlines)
                                keyPasteHint = "已从剪贴板读入 \(keyDraft.count) 个字符，点「保存 Key」生效"
                            } else {
                                keyPasteHint = "剪贴板里没有文字"
                            }
                        }
                        .help("从剪贴板读入 Key（有些输入法/安全设置会挡住 ⌘V，用这个更稳）")
                    }
                }
                if !keyPasteHint.isEmpty {
                    Text(keyPasteHint).font(.caption).foregroundStyle(.secondary)
                }
                HStack {
                    Button("保存 Key") {
                        model.setPolishKey(keyDraft)
                        keyDraft = ""
                        keyPasteHint = ""
                    }
                    .disabled(keyDraft.trimmingCharacters(in: .whitespaces).isEmpty)
                    Button("检测可用模型") { model.detectPolishModels() }
                        .disabled(!model.config.polish_has_key && keyDraft.isEmpty)
                    Spacer()
                    Button("测试连接") { model.testPolish() }
                }
                if !model.polishModelsHint.isEmpty {
                    Text(model.polishModelsHint).font(.caption).foregroundStyle(model.polishModels.isEmpty ? Color.orange : .secondary)
                }
                if !model.polishTestResult.isEmpty {
                    Text(model.polishTestResult)
                        .font(.callout)
                        .textSelection(.enabled)
                        .foregroundStyle(model.polishTestResult.hasPrefix("润色后") ? .primary : Color.orange)
                }
            } header: {
                Text("OpenAI 兼容接口")
            }

            Section {
                HStack {
                    Text("超时")
                    Slider(value: $timeout, in: 500...10000, step: 500) { Text("超时") } minimumValueLabel: { Text("0.5s") } maximumValueLabel: { Text("10s") }
                        .onChange(of: timeout) { new in model.setPolishTimeout(Int(new)) }
                    Text(String(format: "%.1fs", timeout / 1000)).monospacedDigit()
                }

            }
        }
        .formStyle(.grouped)
        .onAppear {
            endpoint = model.config.polish_endpoint
            modelName = model.config.polish_model
            if let p = ProviderPreset.all.first(where: { $0.endpoint == endpoint && $0.id != "custom" }) ?? ProviderPreset.all.first(where: { $0.endpoint.hasPrefix(endpoint) && $0.id != "custom" && !endpoint.isEmpty }) {
                presetId = p.id
            }
            level = model.config.polish_level
            timeout = Double(model.config.polish_timeout_ms)
        }
        .onChange(of: model.config.polish_level) { new in if new != level { level = new } }
        .alert("这段文字会离开本机", isPresented: Binding(
            get: { model.pendingPolishLevel != nil || model.pendingPolishEnabled },
            set: { if !$0 { model.cancelPolishConsent() } }
        )) {
            Button("取消", role: .cancel) { model.cancelPolishConsent() }
            Button("我明白，开启润色") { model.confirmPolishConsent() }
        } message: {
            Text("开启后每段文字会发到你配置的服务端")
                .font(.caption).foregroundStyle(.secondary)
                .help("语音和模型仍然只在本机；只有原文和整理后的文字会经过网络（比如 DeepSeek）。可以随时关掉。")
        }
    }
}

/// 常用 OpenAI 兼容服务商：省得用户去翻文档找 base URL 和模型名。
struct ProviderPreset: Identifiable {
    let id: String
    let name: String
    let endpoint: String
    let model: String

    static let all: [ProviderPreset] = [
        ProviderPreset(id: "deepseek", name: "DeepSeek", endpoint: "https://api.deepseek.com", model: "deepseek-chat"),
        ProviderPreset(id: "openai", name: "OpenAI", endpoint: "https://api.openai.com/v1", model: "gpt-4o-mini"),
        ProviderPreset(id: "moonshot", name: "月之暗面 Kimi", endpoint: "https://api.moonshot.cn/v1", model: "moonshot-v1-8k"),
        ProviderPreset(id: "zhipu", name: "智谱 GLM", endpoint: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4-flash"),
        ProviderPreset(id: "dashscope", name: "通义千问", endpoint: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus"),
        ProviderPreset(id: "siliconflow", name: "硅基流动", endpoint: "https://api.siliconflow.cn/v1", model: "Qwen/Qwen2.5-7B-Instruct"),
        ProviderPreset(id: "custom", name: "自定义", endpoint: "", model: ""),
    ]
}

// MARK: - 历史与统计

struct HistorySection: View {
    @ObservedObject var model: AppModel
    @State private var confirmClear = false
    @State private var pendingDelete: HistoryItem?
    @State private var query = ""

    private var filtered: [HistoryItem] {
        let q = query.trimmingCharacters(in: .whitespaces)
        guard !q.isEmpty else { return model.history }
        return model.history.filter {
            $0.text.localizedCaseInsensitiveContains(q) || $0.app.localizedCaseInsensitiveContains(q)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let s = model.stats {
                HStack(spacing: 18) {
                    stat("转写次数", "\(s.sessions)")
                    stat("总字数", "\(s.chars)")
                    stat("语音时长", "\(s.speechMinutes) 分钟")
                    stat("省下打字", "\(s.savedMinutes) 分钟")
                    if let lat = s.latency, let p50 = lat.p50, let p95 = lat.p95 {
                        stat("松手→出字", "\(p50) / \(p95) ms")
                    }
                    Spacer()
                }
                Text("省下打字按 40 字/分钟折算")
                    .font(.caption).foregroundStyle(.secondary)
                    .help("统计自 \(s.since)。最后一项是延时中位数 / p95（验收线 p95 ≤ 1500ms）。")
            }

            HStack {
                Toggle("记录历史", isOn: Binding(
                    get: { model.config.history_enabled },
                    set: { model.setHistoryEnabled($0) }
                ))
                Spacer()
                Button("重试上次失败") { model.retryFailed() }
                Button("清空历史") { confirmClear = true }
                    .disabled(model.history.isEmpty)
            }

            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                TextField("在历史里搜（文字或应用名）", text: $query)
                    .textFieldStyle(.plain)
                if !query.isEmpty {
                    Button {
                        query = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal, 8).padding(.vertical, 5)
            .background(RoundedRectangle(cornerRadius: 7).fill(Color.primary.opacity(0.06)))

            if model.history.isEmpty {
                Text("还没有记录。按住快捷键说一句试试。")
                    .font(.callout).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if filtered.isEmpty {
                Text("没有匹配「\(query)」的记录。")
                    .font(.callout).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List {
                    ForEach(filtered) { item in
                        HStack(alignment: .top, spacing: 10) {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(item.text.isEmpty ? "（没有识别出文字）" : item.text)
                                    .font(.system(size: 12))
                                    .lineLimit(2)
                                Text("\(item.timeText) · \(item.app.isEmpty ? "未知应用" : item.app) · \(item.statusText)")
                                    .font(.caption2).foregroundStyle(.secondary)
                            }
                            Spacer()
                            Button {
                                NSPasteboard.general.clearContents()
                                NSPasteboard.general.setString(item.text, forType: .string)
                                model.showToast("已复制这条")
                            } label: {
                                Image(systemName: "doc.on.doc")
                            }
                            .buttonStyle(.borderless)
                            .help("复制这条文字")
                            Button {
                                pendingDelete = item
                            } label: {
                                Image(systemName: "trash")
                            }
                            .buttonStyle(.borderless)
                            .help("删除这条记录")
                        }
                    }
                }
                .listStyle(.inset)
            }
        }
        .padding(4)
        .confirmationDialog("清空全部历史？", isPresented: $confirmClear, titleVisibility: .visible) {
            Button("清空 \(model.history.count) 条记录", role: .destructive) { model.clearHistory() }
            Button("取消", role: .cancel) {}
        } message: {
            Text("只删本机记录，不影响已经粘贴出去的文字。")
        }
        .confirmationDialog("删除这条记录？", isPresented: Binding(
            get: { pendingDelete != nil },
            set: { if !$0 { pendingDelete = nil } }
        ), titleVisibility: .visible) {
            Button("删除", role: .destructive) {
                if let item = pendingDelete { model.deleteHistory(item.id) }
                pendingDelete = nil
            }
            Button("取消", role: .cancel) { pendingDelete = nil }
        } message: {
            Text(pendingDelete.map { "「\($0.text.prefix(40))」" } ?? "")
        }
        .onAppear { model.refreshStats(); model.refreshHistory() }
    }

    private func stat(_ title: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(value).font(.system(size: 18, weight: .semibold)).monospacedDigit()
            Text(title).font(.caption).foregroundStyle(.secondary)
        }
    }
}

// MARK: - 诊断

// MARK: - 热词（M2，DESIGN §4.3）

struct HotwordsSection: View {
    @ObservedObject var model: AppModel
    @State private var draft: [HotwordRule] = []
    /// 不设 @State dirty：打字时任何父视图状态变化都会重建 List，把中文输入法的组合态打断。
    /// 直接用「草稿 vs 已保存」比较算出来。
    private var dirty: Bool { draft != model.hotwords }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {

            if !model.suggestions.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("自动学到的改法（尚未采纳）")
                        .font(.callout).bold()
                    ForEach(model.suggestions) { s in
                        HStack(spacing: 8) {
                            Text(s.from).monospaced()
                            Image(systemName: "arrow.right").foregroundStyle(.secondary)
                            Text(s.to).monospaced()
                            Spacer()
                            Button("采纳") { model.acceptSuggestion(s) }
                                .buttonStyle(.borderedProminent)
                            Button("忽略") { model.dismissSuggestion(s) }
                                .buttonStyle(.borderless)
                        }
                        .padding(8)
                        .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 8))
                    }
                }
            }

            // 不用 List：List 的行会随状态刷新重建，文本框在 macOS 上会掉焦点/打断输入法。
            // 这里用一个纵向堆叠 + 分隔线，行的身份由 HotwordRule.id 决定，打字时不重建。
            ScrollView {
                VStack(spacing: 0) {
                    ForEach($draft) { $rule in
                        HStack(spacing: 10) {
                            TextField("想听到的词", text: $rule.to)
                                .textFieldStyle(.roundedBorder)
                                .font(.body)
                            if !rule.from.isEmpty && rule.from != rule.to {
                                Text("自动学到：\(rule.from) → \(rule.to)")
                                    .font(.caption).foregroundStyle(.tertiary)
                                    .lineLimit(1)
                            }
                            Button {
                                draft.removeAll { $0.id == rule.id }
                            } label: {
                                Image(systemName: "trash")
                            }
                            .buttonStyle(.borderless)
                            .help("删除这条")
                        }
                        .padding(.vertical, 5)
                        Divider().opacity(0.4)
                    }
                }
            }
            .frame(minHeight: 200, maxHeight: 320)

            HStack(spacing: 10) {
                Button("添加一个词") {
                    draft.append(HotwordRule(from: "", to: ""))
                }
                .help("写你希望它出现的词就够了，例如「深度求索」。不用自己想会被听错成什么：程序会从历史里学，学到后点「采纳」就挂到这个词上")
                Spacer()
                if dirty {
                    Text("有未保存的改动").font(.caption).foregroundStyle(.orange)
                }
                Button("保存") {
                    // 单列模型：空 from 归一成 from == to（等价规则）
                    let normalized = draft.map {
                        HotwordRule(from: $0.from.isEmpty ? $0.to : $0.from, to: $0.to)
                    }
                    model.saveHotwords(normalized)
                    draft = model.hotwords
                }
                .keyboardShortcut("s", modifiers: .command)
                .disabled(!dirty)
            }

            if !model.hotwordsError.isEmpty {
                Label(model.hotwordsError, systemImage: "exclamationmark.triangle.fill")
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .textSelection(.enabled)
            }

        }
        .padding(.top, 4)
        .onAppear { draft = model.hotwords }
        .onChange(of: model.hotwords) { new in
            if draft != new { draft = new }
        }
    }
}

struct DiagnoseSection: View {
    @ObservedObject var model: AppModel
    @State private var toml = ""
    @State private var showToml = false
    @State private var permissionTick = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            PermissionMatrix(tick: permissionTick) { permissionTick += 1 }
            HStack {
                Button("刷新") {
                    model.diagnose = model.core.diagnose() ?? ""
                    model.refreshAll()
                }
                Button("复制诊断信息") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(model.diagnose, forType: .string)
                    model.showToast("已复制")
                }
                Button("麦克风设置") { open("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone") }
                Button("辅助功能设置") { open("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility") }
                Spacer()
                Toggle("编辑 config.toml", isOn: $showToml)
            }

            if showToml {
                TextEditor(text: Binding(
                    get: { toml.isEmpty ? (model.core.configToml() ?? "") : toml },
                    set: { toml = $0 }
                ))
                .font(.system(size: 11, design: .monospaced))
                .frame(minHeight: 220)
                HStack {
                    Button("保存 TOML") {
                        let (code, error) = model.core.setConfigToml(toml)
                        if code == 0 {
                            model.showToast("已保存")
                            model.refreshAll()
                        } else {
                            model.showToast("保存失败：\(error ?? "未知错误")（旧配置保留）")
                        }
                    }
                    Button("重新载入") { toml = model.core.configToml() ?? "" }
                    Spacer()
                }
                if !model.lanLog.isEmpty {
                    lanLogView
                }
            } else {
                ScrollView {
                    Text(model.diagnose.isEmpty ? "（点「刷新」生成）" : model.diagnose)
                        .font(.system(size: 11.5, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(8)
                .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.04)))
            }
        }
        .padding(4)
        .onAppear {
            model.diagnose = model.core.diagnose() ?? ""
            model.refreshLanLog()
        }
    }

    /// 局域网请求摘要：只显示时间/来源/路径/状态/耗时，**不显示内容**（也不存内容）
    private var lanLogView: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("最近的局域网请求").font(.system(size: 11, weight: .semibold))
                Text("（不记内容，只看状态码排查）").font(.system(size: 10)).foregroundColor(.secondary)
                Spacer()
                Button("刷新") { model.refreshLanLog() }.controlSize(.small)
            }
            ForEach(model.lanLog.suffix(6).reversed()) { e in
                HStack(spacing: 8) {
                    Text(e.timeText).font(.system(size: 10, design: .monospaced)).foregroundColor(.secondary)
                    Text(e.src).font(.system(size: 10, design: .monospaced))
                    Text(e.path).font(.system(size: 10, design: .monospaced)).foregroundColor(.secondary)
                    Text("\(e.code)").font(.system(size: 10, weight: .semibold, design: .monospaced))
                        .foregroundColor(e.code == 200 ? .green : (e.code == 503 ? .orange : .red))
                    Text("\(e.ms)ms").font(.system(size: 10, design: .monospaced)).foregroundColor(.secondary)
                }
            }
        }
        .padding(8)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.04)))
    }

    private func open(_ url: String) {
        if let u = URL(string: url) { NSWorkspace.shared.open(u) }
    }
}

// MARK: - 权限矩阵

/// 三行权限：状态 + 后果 + 去设置。缺哪一项就说清缺了会怎样（不喊「权限不足」）
struct PermissionMatrix: View {
    var tick: Int = 0
    var onChange: () -> Void = {}

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("权限").font(.system(size: 12, weight: .semibold))
                Spacer()
                Button("重新检查") { onChange() }
                    .buttonStyle(.borderless)
                    .font(.caption)
                Button("重新授权…") {
                    Permission.reauthorizeWithConfirm()
                    onChange()
                }
                .buttonStyle(.borderless)
                .font(.caption)
                .help("系统里可能还记着已删除副本的授权。这个按钮会清掉旧记录并重新请求辅助功能 / 输入监控")
            }
            ForEach(Permission.allCases, id: \.rawValue) { p in
                let state = p.state
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: state.ok ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                        .foregroundStyle(state.ok ? Color.green : Color.orange)
                        .font(.system(size: 12))
                    VStack(alignment: .leading, spacing: 1) {
                        HStack(spacing: 6) {
                            Text(p.title).font(.system(size: 12, weight: .medium))
                            Text(state.text).font(.caption2).foregroundStyle(.secondary)
                            if state == .notAsked, p == .microphone {
                                Button("请求") { Permission.requestMicrophone { onChange() } }
                                    .buttonStyle(.borderless).font(.caption2)
                            }
                        }
                        if !state.ok {
                            Text(p.consequence).font(.caption2).foregroundStyle(.secondary)
                        }
                    }
                    Spacer()
                    if !state.ok {
                        Button("去设置") { Permission.openSettings(p); onChange() }
                            .buttonStyle(.borderless).font(.caption2)
                    }
                }
            }
        }
        .padding(8)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.04)))
        .id(tick)
    }
}

// MARK: - 按键录制

/// 把 SwiftUI 的焦点转给一个能收 keyDown 的 NSView
struct KeyRecorder: NSViewRepresentable {
    @Binding var isActive: Bool
    var onKey: (NSEvent) -> Void

    func makeNSView(context: Context) -> RecorderView {
        let v = RecorderView()
        v.onKey = onKey
        return v
    }

    func updateNSView(_ v: RecorderView, context: Context) {
        v.onKey = onKey
        v.wantsFocus = isActive
        if isActive {
            DispatchQueue.main.async { v.window?.makeFirstResponder(v) }
        }
    }

    final class RecorderView: NSView {
        var onKey: ((NSEvent) -> Void)?
        var wantsFocus = false

        override var acceptsFirstResponder: Bool { wantsFocus }

        override func keyDown(with event: NSEvent) {
            onKey?(event)
        }

        override func flagsChanged(with event: NSEvent) {
            // 单修饰键（右 Option / Fn…）也算一次按键：按下那一刻就交付
            let code = CGKeyCode(event.keyCode)
            guard HotkeySpec.isModifierKeyCode(code) else { return }
            let flags = event.modifierFlags
            let pressed: Bool
            switch code {
            case 58, 61: pressed = flags.contains(.option)
            case 55, 54: pressed = flags.contains(.command)
            case 56, 60: pressed = flags.contains(.shift)
            case 59, 62: pressed = flags.contains(.control)
            default: pressed = true
            }
            guard pressed else { return }
            onKey?(event)
        }
    }
}
