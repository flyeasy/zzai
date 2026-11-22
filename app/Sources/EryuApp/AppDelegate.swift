import AppKit
import AVFoundation
import Darwin

func log(_ s: String) {
    FileHandle.standardError.write(("[eryu] " + s + "\n").data(using: .utf8)!)
}

final class InstanceLock {
    private var fd: Int32 = -1
    /// 返回 false 表示已有实例在跑
    func acquire(at path: String) -> Bool {
        fd = open(path, O_CREAT | O_RDWR, 0o600)
        guard fd >= 0 else { return true } // 拿不到锁文件也不阻塞启动
        return flock(fd, LOCK_EX | LOCK_NB) == 0
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var core: Core?
    private var hotkey: Hotkey?
    private var profileHotkeys: ProfileHotkeys?
    private var overlay: OverlayController?
    /// 录音中按 Esc 取消（只旁听，不拦截；需要与热键同样的权限）
    private var escTap: KeyTap?
    private var permTimer: Timer?
    private var lastMissingPerms = ""
    /// 隐私模式：不显示识别文本（DESIGN §10.2）
    private var privacyOn = false
    /// 壳侧记住的阶段：睡眠/唤醒时要判断「是不是正录着」，避免 key-up 丢了卡在 Listening
    private var lastPhase = "idle"
    /// 快速连按检测（DESIGN §9.4）：短按后 400ms 内再按 = 取回上次结果，不开始新录音
    private var quickTapTimer: DispatchWorkItem?
    private var quickTapDownAt: Date?
    /// 处于「短按待定」：已经按下但还没把 up 交给 core
    private var quickTapPending = false
    /// 自测用：双击是否被识别
    var hotkeyDoubleTapped = false
    /// 选区指令管线（DESIGN §10.2）
    private var selectionLane: SelectionLane?
    /// 注入后编辑追踪（热词自动学习，DESIGN §10.2）
    private var editObserver = EditObserver()
    /// 录音时暂停其它播放器（DESIGN §10.2）
    private let mediaPause = MediaPause()
    private var pullDone: String?
    /// `--edit-observe-test`：自建输入框 → 上屏一段 → 1.2s 后"用户改词" → 看是否学到候选
    private func runEditObserveTest() {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 560, height: 160),
                              styleMask: [.titled], backing: .buffered, defer: false)
        window.title = "字在 编辑追踪自测"
        let tv = NSTextView(frame: NSRect(x: 10, y: 10, width: 540, height: 140))
        tv.font = .systemFont(ofSize: 14)
        let injected = "我们用深度求索来写代码"
        tv.string = injected
        window.contentView = tv
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        window.makeFirstResponder(tv)

        let observer = EditObserver()
        self.editObserver = observer
        var got = false
        observer.onSuggestion = { s in
            got = true
            print("EDITTEST OK 学到候选：\(s.from) → \(s.to)")
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { exit(0) }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
            observer.watch(expected: injected, element: AX.focusedElement())
            // 模拟用户把「深度求索」改成「DeepSeek」
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) {
                tv.string = "我们用DeepSeek来写代码"
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 4.5) {
                if !got {
                    print("EDITTEST FAIL 没学到候选（4.5s 内没检测到改动）")
                    exit(2)
                }
            }
        }
    }

    /// `--selection-test [--replace 文本]`：自建窗口选区 → AX 读 → AX 写回 → 用 NSTextView 原文核对
    private func runSelectionTest() {
        let args = CommandLine.arguments
        let replaceAt = args.firstIndex(of: "--replace").map { $0 + 1 }
        let replaceWith = replaceAt.flatMap { $0 < args.count ? args[$0] : nil }

        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 520, height: 160),
                              styleMask: [.titled], backing: .buffered, defer: false)
        window.title = "字在 选区自测"
        let tv = NSTextView(frame: NSRect(x: 10, y: 10, width: 500, height: 140))
        tv.string = "这是一段用来测试选区读写的文字，包含第三会议室和周三下午。"
        tv.font = .systemFont(ofSize: 14)
        window.contentView = tv
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        window.makeFirstResponder(tv)
        tv.setSelectedRange(NSRange(location: 0, length: 20))

        DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) {
            guard let sel = AX.selection() else {
                print("SELECTION FAIL 读不到选区（AX 未授权或控件不暴露 AXSelectedText）")
                exit(2)
            }
            print("SELECTION OK \(sel.text.count) 字：\(sel.text.prefix(60))")
            guard let newText = replaceWith else { exit(0) }
            let ok = AX.replaceSelection(newText, element: sel.element)
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                let back = tv.string.contains(newText)
                print("REPLACE \(ok ? "OK" : "FAIL") 写入 \(newText.count) 字；文本视图回读 \(back ? "OK" : "MISMATCH")")
                print("DOCNOW：\(tv.string.prefix(60))")
                exit(ok && back ? 0 : 3)
            }
        }
    }

    /// 仅 --demo-palette 截图自测用
    private var paletteDemo: CommandPalette?
    private let injector = Injector()
    /// 按热键时锁定的上屏目标（I-173）：说话期间弹了别的窗口，松手也不跟过去
    private var speechTarget: Injector.Target?
    private var status: StatusController?
    private var model: AppModel?
    private var settings: SettingsWindowController?
    private var onboarding: OnboardingController?
    private var mini: MiniWindowController?
    /// `--onboarding-selftest`：判定函数 + （给了 ERYU_MIC_WAV 时）引导第 3 步真实闭环
    private var onboardingSelftest = false
    private var onboardingSelftestOK = true
    private let lock = InstanceLock()

    private let selfTest = CommandLine.arguments.contains("--self-test")
    private var selfTestExpectIdle = 0
    private var selfTestSeen: [String] = []

    /// `--mic-test N`：不按热键，直接录 N 秒走完整链路（验证麦克风 + 真实识别）
    private var micTestSecs: Int32? {
        let args = CommandLine.arguments
        guard let i = args.firstIndex(of: "--mic-test"), i + 1 < args.count else { return nil }
        return Int32(args[i + 1])
    }
    /// 缺权限时的引导：一次问清三项，点「去授权」就弹出系统界面
    private func promptForPermissions(_ missing: [Permission]) {
        let alert = NSAlert()
        alert.messageText = "字在还需要 \(missing.count) 项系统权限"
        var body = "这些开关由 macOS 管控，只有你能打开（一次授权后不会再问）：\n\n"
        for (i, p) in missing.enumerated() {
            body += "\(i + 1). \(p.title) — \(p.consequence)\n"
        }
        body += "\n点「去授权」会弹出系统的授权窗口并打开设置页。麦克风立刻生效；「辅助功能 / 输入监控」授权后如果没变化，用菜单里的「退出并重新打开」重启一次 Eryu。"
        alert.informativeText = body
        alert.addButton(withTitle: "去授权")
        alert.addButton(withTitle: "稍后")
        if alert.runModal() == .alertFirstButtonReturn {
            Permission.requestAll()
        }
    }

    /// `--inject-test`：在自建文本框里验证「写剪贴板 → ⌘V → 还原剪贴板」真的对真实应用生效
    /// （不会碰你的其它窗口：目标是我们自己弹出的测试窗口）
    private func runInjectTest() {
        let text = "字在 上屏自测：你好，世界。"
        if !CommandLine.arguments.contains("--inject-global") {
            injector.targetPid = ProcessInfo.processInfo.processIdentifier
        }
        let before = NSPasteboard.general.string(forType: .string) ?? ""

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 520, height: 160),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        window.title = "字在 上屏自测"
        let textView = NSTextView(frame: NSRect(x: 0, y: 0, width: 520, height: 160))
        textView.font = .systemFont(ofSize: 14)
        window.contentView = textView
        window.center()
        NSApp.setActivationPolicy(.regular)
        NSRunningApplication.current.activate(options: [.activateAllWindows, .activateIgnoringOtherApps])
        window.makeKeyAndOrderFront(nil)
        textView.window?.makeFirstResponder(textView)

        // 只有真的成为前台应用才发 ⌘V——否则会粘到别的应用里（绝不能冒这个险）
        var waited = 0.0
        func waitActive() {
            if NSApp.isActive || waited > 2.0 {
                guard NSApp.isActive else {
                    let line = "INJECTTEST FAIL 无法把测试窗口激活为前台（跳过，避免粘到别的应用）"
                    print(line); Self.writeTestLog(line); exit(1)
                }
                run()
                return
            }
            waited += 0.1
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { waitActive() }
        }
        func run() {
            self.injector.inject(text: text, mode: "paste") { [weak self] result in
                guard let self else { return }
                switch result {
                case .ok:
                    // 等 ⌘V 生效 + 剪贴板还原完成（750ms）
                    DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) {
                        let got = textView.string
                        let after = NSPasteboard.general.string(forType: .string) ?? ""
                        let textOK = got.contains(text)
                        let clipOK = after == before
                        let line = "INJECTTEST \(textOK && clipOK ? "OK" : "FAIL") 文本框=\(got.debugDescription) 剪贴板还原=\(clipOK) keyWindow=\(window.isKeyWindow)"
                        print(line)
                        Self.writeTestLog(line)
                        exit(textOK && clipOK ? 0 : 1)
                    }
                case .undelivered(let why), .unknown(let why):
                    print("INJECTTEST FAIL 未送达：\(why)")
                    Self.writeTestLog("INJECTTEST FAIL 未送达：\(why)")
                    exit(1)
                }
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { waitActive() }
    }

    /// CLI 自测结果写文件（用 `open` 启动时 stdout 不可见）
    static func writeTestLog(_ line: String) {
        let path = "/tmp/eryu-test.log"
        let entry = line + "\n"
        if let handle = FileHandle(forWritingAtPath: path) {
            handle.seekToEndOfFile()
            handle.write(entry.data(using: .utf8)!)
            try? handle.close()
        } else {
            try? entry.write(toFile: path, atomically: true, encoding: .utf8)
        }
    }

    /// `--hotkey-selftest`：自己合成一次「右 Option 按下/松开」，验证热键→录音→识别→提交整条链路（不上屏）
    private var hotkeySelfTest = false
    /// --acceptance：用户按一次真实热键，把「浮层有没有出、流式有没有字、松手有没有结果」记录下来
    private var acceptance: AcceptanceRun?
    /// 最近一次热键注册失败的原因（nil = 成功）
    private var hotkeyRegistrationReason: String?
    private var hotkeyDownCount = 0
    private var hotkeyUpCount = 0
    private var hotkeySubmitCount = 0

    private var micTestText = ""
    private var micTestDone = false

    /// `--diagnose`：打印诊断信息后退出（`eryu diagnose` 的等价位，便于反馈问题）
    private var cliDiagnose = false
    private var cliBench = false
    /// `--dump-config`：打印当前 config.toml 后退出
    private var cliDumpConfig = false
    /// `--show-settings`：启动后直接打开设置窗口（调试/截图用）
    private var showSettings = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        setvbuf(stdout, nil, _IOLBF, 0)
        installEditMenu()

        // 先判断参数：不认识的 --flag 直接退出（以前会掉进交互模式，真开麦克风）
        if let first = Self.unknownFlag(CommandLine.arguments) {
            print("不认识的参数：\(first)")
            print("可用参数见 README 的「自测命令」；看版本用 --version，看诊断用 --diagnose。")
            exit(2)
        }
        if CommandLine.arguments.contains("--perm-report") {
            print(PermReport.write())
            exit(0)
        }
        if CommandLine.arguments.contains("--version") || CommandLine.arguments.contains("--help") {
            let v = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "?"
            print("Eryu \(v)")
            print("自测：--self-test / --acceptance / --decode-wav <wav> / --diagnose（完整列表见 README）")
            exit(0)
        }

        Core.keepABISymbolsAlive()
        log("Eryu 启动，core ABI v\(Core.abiMajor).\(Core.abiMinor)")

        // 观测型 CLI（自测/验收/解码/诊断…）不参与单实例锁：否则正常运行时想跑一次验收会被
        // 「已有 Eryu 实例在运行」直接挡掉（冒烟里就踩过：3g 输出为空）
        let observational = ["--self-test", "--simulate", "--acceptance", "--acceptance-synthetic",
                             "--decode-wav", "--diagnose",
                             "--dump-config", "--mic-test", "--soak", "--bench", "--segment-test",
                             "--verify-test", "--type-event-test", "--double-tap-test", "--single-tap-test",
                             "--edit-observe-test", "--sleep-test", "--hotkey-selftest", "--baseline",
                             "--print-last", "--voice-state", "--voice-enroll-wav", "--voice-clear",
                             "--pull-model", "--settings-live-test", "--fallback-test", "--privacy-test", "--degrade-test",
                             "--contract", "--lan-serve", "--lan-info", "--lan-log",
                             "--onboarding-step", "--onboarding-selftest"]
        let observationalRun = CommandLine.arguments.contains { arg in
            observational.contains { arg == $0 || arg.hasPrefix($0 + "=") }
        }
        if !selfTest, !observationalRun, !lock.acquire(at: NSTemporaryDirectory() + "eryu.lock") {
            log("已有 Eryu 实例在运行，退出")
            NSApp.terminate(nil)
            return
        }

        guard let core = Core() else {
            log("核心初始化失败（ABI 不兼容或创建失败）")
            NSApp.terminate(nil)
            return
        }
        // Dock / 关于窗口 / 授权弹窗里显示的图标（bundle 里是 Eryu.icns）
        if let icon = NSImage(named: "Eryu.icns") ?? Bundle.main.image(forResource: "Eryu") {
            NSApp.applicationIconImage = icon
        }
        self.core = core
        // 自动化路径不写历史、不落盘：否则每跑一次冒烟就把统计和「历史」弄脏
        let testFlags = ["--self-test", "--decode-wav", "--soak", "--bench", "--mic-test",
                         "--segment-test", "--verify-test", "--type-event-test",
                         "--double-tap-test", "--single-tap-test", "--edit-observe-test",
                         "--acceptance", "--acceptance-synthetic", "--simulate", "--onboarding-selftest"]
        if testFlags.contains(where: { CommandLine.arguments.contains($0) }) {
            core.setTestMode(true)
        }
        // 自测绝不许把测试句子粘进用户正在用的应用（曾经发生过：--mic-test 真的往 DSH 输入框里贴了文字）
        if ["--mic-test", "--decode-wav", "--soak", "--bench", "--simulate", "--acceptance", "--onboarding-selftest"].contains(where: { CommandLine.arguments.contains($0) }) {
            injector.dryRun = true
        }
        // 浮层尽早建：所有 CLI 自测分支都在后面 return，晚建的话自测里看不到浮层
        overlay = OverlayController()
        overlay?.profileProvider = { [weak self] in
            let p = self?.model?.profiles.first(where: { $0.active })
            return (p?.icon ?? "sparkles", p?.name ?? "日常")
        }

        if selfTest {
            injector.dryRun = true
            startSelfTest(core)
            return
        }

        if let secs = micTestSecs {
            requireEngine("MICTEST")
            injector.dryRun = true
            startMicTest(core, seconds: secs)
            return
        }

        cliBench = CommandLine.arguments.contains("--bench")
        cliDiagnose = CommandLine.arguments.contains("--diagnose")
        cliDumpConfig = CommandLine.arguments.contains("--dump-config")
        showSettings = CommandLine.arguments.contains("--show-settings")
            || CommandLine.arguments.contains { $0.hasPrefix("--show-settings=") }
        if CommandLine.arguments.contains("--ask-permissions") {
            Permission.requestAll()
            Thread.sleep(forTimeInterval: 1.0)
            let states = Permission.allCases.map { "\($0.title)=\($0.state.text)" }.joined(separator: " ")
            print("PERMCHECK \(states)")
            Self.writeTestLog("PERMCHECK \(states)")
            core.onEvent = nil
            self.core = nil
            exit(0)
        }
        // 热词页输入框焦点稳定性自测：聚焦第一个框，逐字打 "abcde"（模拟输入法逐键），
        // 再看焦点是否还在同一个框、内容是否连成串
        if CommandLine.arguments.contains("--hotword-type-test") {
            showSettings = true
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.8) { [weak self] in
                guard let self else { return }
                NSApp.activate(ignoringOtherApps: true)
                self.settings?.present()
                Thread.sleep(forTimeInterval: 0.8)
                guard let el = Self.firstEditable(in: getpid()) else {
                    print("HWTYPE FAIL 没找到输入框"); exit(1)
                }
                AXUIElementSetAttributeValue(el, kAXFocusedAttribute as CFString, kCFBooleanTrue)
                let src = CGEventSource(stateID: .hidSystemState)
                for ch in "abcde" {
                    var buf = Array(String(ch).utf16)
                    let down = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: true)
                    down?.keyboardSetUnicodeString(stringLength: buf.count, unicodeString: &buf)
                    down?.post(tap: .cghidEventTap)
                    let up = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: false)
                    up?.keyboardSetUnicodeString(stringLength: buf.count, unicodeString: &buf)
                    up?.post(tap: .cghidEventTap)
                    Thread.sleep(forTimeInterval: 0.08)
                }
                Thread.sleep(forTimeInterval: 0.4)
                let focused = AX.focusedElement()
                let v = focused.flatMap { AX.value(of: $0) } ?? ""
                let same = (focused.map { CFEqual($0, el) }) ?? false
                print("HWTYPE 内容=\(v) 焦点还在这格=\(same)")
                _ = self
                exit(v == "abcde" && same ? 0 : 1)
            }
            return
        }
        // 在自己的窗口里能不能上屏：设置里选中一个多行输入框，直接注入，看字落不落进去
        if let i = CommandLine.arguments.firstIndex(of: "--self-paste-test"), i + 1 < CommandLine.arguments.count {
            let text = CommandLine.arguments[i + 1]
            showSettings = true
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.6) { [weak self] in
                guard let self else { return }
                guard let el = Self.firstEditable(in: getpid()) else {
                    print("SELFPASTE FAIL 没找到可编辑控件")
                    exit(1)
                }
                AXUIElementSetAttributeValue(el, kAXFocusedAttribute as CFString, kCFBooleanTrue)
                self.injector.inject(text: text, mode: "paste", target: nil) { result in
                    var line = "SELFPASTE result="
                    switch result {
                    case .ok: line += "ok"
                    case .undelivered(let why): line += "undelivered(\(why))"
                    case .unknown(let why): line += "unknown(\(why))"
                    }
                    let v = AX.value(of: el) ?? ""
                    line += " 落到输入框=\(v.contains(text)) 值=\(v.prefix(40))"
                    print(line)
                    exit(v.contains(text) ? 0 : 1)
                }
            }
            return
        }
        // I-173 焦点锁定自测：抓住「说话时」的前台应用，再故意切到别的应用，看松手后字落在哪
        if let i = CommandLine.arguments.firstIndex(of: "--focus-lock-test"), i + 1 < CommandLine.arguments.count {
            let text = CommandLine.arguments[i + 1]
            let script = Process()
            script.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
            script.arguments = ["-e", "tell application \"TextEdit\" to activate", "-e", "tell application \"TextEdit\" to make new document"]
            try? script.run()
            script.waitUntilExit()
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) { [weak self] in
                guard let self else { return }
                let target = Injector.captureTarget()
                let captured = target?.name ?? "nil"
                // 故意把前台切到 Finder（模拟「说话中弹出别的窗口」）
                NSWorkspace.shared.launchApplication("Finder")
                _ = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.finder").first?.activate(options: [.activateIgnoringOtherApps])
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { [weak self] in
                    guard let self else { return }
                    let nowFront = NSWorkspace.shared.frontmostApplication?.localizedName ?? "?"
                    self.injector.inject(text: text, mode: "paste", target: target) { result in
                        var line = "FOCUSLOCK target=\(captured) switchedTo=\(nowFront) result="
                        switch result {
                        case .ok: line += "ok"
                        case .undelivered(let why): line += "undelivered(\(why))"
                        case .unknown(let why): line += "unknown(\(why))"
                        }
                        DispatchQueue.main.asyncAfter(deadline: .now() + 0.9) {
                            let front = NSWorkspace.shared.frontmostApplication?.localizedName ?? "?"
                            var got = ""
                            if let t = target {
                                let ax = AXUIElementCreateApplication(t.pid)
                                var raw: CFTypeRef?
                                if AXUIElementCopyAttributeValue(ax, kAXFocusedUIElementAttribute as CFString, &raw) == .success,
                                   let rawEl = raw,
                                   let el = (rawEl as! AXUIElement) as AXUIElement?,
                                   let v = AX.value(of: el) {
                                    got = v
                                }
                            }
                            line += " frontAfter=\(front) 文本框含结果=\(got.contains(text)) 文本=\(got.prefix(40))"
                            print(line); Self.writeTestLog(line)
                            exit(0)
                        }
                    }
                }
            }
            return
        }
        if let i = CommandLine.arguments.firstIndex(of: "--inject-paste"), i + 1 < CommandLine.arguments.count {
            let text = CommandLine.arguments[i + 1]
            let pre = "PREFLIGHT listen=\(CGPreflightListenEventAccess()) post=\(CGPreflightPostEventAccess()) ax=\(AXIsProcessTrusted()) front=\(NSWorkspace.shared.frontmostApplication?.localizedName ?? "?")"
            print(pre)
            Self.writeTestLog(pre)
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
                guard let self else { return }
                self.injector.inject(text: text, mode: "paste") { result in
                    let line: String
                    switch result {
                    case .ok: line = "PASTE OK"
                    case .undelivered(let why): line = "PASTE FAIL undelivered: \(why)"
                    case .unknown(let why): line = "PASTE FAIL unknown: \(why)"
                    }
                    print(line)
                    Self.writeTestLog(line)
                    DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { exit(0) }
                }
            }
            return
        }
        if CommandLine.arguments.contains("--sleep-test") {
            // 直接调处理器（不能真让机器睡），验证「录音中会收尾、唤醒会重挂热键」
            if hotkey == nil {
                let h = Hotkey()
                h.onDown = { [weak self] in self?.handleHotkeyDown() }
                h.onUp = { [weak self] in self?.handleHotkeyUp() }
                hotkey = h
            }
            var registered = false
            var why = ""
            if let h = hotkey {
                let spec = model?.config.hotkey ?? "ALT_R"
                let (ok, reason) = h.apply(spec: spec)
                registered = ok
                why = reason ?? ""
                if !ok { log("自测重挂未成功：\(why)") }
            }
            lastPhase = "listening"
            handleWillSleep()
            lastPhase = "idle"
            handleDidWake()
            let line = "SLEEPTEST OK 睡眠收尾=已取消 唤醒重挂=\(registered) 原因=\(why)"
            print(line); Self.writeTestLog(line); exit(0)
        }
        if CommandLine.arguments.contains("--inject-test") {
            runInjectTest()
            return
        }
        if CommandLine.arguments.contains("--perm-check") {
            func line(_ name: String, _ granted: Bool, _ how: String) {
                let text = "\(granted ? "✅" : "❌") \(name)：\(granted ? "已授权" : "未授权")（\(how)）"
                print(text)
                Self.writeTestLog(text)
            }
            let mic = AVCaptureDevice.authorizationStatus(for: .audio) == .authorized
            line("麦克风", mic, "系统设置 → 隐私与安全性 → 麦克风")
            line("辅助功能", AXIsProcessTrusted(), "上屏注入 + 单键热键都要")
            let listen = CGPreflightListenEventAccess()
            line("输入监控", listen, "单键热键（监听键盘）")
            print("  （辅助功能/输入监控改动后需要重启 Eryu 才生效）")
            Self.writeTestLog("PERMCHECK 完")
            core.onEvent = nil
            self.core = nil
            exit(0)
        }
        if CommandLine.arguments.contains("--voice-state") {
            // 声纹状态：有没有注册、模型在不在、阈值多少（给脚本/排查用）
            print(core.voiceState() ?? "{}")
            core.onEvent = nil
            self.core = nil
            exit(0)
        }
        if let i = CommandLine.arguments.firstIndex(of: "--voice-enroll-wav"), i + 1 < CommandLine.arguments.count {
            // 用一段 16k 单声道 WAV 注册声纹（真实注册在设置页录音，这里给自动化留口子）
            let (code, msg) = core.voiceEnrollWav(CommandLine.arguments[i + 1])
            print(code == 0 ? "VOICEENROLL OK \(msg ?? "")" : "VOICEENROLL FAIL \(code) \(msg ?? "")")
            core.onEvent = nil
            self.core = nil
            exit(code == 0 ? 0 : 1)
        }
        if CommandLine.arguments.contains("--voice-clear") {
            let (code, msg) = core.voiceClear()
            print(code == 0 ? "VOICECLEAR OK" : "VOICECLEAR FAIL \(code) \(msg ?? "")")
            core.onEvent = nil
            self.core = nil
            exit(code == 0 ? 0 : 1)
        }
        if let i = CommandLine.arguments.firstIndex(of: "--decode-wav"), i + 1 < CommandLine.arguments.count {
            // 无麦克风的确定性自测：把一段 16k WAV 当成一次录音，走完整解码/失败/重试链路
            let path = CommandLine.arguments[i + 1]
            let fault = ProcessInfo.processInfo.environment["ERYU_FAULT_FAIL_SEGMENTS"]
            core.onEvent = { [weak self] event in
                switch event.kind {
                case "notice":
                    print("WAVNOTICE \(event.code) \(event.message)")
                case "result":
                    print("WAVRESULT \(event.string)")
                case "failed_segments":
                    let idx = (event.payload["indexes"] as? [Int]) ?? []
                    print("WAVFAILED \(idx)")
                    if let first = idx.first, let c = self?.core {
                        let (code, err) = c.retrySegment(first)
                        print("WAVRETRY 第 \(first) 段 返回=\(code) \(err ?? "")")
                    }
                case "segment_result":
                    let txt = (event.payload["text"] as? String) ?? ""
                    print("WAVSEGRESULT \(txt)")
                    self?.exitWavTest(txt.isEmpty ? 1 : 0)
                    return
                case "submit_request":
                    // 崩溃恢复钩子：此刻结果已落盘（core: result → save_pending_result → submit_request）
                    if ProcessInfo.processInfo.environment["ERYU_KILL_ON_SUBMIT"] != nil {
                        fflush(stdout)
                        kill(getpid(), SIGKILL)
                    }
                    // CLI 自测没有注入器：直接回 undelivered，别让它等 2s 超时
                    let tid = event.taskId
                    if !tid.isEmpty { _ = self?.core?.submitResult(taskId: tid, status: "undelivered", detail: "cli") }
                case "error":
                    if event.code.hasPrefix("partial_result") { print("WAVPARTIAL \(event.message)") }
                    else if event.code.hasPrefix("submit_") { print("WAVSUBMIT \(event.code)（CLI 无注入器，属预期）") }
                    else { print("WAVERROR \(event.code): \(event.message)"); self?.exitWavTest(2); return }
                default: break
                }
                if event.kind == "state", event.phase == "idle", fault == nil {
                    print("WAVTEST OK")
                    self?.exitWavTest(0)
                }
            }
            // 没有模型时引擎会回退 stub，stub 会造出「测试文本：你好，Eryu。」——
            // 对 --decode-wav 这种"拿真音频验真识别"的命令来说，那是假绿，必须拦住。
            requireEngine("WAVTEST")
            let (code, err) = core.decodeWav(path)
            if code != 0 {
                print("WAVTEST FAIL 受理失败（\(code)）\(err ?? "")")
                exit(1)
            }
            // 超时按音频长度放大：Qwen3 在长音频上 RTF 能到 0.36（75s 要解 27s），固定 25s 会假超时
            let secs = (try? AVAudioFile(forReading: URL(fileURLWithPath: path)))
                .map { Double($0.length) / $0.fileFormat.sampleRate } ?? 0
            let budget = max(25.0, secs * 1.2 + 20)
            DispatchQueue.main.asyncAfter(deadline: .now() + budget) {
                print(String(format: "WAVTEST FAIL 超时（%.0fs 预算）", budget))
                exit(1)
            }
            return
        }
        if let i = CommandLine.arguments.firstIndex(of: "--soak"), i + 2 < CommandLine.arguments.count {
            // 长跑自测：同一进程里连续解 N 次同一个 WAV，看内存会不会一路涨、延迟会不会漂移
            let times = max(1, Int(CommandLine.arguments[i + 1]) ?? 10)
            let path = CommandLine.arguments[i + 2]
            // 第一次解完再取基线：标点模型/词表是懒加载的，把「第一轮的一次性开销」算成本地增长会误报
            var startRSS = Int(Self.residentMB())
            var warmed = false
            var done = 0
            var failures = 0
            var stepStart = DispatchTime.now()
            var finished = false
            func finish(_ reason: String) {
                if finished { return }
                finished = true
                let endRSS = Int(Self.residentMB())
                let grew = endRSS - startRSS
                let ok = failures == 0 && grew < 120
                print("SOAK \(ok ? "OK" : "FAIL") 次数=\(done)/\(times) 失败=\(failures) RSS \(startRSS)→\(endRSS)MB（增长 \(grew)MB）\(reason)")
                core.onEvent = nil
                self.core = nil
                exit(ok ? 0 : 1)
            }
            core.onEvent = { event in
                switch event.kind {
                case "submit_request":
                    let tid = event.taskId
                    if !tid.isEmpty { _ = core.submitResult(taskId: tid, status: "undelivered", detail: "soak") }
                case "error":
                    if event.code.hasPrefix("submit_") { break }
                    failures += 1
                    print("SOAK 第 \(done + 1) 次出错：\(event.code) \(event.message)")
                case "state" where event.phase == "idle":
                    done += 1
                    if !warmed {
                        warmed = true
                        startRSS = Int(Self.residentMB())
                    }
                    let ms = Double(DispatchTime.now().uptimeNanoseconds - stepStart.uptimeNanoseconds) / 1e6
                    print(String(format: "SOAK %d/%d %.0fms RSS=%.0fMB", done, times, ms, Self.residentMB()))
                    if done >= times { finish(""); return }
                    if failures >= 3 { finish("（连续失败，提前结束）"); return }
                    stepStart = DispatchTime.now()
                    let (code, err) = core.decodeWav(path)
                    if code != 0 {
                        failures += 1
                        print("SOAK 受理失败 \(code) \(err ?? "")")
                        finish("")
                    }
                default:
                    break
                }
            }
            requireEngine("SOAK")
            let (code, err) = core.decodeWav(path)
            if code != 0 {
                print("SOAK FAIL 第一次就受理失败（\(code)）\(err ?? "")")
                exit(1)
            }
            stepStart = DispatchTime.now()
            DispatchQueue.main.asyncAfter(deadline: .now() + Double(times) * 20 + 30) {
                finish("（超时）")
            }
            return
        }
        if let i = CommandLine.arguments.firstIndex(of: "--fallback-test"),
           i + 1 < CommandLine.arguments.count {
            // 上屏失败兜底：强制 undelivered，确认结果没丢（剪贴板里有、菜单能取回、暂存还在）
            let path = CommandLine.arguments[i + 1]
            requireEngine("FALLBACK")
            setenv("ERYU_FORCE_UNDELIVERED", "辅助功能未授权（测试）", 1)
            core.setTestMode(true)
            var text = ""
            var submitted = false
            core.onEvent = { [weak self] event in
                guard let self else { return }
                switch event.kind {
                case "result":
                    text = event.string
                    print("FALLBACKRESULT \(text)")
                case "submit_request":
                    submitted = true
                    // 走真实注入路径 → 被强制 undelivered → 触发兜底（复制剪贴板 + 提示）
                    self.handle(event)
                case "notice":
                    print("FALLBACKNOTICE \(event.code) \(event.message)")
                default:
                    break
                }
                if event.kind == "state", event.phase == "idle", submitted {
                    let pb = NSPasteboard.general.string(forType: .string) ?? ""
                    let last = self.core?.takeLastResult() ?? ""
                    let clipOK = !text.isEmpty && pb.contains(text.prefix(12))
                    let menuOK = last == text
                    print("FALLBACK \(clipOK && menuOK ? "OK" : "FAIL") 剪贴板=\(clipOK) 可取回=\(menuOK) 文本=\(text.prefix(20))")
                    self.core?.onEvent = nil
                    self.core = nil
                    exit(clipOK && menuOK ? 0 : 1)
                }
            }
            _ = core.decodeWav(path)
            DispatchQueue.main.asyncAfter(deadline: .now() + 25) {
                print("FALLBACK FAIL 超时（submitted=\(submitted)）")
                exit(1)
            }
            return
        }

        if CommandLine.arguments.contains("--degrade-test") {
            runDegradeTest()
            return
        }
        if let i = CommandLine.arguments.firstIndex(of: "--privacy-test"),
           i + 1 < CommandLine.arguments.count {
            runPrivacyTest(CommandLine.arguments[i + 1])
            return
        }
        if CommandLine.arguments.contains("--settings-live-test") {
            requireEngine("SETLIVE")
            runSettingsLiveTest()
            return
        }
        if CommandLine.arguments.contains("--segment-test") {
            requireEngine("SEGTEST")
            // 验证「部分成功后只重试某段」这条 ABI 链路（真实失败段要等 ASR 出问题才有）
            let none = core.failedSegments()
            let (code, msg) = core.retrySegment(3)
            let ok = none.isEmpty && code == -3 && (msg?.contains("没有可重试") ?? false)
            print("SEGTEST \(ok ? "OK" : "FAIL") 可重试段=\(none) 不存在的段返回=\(code) 原因=\(msg ?? "-")")
            core.onEvent = nil
            self.core = nil
            exit(ok ? 0 : 1)
        }
        if CommandLine.arguments.contains("--baseline") {
            runBaseline()
            return
        }
        if cliBench {
            // 验收口径：松手→出字 p50/p95（DESIGN §5；样本来自真实录音结果）
            let json = core.statsJSON() ?? "{}"
            if let data = json.data(using: .utf8),
               let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let lat = obj["latency"] as? [String: Any] {
                let n = lat["count"] as? Int ?? 0
                if n == 0 {
                    print("BENCH 还没有样本：先按住热键说几句话（或 --mic-test 若干次）再跑")
                } else {
                    print("BENCH 样本=\(n) min=\(lat["min"] ?? "-") p50=\(lat["p50"] ?? "-") p95=\(lat["p95"] ?? "-") max=\(lat["max"] ?? "-")")
                    if let p95 = lat["p95"] as? Int {
                        print(p95 <= 1500 ? "BENCH 判定：p95 \(p95)ms ≤ 1500ms ✅" : "BENCH 判定：p95 \(p95)ms > 1500ms ❌")
                    }
                }
            } else {
                print("BENCH 读不到统计")
            }
            core.onEvent = nil
            self.core = nil
            exit(0)
        }
        if cliDiagnose {
            print(core.diagnose() ?? "（诊断不可用）")
            core.onEvent = nil
            self.core = nil
            exit(0)
        }
        if CommandLine.arguments.contains("--print-last") {
            // 等 worker 线程读完 pending/last_result.txt（崩溃前没确认上屏的那条）
            Thread.sleep(forTimeInterval: 0.3)
            let text = core.takeLastResult() ?? ""
            print(text.isEmpty ? "（没有可取回的结果）" : text)
            core.onEvent = nil
            self.core = nil
            exit(0)
        }
        if cliDumpConfig {
            print(core.configToml() ?? "")
            core.onEvent = nil
            self.core = nil
            exit(0)
        }

        let model = AppModel(core: core)
        self.model = model
        self.privacyOn = model.config.privacy_mode
        model.onHotkeyChanged = { [weak self] spec in self?.applyHotkey(spec) }

        let initialTab = CommandLine.arguments
            .first { $0.hasPrefix("--show-settings=") }
            .flatMap { SettingsTab(cliName: String($0.dropFirst("--show-settings=".count))) } ?? .general
        let settings = SettingsWindowController(model: model, initialTab: initialTab)
        self.settings = settings

        status = StatusController(
            core: core,
            hotkeyHint: model.config.hotkey,
            onSelfTest: { [weak self] in self?.runManualSelfTest() },
            onCancel: { [weak self] in self?.core?.cancel() },
            onCopyLast: { [weak self] in self?.copyLastResult() },
            onMicTest: { [weak self] in self?.runMicTest() },
            onSettings: { [weak self] in self?.settings?.present() },
            onHistory: { [weak self] in self?.settings?.present() },
            onShowMain: { [weak self] in self?.showMiniWindow() },
            onRetryFailed: { [weak self] in self?.core?.retryFailed() },
            onSelectionCommand: { [weak self] in self?.selectionLane?.begin() },
            onRelaunch: { [weak self] in self?.relaunchApp() },
            onPermReport: { [weak self] in self?.showPermReport() },
            onResetPermissions: { [weak self] in self?.resetPermissions() }
        )

        editObserver.onSuggestion = { [weak self] s in
            guard let self else { return }
            if self.privacyOn { return }
            self.model?.addSuggestion(s)
            self.status?.showNotice("学到一个改法：「\(s.from)」→「\(s.to)」，设置 → 热词里可采纳")
        }

        let mini = MiniWindowController(model: model)
        mini.onSettings = { [weak self] in self?.settings?.present() }
        mini.onToggleTalk = { [weak self] in self?.toggleMiniTalk() }
        mini.onClose = { [weak self] in self?.model?.setMiniWindow(false) }
        self.mini = mini

        if CommandLine.arguments.contains("--onboarding-selftest") {
            onboardingSelftest = true
            onboardingSelftestOK = ProbeSelftest.run()
            print(onboardingSelftestOK ? "ONBOARDING SELFTEST 判定函数 OK" : "ONBOARDING SELFTEST 判定函数 FAIL")
        }

        let lane = SelectionLane(core: core, model: model,
                                 onStatus: { [weak self] msg in self?.status?.showNotice(msg) },
                                 onError: { [weak self] msg in self?.status?.showError(code: "selection", message: msg) })
        self.selectionLane = lane

        core.onEvent = { [weak self] event in self?.handle(event) }

        maybeOfferModelDownload()

        if let i = CommandLine.arguments.firstIndex(of: "--simulate"),
           i + 1 < CommandLine.arguments.count {
            requireEngine("SIMULATE")
            runSimulate(CommandLine.arguments[i + 1])
            return
        }
        hotkeySelfTest = CommandLine.arguments.contains("--hotkey-selftest")
        if hotkeySelfTest {
            injector.dryRun = true
        }

        // 契约自测：任何平台的壳都该满足的 core 行为（不涉及麦克风/权限/注入）
        // 最近局域网请求（不记内容，只看状态码/耗时，手机连不上时排查用）
        if CommandLine.arguments.contains("--lan-log") {
            let json = core.lanLog() ?? "[]"
            print("LANLOG \(json)")
            exit(0)
        }

        // 局域网接口的地址一览（含 .local，手机在同一个 Wi-Fi 直接用，不用查 IP）
        if CommandLine.arguments.contains("--lan-info") {
            let port = model.config.lan_port
            let enabled = model.config.lan_enabled
            let token = model.config.lan_has_token
            print("LANINFO 开关=\(enabled) 端口=\(port) token=\(token ? "已设" : "没设")")
            let host = ProcessInfo.processInfo.hostName
            print("  本机：http://127.0.0.1:\(port)/")
            if !host.isEmpty {
                print("  同一 Wi-Fi（推荐）：http://\(host):\(port)/")
            }
            for ip in Self.lanIPv4Addresses() {
                print("  按 IP：http://\(ip):\(port)/")
            }
            print("  转写：POST http://<上面任一地址去掉结尾斜杠>/transcribe，body 放 16k 单声道 WAV")
            exit(0)
        }

        // 局域网听写接口：只把服务挂着，方便用 curl / 手机试（不当交互用）
        if let i = CommandLine.arguments.firstIndex(of: "--lan-serve") {
            let secs = CommandLine.arguments.count > i + 1 ? (Int(CommandLine.arguments[i + 1]) ?? 60) : 60
            print("LANSERVE 等 \(secs) 秒；端口见 config.toml 的 [lan] port 或下面的 notice")
            DispatchQueue.main.asyncAfter(deadline: .now() + Double(secs)) {
                // 请求摘要只在本进程内存里：退出前打出来，脚本才能断言
                print("LANLOG \(self.core?.lanLog() ?? "[]")")
                self.core = nil
                exit(0)
            }
            return
        }

        if CommandLine.arguments.contains("--contract") {
            runContractTest()
            return
        }

        if CommandLine.arguments.contains("--acceptance")
            || CommandLine.arguments.contains("--acceptance-synthetic") {
            runAcceptance()
            return
        }

        if let i = CommandLine.arguments.firstIndex(of: "--pull-model"),
           i + 1 < CommandLine.arguments.count {
            runPullModelTest(CommandLine.arguments[i + 1])
            return
        }

        if CommandLine.arguments.contains("--verify-test") {
            runVerifyTest()
            return
        }

        if CommandLine.arguments.contains("--type-event-test") {
            runTypeEventTest()
            return
        }

        if CommandLine.arguments.contains("--edit-observe-test") {
            runEditObserveTest()
            return
        }

        if CommandLine.arguments.contains("--selection-test") {
            runSelectionTest()
            return
        }

        if CommandLine.arguments.contains("--demo-palette") {
            let palette = CommandPalette(
                commands: SelectionLane.commands,
                preview: "我们明天下午三点在十三楼会议室过一下这版设计，重点是对齐上屏失败时的兜底方案。"
            ) { cmd in
                print("PALETTE OK 选中：\(cmd.title)")
                exit(0)
            }
            self.paletteDemo = palette
            palette.show()
            return
        }

        if CommandLine.arguments.contains("--slot-test") {
            runSlotSelfTest()
            return
        }
        if CommandLine.arguments.contains("--double-tap-test") {
            runDoubleTapTest()
            return
        }
        if CommandLine.arguments.contains("--single-tap-test") {
            runSingleTapTest()
            return
        }
        if CommandLine.arguments.contains("--demo-overlay") {
            runOverlayDemo()
            return
        }

        let hotkey = Hotkey()
        hotkey.onDown = { [weak self] in
            self?.handleHotkeyDown()
        }
        hotkey.onUp = { [weak self] in
            self?.handleHotkeyUp()
        }
        self.hotkey = hotkey

        applyHotkey(model.config.hotkey, announce: true)
        installProfileHotkeys(model)

        // 睡眠/唤醒：睡眠时把录音收尾（key-up 会丢），唤醒后重挂热键（I-038）
        let nc = NSWorkspace.shared.notificationCenter
        nc.addObserver(forName: NSWorkspace.willSleepNotification, object: nil, queue: .main) { [weak self] _ in
            self?.handleWillSleep()
        }
        nc.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { [weak self] _ in
            self?.handleDidWake()
        }
        nc.addObserver(forName: NSWorkspace.sessionDidResignActiveNotification, object: nil, queue: .main) { [weak self] _ in
            self?.handleWillSleep()
        }

    }

    /// `--single-tap-test`：只按一下（100ms）→ 400ms 后应回落到「太短」提示，不能被双击逻辑吞掉
    private func runSingleTapTest() {
        injector.dryRun = true
        var sawTooShort = false
        core?.onEvent = { [weak self] event in
            self?.handle(event)
            // 单按一下必须落到 core 的失败归因上：太短，或没声音（安静环境先报没声音）
            if event.kind == "error", event.code == "too_short" || event.code == "no_audio" {
                sawTooShort = true
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { [weak self] in
            self?.handleHotkeyDown()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.7) { [weak self] in
            self?.handleHotkeyUp()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { [weak self] in
            print("SINGLETAP \(sawTooShort ? "OK" : "FAIL") 收到太短提示=\(sawTooShort) 双击标记=\(self?.hotkeyDoubleTapped ?? false)")
            self?.core = nil
            exit(sawTooShort ? 0 : 1)
        }
    }

    /// `--double-tap-test`：先造一条「上次结果」，再模拟「短按 → 400ms 内再按下」，
    /// 期望不开始新录音、把上次结果放进剪贴板（双击取回上次结果，DESIGN §9.4）
    private func runDoubleTapTest() {
        injector.dryRun = true
        let text = "双击取回测试文本-DT1"
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { [weak self] in
            self?.core?.debugSubmit(text)
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.4) { [weak self] in
            self?.handleHotkeyDown()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { [weak self] in
            self?.handleHotkeyUp()          // 按了 100ms：进「短按待定」
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.62) { [weak self] in
            self?.handleHotkeyDown()        // 400ms 内第二下：应取回上次结果
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.1) { [weak self] in
            guard let self else { return }
            let clip = NSPasteboard.general.string(forType: .string) ?? ""
            let ok = self.hotkeyDoubleTapped && clip == text
            print("DOUBLETAP \(ok ? "OK" : "FAIL") 识别双击=\(self.hotkeyDoubleTapped) 剪贴板=\(clip)")
            self.core = nil
            exit(ok ? 0 : 1)
        }
    }

    /// 菜单栏 app 也要有「编辑」菜单：SwiftUI 文本框的 ⌘C/⌘V/⌘A/⌘Z 是靠菜单项的命令走响应链的，
    /// 没有 Edit 菜单时（本 app 只有状态栏菜单）自己在设置里按 ⌘V 什么都不会发生——
    /// 表现为「往自己的输入框里说话/粘贴都没反应」（用户 2026-11-22）。菜单本身不显示，只借它的快捷键。
    private func installEditMenu() {
        let main = NSMenu()
        let appItem = NSMenuItem()
        main.addItem(appItem)
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "隐藏 Eryu", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "退出 Eryu", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu

        let editItem = NSMenuItem()
        main.addItem(editItem)
        let edit = NSMenu(title: "编辑")
        edit.addItem(withTitle: "撤销", action: Selector(("undo:")), keyEquivalent: "z")
        let redo = edit.addItem(withTitle: "重做", action: Selector(("redo:")), keyEquivalent: "z")
        redo.keyEquivalentModifierMask = [.command, .shift]
        edit.addItem(.separator())
        edit.addItem(withTitle: "剪切", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "拷贝", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "粘贴", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "全选", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        editItem.submenu = edit
        NSApp.mainMenu = main
    }

    /// 热键按下：短按待定期间又按下 = 双击，取回上次结果而不开始新录音（DESIGN §9.4）
    private func handleHotkeyDown() {
        // 短按待定期间又按下 = 双击：这一次不录音，改为取回上次结果
        if quickTapPending {
            quickTapTimer?.cancel()
            quickTapTimer = nil
            quickTapPending = false
            hotkeyDoubleTapped = true
            core?.cancel()          // 结束刚才那个空会话，别留下 starting/listening
            copyLastResult()
            return
        }
        hotkeyDownCount += 1
        if hotkeySelfTest { return }
        quickTapDownAt = Date()
        if let app = NSWorkspace.shared.frontmostApplication?.localizedName {
            core?.noteForegroundApp(app)
        }
        speechTarget = Injector.captureTarget()
        core?.hotkeyDown()
    }

    /// 热键松开：按得极短时先挂起 400ms，等第二下（双击=取回上次结果）
    private func handleHotkeyUp() {
        hotkeyUpCount += 1
        if hotkeySelfTest { return }
        if getenv("ERYU_MIC_WAV") != nil {
            simUpAt = Date()
        }
        let held = quickTapDownAt.map { Date().timeIntervalSince($0) } ?? 1
        guard held < 0.2 else {
            core?.hotkeyUp()
            return
        }
        quickTapPending = true
        let work = DispatchWorkItem { [weak self] in
            guard let self, self.quickTapPending else { return }
            self.quickTapPending = false
            self.core?.hotkeyUp()   // 就按了一下：交给 core 按「太短」处理，用户能看到原因
        }
        quickTapTimer = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4, execute: work)
    }

    /// 润色档快捷键槽（≤3）：⌃⌥1/2/3 直接切档
    private func installProfileHotkeys(_ model: AppModel) {
        let profileHotkeys = ProfileHotkeys()
        profileHotkeys.onTrigger = { [weak self] name in
            guard let self else { return }
            self.model?.activateProfile(name)
            self.status?.showNotice("润色档已切到「\(name)」")
        }
        self.profileHotkeys = profileHotkeys
        model.onProfileSlots = { [weak self, weak profileHotkeys] profiles in
            guard let self, let profileHotkeys else { return }
            let problems = profileHotkeys.apply(profiles: profiles)
            for p in problems {
                NSLog("[eryu] %@", p)
                self.status?.showNotice(p)
            }
        }
        model.applyProfileSlots()

        // 每次启动都写一份权限真相（/tmp/eryu-perm.json），用户报「明明给了权限」时有据可查
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { [weak self] in
            let report = PermReport.write()
            log("权限报告：\(report.replacingOccurrences(of: "\n", with: " "))")
        }

        // 权限可能在你点完系统设置后才生效：每 2 秒复查一次，一旦补齐就**不用重启**地恢复功能
        startPermissionWatch()

        // 缺权限时主动引导：同一组缺项 24 小时内只问一次（每次启动都弹是骚扰，且挡住真正的问题）
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.9) { [weak self] in
            guard let self else { return }
            let missing = Permission.allCases.filter { !$0.state.ok }
            guard !missing.isEmpty, !self.hotkeySelfTest,
                  !CommandLine.arguments.contains("--no-permission-prompt") else { return }
            let key = missing.map(\.rawValue).sorted().joined(separator: ",")
            let stampKey = "prompted:" + key
            let last = UserDefaults.standard.double(forKey: stampKey)
            let now = Date().timeIntervalSince1970
            if last > 0 && now - last < 24 * 3600 {
                log("缺少权限（已提示过，24 小时内不再弹窗）：\(Permission.missingSummary() ?? "")")
                self.status?.showNotice("缺权限：\(missing.map(\.title).joined(separator: "、"))（菜单 → 权限状态）")
                return
            }
            UserDefaults.standard.set(now, forKey: stampKey)
            self.promptForPermissions(missing)
        }

        if hotkeySelfTest {
            runHotkeySelfTest(model.config.hotkey)
        }

        model.refreshAll()

        // 权限体检：指名缺哪一项，不一次索要全部（DESIGN §5 M1c）
        if let summary = Permission.missingSummary() {
            status?.showNotice(summary)
            log(summary)
        }

        if showSettings {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak self] in
                self?.settings?.present()
            }
        }

        // 首次启动：五步引导（模型 / 麦克风 / 试说一句 / 上屏方式 / 润色）。
        // 试说从未成功的老用户也会从第 3 步回来一次（R-101 / `onboarding_probe_done`）。
        let forceOnboarding = CommandLine.arguments.contains("--onboarding")
        let probePending = model.config.onboarding_done && !model.config.onboarding_probe_done
        let stepArg = Self.onboardingStepArg()
        if !showSettings,
           forceOnboarding || !model.config.onboarding_done || probePending || stepArg != nil || onboardingSelftest {
            let ob = OnboardingController(model: model)
            ob.startStep = onboardingSelftest ? 2 : (stepArg ?? (probePending ? 2 : 0))
            ob.onFinish = { [weak self] in
                // 引导结束：无条件把迷你主窗亮一次并写回 true（R-103 / D-111）
                self?.model?.setMiniWindow(true)
                self?.mini?.present()
            }
            ob.onTalkDown = { [weak self] in self?.probeTalkDown() }
            ob.onTalkUp = { [weak self] in self?.probeTalkUp() }
            onboarding = ob
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { ob.present() }
            if onboardingSelftest { startOnboardingSelftest(ob) }
        } else if model.config.mini_window {
            mini?.present()
        }
    }

    /// `--onboarding-step N`（0…4）：截图/自测直接落到某一步，不写配置
    static func onboardingStepArg() -> Int? {
        let args = CommandLine.arguments
        var raw: String?
        if let i = args.firstIndex(of: "--onboarding-step"), i + 1 < args.count {
            raw = args[i + 1]
        } else if let a = args.first(where: { $0.hasPrefix("--onboarding-step=") }) {
            raw = String(a.dropFirst("--onboarding-step=".count))
        }
        guard let raw, let n = Int(raw) else { return nil }
        return min(max(n, 0), 4)
    }

    /// 引导页里「按住说话」的按下/松开：走与热键相同的 core 链路，但不经过双击取回逻辑。
    private func probeTalkDown() {
        guard let core else { return }
        guard lastPhase == "idle" || lastPhase == "done" else { return }
        if let app = NSWorkspace.shared.frontmostApplication?.localizedName {
            core.noteForegroundApp(app)
        }
        core.hotkeyDown()
    }

    private func probeTalkUp() {
        guard lastPhase == "starting" || lastPhase == "listening" else { return }
        quickTapPending = false
        quickTapTimer?.cancel()
        quickTapTimer = nil
        core?.hotkeyUp()
    }

    /// 迷你主窗按钮：点一下开始、再点停止（只作用于这颗按钮，全局点击模式留给 R-110）
    private func toggleMiniTalk() {
        guard let core else { return }
        switch lastPhase {
        case "idle":
            if let app = NSWorkspace.shared.frontmostApplication?.localizedName {
                core.noteForegroundApp(app)
            }
            core.hotkeyDown()
        case "starting", "listening":
            quickTapPending = false
            quickTapTimer?.cancel()
            quickTapTimer = nil
            core.hotkeyUp()
        default:
            break
        }
    }

    private func showMiniWindow() {
        model?.setMiniWindow(true)
        mini?.present()
    }

    /// `--onboarding-selftest`：给了 ERYU_MIC_WAV 就跑真实闭环（判定函数已在启动早期跑过）
    private func startOnboardingSelftest(_ ob: OnboardingController) {
        let wav = ProcessInfo.processInfo.environment["ERYU_MIC_WAV"] ?? ""
        guard onboardingSelftestOK else {
            print("ONBOARDING SELFTEST FAIL 判定函数")
            exit(1)
        }
        guard !wav.isEmpty else {
            print("ONBOARDING SELFTEST 没有 ERYU_MIC_WAV，跳过真实录音（判定函数已通过）")
            exit(0)
        }
        requireEngine("ONBOARDING")
        let hold = Double(ProcessInfo.processInfo.environment["ERYU_ONBOARDING_HOLD"] ?? "") ?? 0
        ob.onProbeVerdict = { [weak self] ok, text in
            print("ONBOARDING SELFTEST \(ok ? "OK" : "FAIL") 第 3 步出字：\(text)")
            if hold > 0 {
                // 截图用：保持窗口 N 秒再退出
                print("ONBOARDING SELFTEST 保持窗口 \(Int(hold))s 供截图")
                DispatchQueue.main.asyncAfter(deadline: .now() + hold) {
                    self?.core = nil
                    exit(ok ? 0 : 1)
                }
                return
            }
            self?.core = nil
            exit(ok ? 0 : 1)
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) { [weak self] in self?.core?.hotkeyDown() }
        DispatchQueue.main.asyncAfter(deadline: .now() + 4.0) { [weak self] in self?.core?.hotkeyUp() }
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            print("ONBOARDING SELFTEST FAIL 超时（30s 内没出字）")
            exit(2)
        }
    }

    /// 每 2 秒复查权限：用户点完系统设置里的开关后，热键/上屏应当在几秒内自己恢复，
    /// 而不是「必须重启 Eryu」——重启这条以前写在文案里，是因为没有轮询（现在有了）。
    private func startPermissionWatch() {
        permTimer?.invalidate()
        lastMissingPerms = Permission.allCases.filter { !$0.state.ok }.map(\.rawValue).joined(separator: ",")
        permTimer = Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { [weak self] _ in
            self?.recheckPermissions()
        }
        if let permTimer { RunLoop.main.add(permTimer, forMode: .common) }
    }

    private func recheckPermissions() {
        let missing = Permission.allCases.filter { !$0.state.ok }
        let key = missing.map(\.rawValue).sorted().joined(separator: ",")
        guard key != lastMissingPerms else { return }
        let wasMissing = !lastMissingPerms.isEmpty
        lastMissingPerms = key
        if !key.isEmpty {
            log("权限变化：缺少 \(missing.map(\.title).joined(separator: "、"))")
            status?.showNotice("还缺权限：\(missing.map(\.title).joined(separator: "、"))（菜单 → 权限状态可看详情）")
            return
        }
        log("权限已齐全：热键与上屏已重新挂载")
        if wasMissing {
            status?.showNotice("权限已生效：现在可以按住热键说话了")
        }
        // 权限刚补齐：重新挂热键、Esc 取消、以及设置页里的权限矩阵
        applyHotkey(model?.config.hotkey ?? "ALT_R")
        updateEscCancel(phase: lastPhase)
        model?.refreshAll()
    }

    /// 菜单「权限状态…」里顺手点一下就能立刻复查（不等轮询）
    func recheckPermissionsNow() {
        lastMissingPerms = "?" // 强制走一次变化分支
        recheckPermissions()
        showPermReport()
    }

    /// 菜单「重新授权（清掉旧副本）…」：旧副本删了，系统里的授权记录还挂着，
    /// 于是新副本永远「被拒绝」。清掉后重新按一次，系统会重新问。
    func resetPermissions() {
        Permission.reauthorizeWithConfirm()
        status?.showNotice("已清掉旧授权，请在系统弹窗/设置里允许 Eryu，然后重启 Eryu")
    }

    /// 菜单「权限状态…」：把真实报告摆出来，可复制
    func showPermReport() {
        let text = PermReport.write()
        let alert = NSAlert()
        alert.messageText = "权限状态"
        alert.informativeText = text
        alert.addButton(withTitle: "复制")
        alert.addButton(withTitle: "知道了")
        if alert.runModal() == .alertFirstButtonReturn {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        hotkey?.unregister()
        hotkey = nil
        settings = nil
        status = nil
        core = nil // deinit 走停机屏障
    }

    /// `--hotkey-selftest`：合成一次「右 Option 按下 → 0.9s 后松开」，
    /// 检查热键按下/松开/提交三件事都发生（注入用 dryRun，不会真往你的窗口里打字）
    /// 润色档快捷键槽自测：注册 → 直接触发第 2 个槽 → 看当前档是否变成「邮件」→ 复原
    private func runSlotSelfTest() {
        let manager = ProfileHotkeys()
        var triggered: String?
        manager.onTrigger = { name in triggered = name }

        let profiles = model?.profiles ?? []
        let problems = manager.apply(profiles: profiles.map { (name: $0.name, slot: $0.slot) })
        let slots = manager.activeSlots
        NSLog("[eryu] 槽位注册：%@ 问题：%@", slots.joined(separator: ","), problems.joined(separator: " / "))

        guard slots.count >= 2 else {
            print("SLOTTEST FAIL 注册到的槽位不足：\(slots)")
            exit(1)
        }
        let before = (model?.profiles.first(where: { $0.active })?.name) ?? "?"
        manager.debugFire(id: 2)
        let after = triggered ?? "?"
        print("SLOTTEST \(after == slots[1] ? "OK" : "FAIL") 槽位=\(slots) 触发=\(after) 切换前=\(before)")
        manager.unregister()
        exit(after == slots[1] ? 0 : 1)
    }

    /// 端到端模拟一次真实听写：不用真麦克风、不用真按键，但走**完整链路**
    /// （热键按下 → 会话开始 → 假麦克风喂 WAV → 准流式预览 → 松手 → VAD+ASR+标点 → 上屏请求）。
    /// 素材播完自动松手；上屏走 dryRun（不会粘进你的应用）。
    private var simUpAt: Date?

    private func runSimulate(_ wavPath: String) {
        setenv("ERYU_MIC_WAV", wavPath, 1)
        injector.dryRun = true
        // 素材时长（用来决定什么时候松手）
        guard let file = try? AVAudioFile(forReading: URL(fileURLWithPath: wavPath)),
              file.fileFormat.sampleRate > 0 else {
            print("SIMFAIL 读不出这个 WAV：\(wavPath)")
            exit(1)
        }
        let seconds = Double(file.length) / file.fileFormat.sampleRate
        let code: CGKeyCode = 61  // 右 Option
        // 模拟分支早于正式的热键初始化，这里自己建一份
        if hotkey == nil {
            let h = Hotkey()
            h.onDown = { [weak self] in self?.handleHotkeyDown() }
            h.onUp = { [weak self] in self?.handleHotkeyUp() }
            hotkey = h
        }
        applyHotkey("ALT_R", announce: false)
        log("模拟听写：\(wavPath) 时长 \(String(format: "%.2f", seconds))s，0.8s 后按下，\(String(format: "%.2f", 0.8 + seconds))s 后松开")
        // ERYU_SIM_ESC=1：录音中按 Esc，验证「取消」真的不会出结果
        let escCancel = ProcessInfo.processInfo.environment["ERYU_SIM_ESC"] == "1"
        var lastPartial = ""
        var finished = false
        var resultText = ""
        var latency = 0.0
        core?.onEvent = { [weak self] event in
            guard let self else { return }
            // 交给正常事件处理：浮层/菜单/状态都按真实路径更新（上屏已 dryRun）
            self.handle(event)
            switch event.kind {
            case "state":
                if event.phase == "listening" { print("SIMSTATE listening") }
            case "partial_text":
                lastPartial = event.string
                print("SIMPARTIAL \(event.string)")
            case "result":
                resultText = event.string
                latency = event.payload["latency_ms"] as? Double ?? 0
                if let up = self.simUpAt {
                    print(String(format: "SIMWALL keyUp→result %.0fms", Date().timeIntervalSince(up) * 1000))
                }
                let waitMs = event.payload["wait_ms"] as? Double ?? 0
                let engMs = event.payload["engine_wait_ms"] as? Double ?? 0
                print(String(format: "SIMRESULT %.0fms wait=%.0f engine=%.0f %@", latency, waitMs, engMs, event.string))
            case "submit_request":
                let tid = event.taskId
                if !tid.isEmpty { _ = self.core?.submitResult(taskId: tid, status: "undelivered", detail: "simulate") }
            case "error":
                if event.code.hasPrefix("submit_") { break }
                print("SIMERROR \(event.code): \(event.message)")
                // 拔麦测试：这条错误是预期的，收尾仍要继续，才能验证「已录内容被保住」
                if ProcessInfo.processInfo.environment["ERYU_SIM_TOLERATE_ERRORS"] != nil,
                   event.code == "mic_unplugged" { break }
                self.exitSim(finished: &finished, ok: false)
            case "notice":
                print("SIMNOTICE \(event.code) \(event.message)")
            default:
                break
            }
            if event.kind == "state", event.phase == "idle", !resultText.isEmpty {
                let ok = !resultText.isEmpty
                print("SIMULATE \(ok ? "OK" : "FAIL") 松手→出字 \(Int(latency))ms 文本=\(resultText) 预览=\(lastPartial)")
                self.exitSim(finished: &finished, ok: ok)
            }
        }
        let noEvents = getenv("ERYU_SIM_NOEVENTS") != nil
        if noEvents {
            // 只喂音频、不合成按键：避免「喂完自动 up」与「真按键 up」撞车，用来量纯链路延迟
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
                self.core?.hotkeyDown()
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
            if noEvents { return }
            Self.postModifier(code: code, down: true)
            if escCancel {
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) {
                    Self.postKey(code: 53)   // kVK_Escape
                    print("SIMESC 已按 Esc")
                }
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8 + seconds) {
            if noEvents { return }
            Self.postModifier(code: code, down: false)
        }
        if escCancel {
            // Esc 之后应当回到 idle 且不出结果
            DispatchQueue.main.asyncAfter(deadline: .now() + 4.0) {
                guard !finished else { return }
                let cancelledCleanly = resultText.isEmpty && self.lastPhase == "idle"
                print("ESCTEST \(cancelledCleanly ? "OK" : "FAIL") 阶段=\(self.lastPhase) 有结果=\(!resultText.isEmpty)")
                self.exitSim(finished: &finished, ok: cancelledCleanly)
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8 + seconds + 20) {
            guard !finished else { return }
            print("SIMULATE FAIL 超时（没有结果）")
            self.core = nil
            exit(1)
        }
    }

    private func exitSim(finished: inout Bool, ok: Bool) {
        if finished { return }
        finished = true
        core?.onEvent = nil
        self.core = nil
        exit(ok ? 0 : 1)
    }

    /// `--contract`：ABI 版本 + 事件 envelope + 一次性 submit 往返（对应 docs/WINDOWS.md §10）
    private func runContractTest() {
        guard let core else { print("CONTRACT FAIL 没有 core"); exit(1) }
        core.setTestMode(true)
        injector.dryRun = true
        var events = 0
        var sawState = false
        var sawResult = false
        var submitOk = false
        var badField = ""
        let deadline = Date().addingTimeInterval(8)
        core.onEvent = { [weak self] event in
            events += 1
            if event.kind.isEmpty || event.seq == 0 { badField = "kind/seq 为空" }
            if event.kind == "state", event.phase == "inserting" || event.phase == "idle" { sawState = true }
            if event.kind == "result", !event.string.isEmpty { sawResult = true }
            if event.kind == "submit_request" {
                if event.taskId.isEmpty { badField = "submit_request 缺 task_id" }
                if event.mode.isEmpty { badField = "submit_request 缺 mode" }
                submitOk = true
                _ = self?.core?.submitResult(taskId: event.taskId, status: "undelivered", detail: "contract")
            }
            if sawResult, submitOk, events > 3 { return }
        }
        core.debugSubmit("契约测试：这条不该出现在任何输入框里")
        // 轮询等待（简单可靠，不依赖 runloop 事件）
        while Date() < deadline {
            if sawResult, submitOk { break }
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
        let pass = sawResult && sawState && submitOk && badField.isEmpty
        print("CONTRACT \(pass ? "OK" : "FAIL") ABI=\(Core.abi) 事件=\(events) 状态=\(sawState) 结果=\(sawResult) 提交=\(submitOk) 问题=\(badField.isEmpty ? "无" : badField)")
        core.onEvent = nil
        self.core = nil
        exit(pass ? 0 : 1)
    }

    /// `--degrade-test`：权限不足时不许崩、不许假装成功，并且要有可用的退路（组合键）
    private func runDegradeTest() {
        var problems: [String] = []
        // 这个分支早于正式热键创建点：自己建一个
        if hotkey == nil {
            let hk = Hotkey()
            hk.onDown = { [weak self] in self?.hotkeyDownCount += 1; self?.core?.hotkeyDown() }
            hk.onUp = { [weak self] in self?.hotkeyUpCount += 1; self?.core?.hotkeyUp() }
            hotkey = hk
        }
        setenv("ERYU_FORCE_TAP_FAIL", "1", 1)
        // 1) 单键热键失败 → 返回中文原因，应用继续活着
        applyHotkey("ALT_R", announce: false)
        if case .singleKey = HotkeySpec.parseBinding("ALT_R")! {
            // 如果这里还是 singleKey 模式，说明注册没成功（被强制失败），符合预期
        }
        let singleReason = hotkeyRegistrationReason
        if singleReason == nil { problems.append("单键失败没给出原因") }
        // 2) 退路：组合键仍能注册
        applyHotkey("CTRL+ALT+SPACE", announce: false)
        if hotkeyRegistrationReason != nil { problems.append("组合键也被拒：\(hotkeyRegistrationReason!)") }
        // 3) 权限缺失的说明必须点名缺哪项
        let missing = Permission.missingSummary() ?? ""
        let permOK = !missing.isEmpty ? missing.contains("缺少权限") : true
        if !permOK { problems.append("权限提示不合规：\(missing)") }
        let ok = problems.isEmpty
        print("DEGRADETEST \(ok ? "OK" : "FAIL") 单键原因=\(singleReason ?? "（空）") 组合键=可用 权限提示=\(missing.isEmpty ? "（三项齐全）" : missing) 问题=\(problems.isEmpty ? "无" : problems.joined(separator: "；"))")
        self.core?.onEvent = nil
        self.core = nil
        exit(ok ? 0 : 1)
    }

    /// `--privacy-test <wav>`：隐私模式下，正文不许出现在菜单「最近」里（磁盘部分由 scripts/privacy_check.sh 查）
    private func runPrivacyTest(_ path: String) {
        requireEngine("PRIVACY")
        guard let core else { return }
        core.setTestMode(true)
        var text = ""
        core.onEvent = { [weak self] event in
            guard let self else { return }
            switch event.kind {
            case "result":
                text = event.string
            case "submit_request":
                if !event.taskId.isEmpty {
                    _ = self.core?.submitResult(taskId: event.taskId, status: "undelivered", detail: "privacy-test")
                }
            default:
                break
            }
            if event.kind == "state", event.phase == "idle", !text.isEmpty {
                let recent = self.status?.recentText ?? ""
                let leaked = !recent.isEmpty && recent.contains(text.prefix(8))
                print("PRIVACYTEST \(leaked ? "FAIL" : "OK") 最近菜单=\(recent.isEmpty ? "（空）" : recent) 文本=\(text.prefix(16))")
                self.core?.onEvent = nil
                self.core = nil
                exit(leaked ? 1 : 0)
            }
        }
        _ = core.decodeWav(path)
        DispatchQueue.main.asyncAfter(deadline: .now() + 25) {
            print("PRIVACYTEST FAIL 超时")
            exit(1)
        }
    }

    /// `--settings-live-test`：改热键/改引擎之后，**立刻**生效吗？（不是等下次重启）
    private func runSettingsLiveTest() {
        let tmp = NSTemporaryDirectory() + "eryu-setlive-\(getpid())"
        try? FileManager.default.createDirectory(atPath: tmp, withIntermediateDirectories: true)
        var problems: [String] = []
        // 这个分支早于正式的热键创建点：必须自己建一个，否则 applyHotkey 会静默返回
        if hotkey == nil {
            let hk = Hotkey()
            hk.onDown = { [weak self] in
                self?.hotkeyDownCount += 1
                self?.core?.hotkeyDown()
            }
            hk.onUp = { [weak self] in
                self?.hotkeyUpCount += 1
                self?.core?.hotkeyUp()
            }
            hotkey = hk
        }

        // 1) 热键：先 F13，再换 ALT_R，验证旧的不再触发、新的立刻触发
        let f13 = HotkeySpec.parseBinding("F13").flatMap { b -> CGKeyCode? in
            if case let .singleKey(c, _) = b { return c }; return nil
        } ?? 105
        let altR: CGKeyCode = 61
        applyHotkey("F13", announce: false)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { Self.postKey(code: f13) }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.9) {
            let afterF13 = self.hotkeyDownCount
            self.applyHotkey("ALT_R", announce: false)
            // 换完之后再按 F13：不该再计数
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { Self.postKey(code: f13) }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.9) {
                let afterOld = self.hotkeyDownCount
                Self.postModifier(code: altR, down: true)
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
                    Self.postModifier(code: altR, down: false)
                }
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.9) {
                    let afterNew = self.hotkeyDownCount
                    if afterF13 == 0 { problems.append("F13 没触发") }
                    if afterOld != afterF13 { problems.append("换键后旧键仍在触发") }
                    if afterNew <= afterOld { problems.append("换键后新键没触发") }
                    self.checkEngineLiveSwitch(tmp: tmp, problems: problems)
                }
            }
        }
    }

    private func checkEngineLiveSwitch(tmp: String, problems initial: [String]) {
        guard let core else { return }
        var problems = initial
        var switched = false
        core.onEvent = { event in
            // core 把 engine_ready 走 emit_error（kind=error），不是 notice
            if event.code == "engine_ready" { switched = true }
        }
        let code = core.setEngine(id: "qwen3")
        if code != 0 { problems.append("切 qwen3 被拒：返回 \(code)") }
        // 6s 在负载高时不够（qwen3 加载 1.3s → 忙时 8–10s），放宽到 20s（I-078 附带发现）
        DispatchQueue.main.asyncAfter(deadline: .now() + 20.0) {
            let diag = core.diagnose() ?? ""
            let line = diag.split(separator: "\n").first { $0.hasPrefix("引擎：") }.map(String.init) ?? ""
            if !line.contains("qwen3") { problems.append("切了引擎但诊断仍是：\(line)") }
            if !switched { problems.append("没收到 engine_ready（可能模型没装）") }
            _ = core.setEngine(id: "sense")
            let ok = problems.isEmpty
            print("SETLIVE \(ok ? "OK" : "FAIL") 热键切换=通过 引擎=\(line) 问题=\(problems.isEmpty ? "无" : problems.joined(separator: "；"))")
            core.onEvent = nil
            self.core = nil
            exit(ok ? 0 : 1)
        }
    }

    private func runHotkeySelfTest(_ spec: String) {
        let code: CGKeyCode = HotkeySpec.parseBinding(spec).flatMap { binding -> CGKeyCode? in
            if case let .singleKey(c, _) = binding { return c }
            return nil
        } ?? 61
        // 不管配置里写的是什么，自测一律走「单键右 Option」
        applyHotkey("ALT_R", announce: false)
        log("热键自测：注册 ALT_R（右 Option），keyCode=\(code)，0.8s 后合成按下，1.7s 后合成松开")

        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
            Self.postModifier(code: code, down: true)
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.7) {
            Self.postModifier(code: code, down: false)
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 5.0) { [weak self] in
            guard let self else { return }
            // 判定只看热键本身（按下/松开都收到了）；提交取决于这段时间有没有说话，仅记录
            let ok = self.hotkeyDownCount > 0 && self.hotkeyUpCount > 0
            let line = "HOTKEYTEST \(ok ? "OK" : "FAIL") 按下=\(self.hotkeyDownCount) 松开=\(self.hotkeyUpCount) 提交=\(self.hotkeySubmitCount)"
            print(line)
            self.hotkey?.unregister()
            exit(ok ? 0 : 1)
        }
    }

    /// 用 CGEvent 合成一次修饰键的按下/松开（本进程已有辅助功能权限）
    /// 合成一个普通按键（Esc 取消自测用）
    private static func postKey(code: CGKeyCode) {
        guard let down = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: true),
              let up = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: false) else { return }
        down.post(tap: .cghidEventTap)
        DispatchQueue.global().asyncAfter(deadline: .now() + 0.05) {
            up.post(tap: .cghidEventTap)
        }
    }

    private static func postModifier(code: CGKeyCode, down: Bool) {
        guard let ev = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down) else { return }
        ev.type = CGEventType.flagsChanged
        ev.flags = down ? CGEventFlags.maskAlternate : CGEventFlags(rawValue: 0)
        ev.post(tap: .cghidEventTap)
    }

    /// `--demo-overlay`：把浮层按「录音中 → 上屏中 → 收起」跑一遍（截图核对用）
    private func runOverlayDemo() {
        let steps: [(TimeInterval, () -> Void)] = [
            (0.3, { [weak self] in self?.overlay?.show(phase: "starting") }),
            (0.9, { [weak self] in self?.overlay?.show(phase: "listening") }),
            (1.2, { [weak self] in self?.overlay?.setLevel(0.06) }),
            (3.0, { [weak self] in self?.overlay?.setPartial("我们把明天的评审") }),
            (3.4, { [weak self] in self?.overlay?.setLevel(0.11) }),
            (5.0, { [weak self] in self?.overlay?.setPartial("我们把明天的评审挪到下午三点，会议室还没定，另外把上次那份预算表也一起发到群里，让大家先看一眼") }),
            (7.0, { [weak self] in self?.overlay?.setFinal("我们把明天的评审挪到下午三点，会议室还没定。") }),
            (7.4, { [weak self] in self?.overlay?.show(phase: "inserting") }),
            (8.4, { [weak self] in self?.overlay?.show(phase: "idle") }),
        ]
        for (delay, action) in steps {
            DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: action)
        }
        log("浮层演示：1.3s 起显示流式文本，3.5s 上屏中，5s 收起")
        // 演示完就退出：否则会留下一个「没有热键」的进程，用户按热键时会以为坏了
        DispatchQueue.main.asyncAfter(deadline: .now() + 10.0) { exit(0) }
    }

    // MARK: - 热键

    /// 系统睡眠/切用户时把正在进行的录音收尾：key-up 事件会丢，不能让它卡到 120s 上限
    private func handleWillSleep() {
        if lastPhase == "starting" || lastPhase == "listening" {
            log("系统要睡眠了：取消本次录音并保住已录内容")
            status?.showNotice("系统睡眠，已取消本次录音（已保留录到的部分）")
            core?.cancel()
        }
    }

    /// 唤醒后重新注册热键：Carbon 热键在睡眠/显示器休眠后可能失灵（I-038）
    private func handleDidWake() {
        if let spec = model?.config.hotkey {
            log("唤醒：重新注册热键 \(spec)")
            applyHotkey(spec)
        }
        model?.refreshDevices()
        model?.refreshAll()
    }

    private func applyHotkey(_ spec: String, announce: Bool = false) {
        guard let hotkey else { return }
        let (ok, reason) = hotkey.apply(spec: spec)
        hotkeyRegistrationReason = ok ? nil : (reason ?? "热键注册失败")
        status?.setHotkeyHint(spec)
        if ok {
            if announce { status?.showNotice("\(spec) 按住说话") }
        } else {
            let why = reason ?? "热键注册失败"
            status?.showError(code: "hotkey_register_failed", message: why)
            status?.showNotice("快捷键没生效：\(why)")
            log("热键注册失败（\(spec)）：\(why)")
        }
    }

    /// 录音开始暂停、结束恢复（只在设置开启时动作）
    private func syncMediaPause(phase: String) {
        guard model?.config.pause_media == true else {
            if mediaPause.isPaused { mediaPause.resumeIfPaused() }
            return
        }
        switch phase {
        case "starting", "listening":
            mediaPause.pauseIfPlaying()
        case "inserting", "idle":
            mediaPause.resumeIfPaused()
        default:
            break
        }
    }

    /// 自测：走完整 ABI 下载一个模型，把进度事件打到 stdout
    private func runPullModelTest(_ id: String) {
        guard let core else { print("PULLTEST FAIL core 未就绪"); exit(1) }
        let (rc, message) = core.pullModel(id)
        if rc != 0 {
            print("PULLTEST FAIL rc=\(rc) \(message ?? "")")
            exit(rc == -5 ? 0 : 1)
        }
        let deadline = Date().addingTimeInterval(1800)
        while Date() < deadline {
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
            if let done = pullDone { print(done); exit(done.hasPrefix("PULLTEST OK") ? 0 : 1) }
        }
        print("PULLTEST FAIL 超时")
        exit(1)
    }

    /// 自测：注入后校验的判定（自建输入框，不真的粘贴）
    private func runVerifyTest() {
        let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 480, height: 220),
                           styleMask: [.titled], backing: .buffered, defer: false)
        win.title = "字在 校验自测"
        let tv = NSTextView(frame: NSRect(x: 0, y: 0, width: 480, height: 220))
        tv.string = "这是一段已经存在的文字，含 DeepSeek 字样。"
        win.contentView = tv
        win.center()
        win.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        win.makeFirstResponder(tv)

        DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) {
            let el = AX.focusedElement()
            let hit = Injector.verify(text: "DeepSeek 字样", element: el)
            let miss = Injector.verify(text: "完全不存在的一段文字", element: el)
            var ok = false
            if case .ok = hit { ok = true }
            var missUnknown = false
            if case .unknown = miss { missUnknown = true }
            let pass = ok && missUnknown
            print(pass ? "VERIFYTEST OK 命中=ok 未命中=unknown" : "VERIFYTEST FAIL 命中=\(hit) 未命中=\(miss)")
            exit(pass ? 0 : 1)
        }
    }

    /// 自测：keystroke 的键盘事件构造（不真的投递，避免打到别的应用）
    private func runTypeEventTest() {
        let sample = "你好 Eryu 42 🚀"
        guard let src = CGEventSource(stateID: .combinedSessionState),
              let down = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: true) else {
            print("TYPETEST FAIL 无法创建事件")
            exit(1)
        }
        let units = Array(sample.utf16)
        var length = 0
        units.withUnsafeBufferPointer { buf in
            down.keyboardSetUnicodeString(stringLength: units.count, unicodeString: buf.baseAddress)
            length = units.count
        }
        // 读回：验证事件里确实带上了这段 UTF-16
        var out = [UniChar](repeating: 0, count: 64)
        var outLen = 0
        out.withUnsafeMutableBufferPointer { buf in
            down.keyboardGetUnicodeString(maxStringLength: 64, actualStringLength: &outLen, unicodeString: buf.baseAddress)
        }
        let back = String(utf16CodeUnits: out, count: outLen)
        let ok = outLen == length && back == sample
        print(ok ? "TYPETEST OK 写入 \(length) 单元 回读=\(back)" : "TYPETEST FAIL 回读=\(back) 期望=\(sample)")
        exit(ok ? 0 : 1)
    }

    // MARK: - 事件

    private func handle(_ event: CoreEvent) {
        if acceptance != nil { recordAcceptance(event) }
        switch event.kind {
        case "state":
            lastPhase = event.phase
            status?.show(phase: event.phase)
            mini?.updatePhase(event.phase)
            onboarding?.notePhase(event.phase)
            // 引导第 3 步时，浮层只会挡住引导页，不显示（DESIGN §12.1）
            if onboarding?.probeActive != true { overlay?.show(phase: event.phase) }
            updateEscCancel(phase: event.phase)
            syncMediaPause(phase: event.phase)
            if selfTest { selfTestSeen.append("state:" + event.phase); checkSelfTest(event) }
        case "result":
            if micTestSecs != nil { micTestText = event.string } else { micTestDone = true }
            let n = event.string.count
            let inProbe = onboarding?.probeActive == true
            if !inProbe {
                if privacyOn {
                    overlay?.setFinalRedacted(n)
                } else {
                    overlay?.setFinal(event.string)
                }
                // 隐私模式：连菜单里的「最近」预览也不显示正文（D-023）
                status?.setRecent(privacyOn ? "" : event.string)
                status?.showNotice("已识别 \(n) 字，正在上屏…")
            }
            model?.refreshHistory()
            model?.refreshStats()
            let latency = (event.payload["latency_ms"] as? Double) ?? 0
            let decode = (event.payload["decode_ms"] as? Double) ?? 0
            if micTestSecs != nil {
                print(String(format: "RESULT 松手→出字 %.0fms（其中识别 %.0fms）", latency, decode))
            }
            status?.setLatency(latency)
        case "notice":
            // 不是错误，但用户该知道（润色失败/到时长上限…）
            log("提示 \(event.code): \(event.message)")
            if !event.message.isEmpty {
                status?.showNotice(event.message)
                if event.code == "record_limit" { overlay?.showError(event.message) }
            }
        case "error":
            let inProbe = onboarding?.probeActive == true
            if inProbe {
                // 引导第 3 步：原因显示在引导页里（没听到声音 / 太短 / 识别失败）
                onboarding?.noteFailure(code: event.code, message: event.message)
            } else if event.code.hasPrefix("partial_result") {
                overlay?.showError("有段落没识别成功，已上屏其余部分 · ⌘R 重试")
            } else if event.code != "polish_failed" {
                overlay?.showError(event.message.isEmpty ? event.code : event.message)
            }
            if !inProbe {
                status?.showError(code: event.code, message: event.message.isEmpty ? event.code : event.message)
            }
            log("错误 \(event.code): \(event.message)")
            if selfTest { selfTestSeen.append("error:" + event.code); failSelfTest("收到错误事件 \(event.code)") }
            if micTestSecs != nil {
                if event.code.hasPrefix("partial_result"), ProcessInfo.processInfo.environment["ERYU_FAULT_FAIL_SEGMENTS"] != nil {
                    // 故障注入模式：部分失败是预期，继续等单段重试结果
                    print("PARTIALSEG \(event.message)")
                } else {
                    print("MICTEST FAIL \(event.code): \(event.message)"); exit(2)
                }
            }
            if event.code.hasPrefix("submit_") && !inProbe {
                status?.showNotice("没能上屏：\(event.message)。文字已保存，菜单「复制上次结果」可取回")
            }
            model?.refreshHistory()
        case "submit_request":
            // 崩溃恢复的端到端测试钩子：在「结果已落盘、还没上屏确认」的瞬间硬杀进程
            // （core 的 on_decoded 顺序是 result → save_pending_result → submit_request，
            //   所以必须等到这里杀，才能保证 pending/last_result.txt 已经写下去）
            if ProcessInfo.processInfo.environment["ERYU_KILL_ON_SUBMIT"] != nil {
                fflush(stdout)
                kill(getpid(), SIGKILL)
            }
            hotkeySubmitCount += 1
            let taskId = event.taskId
            let text = event.string
            log("submit_request task=\(taskId) mode=\(event.mode) len=\(text.count)")
            // 引导第 3 步：只显示、不注入（D-110）。否则测试句会粘进用户背后的应用。
            if let ob = onboarding, ob.offerProbeResult(text) {
                core?.submitResult(taskId: taskId, status: "ok")
                model?.refreshHistory()
                model?.refreshStats()
                return
            }
            let target = speechTarget
            speechTarget = nil
            injector.inject(text: text, mode: event.mode, target: target) { [weak self] result in
                guard let self else { return }
                switch result {
                case .ok:
                    self.core?.submitResult(taskId: taskId, status: "ok")
                    self.status?.showNotice("已上屏（\(text.count) 字）")
                    // 上屏后盯一会儿输入框：用户改词 → 记一条热词候选（隐私模式不盯）
                    if !self.privacyOn {
                        self.editObserver.watch(expected: text, element: AX.focusedElement())
                    }
                case .undelivered(let why):
                    self.core?.submitResult(taskId: taskId, status: "undelivered", detail: why)
                    // 结果不能丢：放进剪贴板 + 菜单「复制上次结果」仍可取回（R-063）
                    let pb = NSPasteboard.general
                    pb.clearContents()
                    pb.setString(text, forType: .string)
                    self.status?.showNotice("没上屏（\(why)）：已复制到剪贴板，也可从菜单取回")
                    print("[eryu] undelivered 已复制到剪贴板 why=\(why)")
                case .unknown(let why):
                    self.core?.submitResult(taskId: taskId, status: "unknown", detail: why)
                    let pb = NSPasteboard.general
                    pb.clearContents()
                    pb.setString(text, forType: .string)
                    self.status?.showNotice("粘贴没确认成功：已复制到剪贴板，也可从菜单取回")
                    print("[eryu] unknown 已复制到剪贴板 why=\(why)")
                }
            }
            model?.refreshHistory()
        case "pending_result":
            let n = event.payload["chars"] as? Int ?? event.string.count
            status?.showNotice("上次有 \(n) 字没确认上屏，已放进「复制上次结果」")
            model?.showToast("上次有 \(n) 字没上屏，菜单可取回")
        case "meter":
            overlay?.setLevel(Float(event.meterRms))
            if event.payload["live"] as? Bool == true {
                overlay?.setSecs((event.payload["secs"] as? Double) ?? 0)
            }
            model?.handleMeterEvent(event)
        case "failed_segments":
            let indexes = (event.payload["indexes"] as? [Int]) ?? []
            if micTestSecs != nil {
                print("FAILEDSEG \(indexes)")
                // 故障注入自测：有失败段就立刻重试第一段，验证「部分成功 → 单段重跑 → 结果落剪贴板」整条链路
                if !indexes.isEmpty, let core = self.core {
                    let (code, err) = core.retrySegment(indexes[0])
                    print("SEGRETRY 第 \(indexes[0]) 段 返回=\(code) \(err ?? "")")
                }
            }
            status?.setFailedSegments(indexes) { [weak self] index in
                guard let self, let core = self.core else { return }
                let (code, err) = core.retrySegment(index)
                if code != 0 {
                    self.status?.showNotice(err ?? "这段没法重试（错误码 \(code)）")
                }
            }
        case "segment_retry_started":
            let idx = (event.payload["index"] as? Int) ?? 0
            status?.showNotice("正在重跑第 \(idx) 段…")
        case "segment_result":
            if micTestSecs != nil {
                let idx = (event.payload["index"] as? Int) ?? 0
                let txt = (event.payload["text"] as? String) ?? ""
                print("SEGRESULT 第 \(idx) 段=\(txt)")
                if ProcessInfo.processInfo.environment["ERYU_FAULT_FAIL_SEGMENTS"] != nil {
                    let ok = !txt.isEmpty
                    print(ok ? "SEGTEST OK 单段重试用例通过" : "SEGTEST FAIL 重试后仍为空")
                    self.core = nil
                    exit(ok ? 0 : 1)
                }
            }
            // 单段结果不自动上屏（每轮只提交一次注入，终审决定①）：复制到剪贴板由用户决定粘贴位置
            let idx = (event.payload["index"] as? Int) ?? 0
            let chars = (event.payload["chars"] as? Int) ?? 0
            if let t = event.payload["text"] as? String, !t.isEmpty {
                let pb = NSPasteboard.general
                pb.clearContents()
                pb.setString(t, forType: .string)
                status?.showNotice("第 \(idx) 段重新识别好了（\(chars) 字），已复制到剪贴板")
            }
        case "partial_text":
            if onboarding?.probeActive == true {
                onboarding?.notePartial(event.string)
                break
            }
            if privacyOn {
                overlay?.setPartialRedacted()
            } else {
                overlay?.setPartial(event.string)
            }
            if let secs = micTestSecs {
                _ = secs
                let ms = (event.payload["ms"] as? Double) ?? 0
                print("PARTIAL \(event.string)  [解码 \(Int(ms))ms]")
            }
        case "stats":
            model?.refreshStats()
        case "history_changed":
            model?.refreshHistory()
        case "config_changed":
            model?.refreshConfig()
            if let m = model { privacyOn = m.config.privacy_mode }
            let n = event.toml.count
            if n > 0 { log("配置已更新（\(n) 字节）") }
        case "model_progress":
            let id = (event.payload["id"] as? String) ?? ""
            let received = (event.payload["received"] as? Double) ?? 0
            let total = (event.payload["total"] as? Double) ?? 0
            let phase = (event.payload["phase"] as? String) ?? "downloading"
            model?.updatePull(id: id, received: received, total: total, phase: phase)
            if total > 0, Int(received / 1_048_576) % 64 == 0 {
                print(String(format: "MODELPROGRESS %@ %.0f/%.0f MB", id, received / 1_048_576, total / 1_048_576))
            }

        case "model_done":
            let id = (event.payload["id"] as? String) ?? ""
            model?.finishPull(id: id, ok: true, message: nil)
            pullDone = "PULLTEST OK \(id) \((event.payload["path"] as? String) ?? "")"
            print("MODELDONE \(id)")

        case "model_failed":
            let id = (event.payload["id"] as? String) ?? ""
            let msg = (event.payload["message"] as? String) ?? "下载失败"
            model?.finishPull(id: id, ok: false, message: msg)
            status?.showError(code: "model_failed", message: msg)
            pullDone = "PULLTEST FAIL \(id) \(msg)" 

        case "config_save_failed":
            status?.showError(code: event.code, message: "配置没保存：\(event.message)")
            model?.showToast("配置没保存：\(event.message)")
        default:
            break
        }
    }

    // MARK: - 菜单动作

    /// `--baseline`：冷启动到引擎就绪的耗时 + 常驻内存（用户关心「太挑机器」）
    /// 授权后辅助功能/输入监控可能不刷新：重启自己（保留同一条安装路径，TCC 身份不变）
    private func relaunchApp() {
        let url = Bundle.main.bundleURL
        let cfg = NSWorkspace.OpenConfiguration()
        cfg.createsNewApplicationInstance = true
        NSWorkspace.shared.openApplication(at: url, configuration: cfg) { _, err in
            if let err {
                NSLog("[eryu] 重启失败：\(err)")
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
            self.core = nil
            exit(0)
        }
    }

    private func runBaseline() {
        // 调用点已经在 Core() 之后：引擎是同步加载的，这时量到的就是「冷启动到能听写」+ 常驻内存
        let boot = Date().timeIntervalSince(eryuProcessStart) * 1000
        let rss = Self.residentMB()
        let engineName = ProcessInfo.processInfo.environment["ERYU_ENGINE"] ?? "sense"
        print(String(format: "BASELINE 冷启动=%.0fms RSS=%.0fMB 模型=%@ 屏幕=%.0fx%.0f", boot, rss, engineName,
                     NSScreen.main?.frame.width ?? 0, NSScreen.main?.frame.height ?? 0))
        core = nil
        exit(0)
    }

    /// 当前进程常驻内存（MB，含共享页；用 task_info 而不是 ps，避免被 fork 副本干扰）
    /// 枚举本机局域网 IPv4（给 `--lan-info` 用；不含 127.0.0.1）
    static func lanIPv4Addresses() -> [String] {
        var out: [String] = []
        var addrs: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&addrs) == 0, let first = addrs else { return out }
        defer { freeifaddrs(addrs) }
        var cur: UnsafeMutablePointer<ifaddrs>? = first
        while let p = cur {
            let ifa = p.pointee
            if let sa = ifa.ifa_addr, sa.pointee.sa_family == UInt8(AF_INET) {
                let flags = Int32(ifa.ifa_flags)
                let up = (flags & IFF_UP) != 0
                let loopback = (flags & IFF_LOOPBACK) != 0
                var buf = [CChar](repeating: 0, count: Int(NI_MAXHOST))
                if up && !loopback,
                   getnameinfo(sa, socklen_t(sa.pointee.sa_len), &buf, socklen_t(buf.count), nil, 0, NI_NUMERICHOST) == 0 {
                    let ip = String(cString: buf)
                    if !ip.isEmpty, !out.contains(ip) { out.append(ip) }
                }
            }
            cur = ifa.ifa_next
        }
        return out
    }

    /// CLI 自测要不要被 stub 拦住：stub 会造出「测试文本：你好，Eryu。」，让识别类自测假绿。
    /// --self-test / --contract 故意允许 stub（它们测的是事件流，不是识别质量）。
    private func requireEngine(_ what: String) {
        guard ProcessInfo.processInfo.environment["ERYU_ALLOW_STUB"] == nil else { return }
        let diag = core?.diagnose() ?? ""
        let engineLine = diag.split(separator: "\n").first { $0.hasPrefix("引擎：") }.map(String.init) ?? ""
        let active = engineLine.split(separator: "（").last.map(String.init) ?? engineLine
        let ready = active.split(separator: " / ").first.map(String.init) ?? active
        guard ready.contains("就绪=false") else { return }
        print("\(what) FAIL 引擎没装好，stub 会造出假文本，所以不跑：\(engineLine)")
        print("  修：设置 → 引擎 里下载模型，或 --pull-model sense")
        print("  只想过流程：ERYU_ALLOW_STUB=1 显式放行")
        exit(1)
    }

    /// 认识的 `--flag`（带值的参数写成前缀，如 `--show-settings=audio` 也算认识）
    static let knownFlags: [String] = [
        "--self-test", "--mic-test", "--diagnose", "--dump-config", "--print-last",
        "--show-settings", "--demo-overlay", "--overlay-solid", "--hotkey-selftest",
        "--perm-check", "--ask-permissions", "--no-permission-prompt", "--inject-paste", "--focus-lock-test",
        "--hotword-type-test", "--self-paste-test",
        "--inject-test", "--type-event-test", "--verify-test", "--edit-observe-test",
        "--decode-wav", "--soak", "--segment-test", "--voice-state", "--voice-enroll-wav",
        "--voice-clear", "--pull-model", "--bench", "--sleep-test", "--baseline",
        "--contract", "--acceptance", "--acceptance-synthetic", "--double-tap-test",
        "--single-tap-test", "--simulate", "--lan-serve", "--lan-info", "--lan-log",
        "--fallback-test", "--privacy-test", "--degrade-test", "--perm-report",
        "--settings-live-test", "--no-permission-prompt", "--onboarding",
        "--onboarding-step", "--onboarding-selftest",
        "--version", "--help",
    ]

    static func unknownFlag(_ args: [String]) -> String? {
        args.dropFirst().first { arg in
            arg.hasPrefix("--") && !knownFlags.contains { arg == $0 || arg.hasPrefix($0 + "=") }
        }
    }

    private static func residentMB() -> Double {
        var info = mach_task_basic_info()
        var count = mach_msg_type_number_t(MemoryLayout<mach_task_basic_info>.size / MemoryLayout<natural_t>.size)
        let kr = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(MACH_TASK_BASIC_INFO), $0, &count)
            }
        }
        guard kr == KERN_SUCCESS else { return 0 }
        return Double(info.resident_size) / 1024.0 / 1024.0
    }

    private func exitWavTest(_ code: Int32) {
        self.core = nil
        exit(code)
    }

    private func runManualSelfTest() {
        log("手动自测：走一遍 识别→上屏")
        core?.debugSubmit("这是一段 Eryu 自测文本：你好，世界。")
    }

    private func startMicTest(_ core: Core, seconds: Int32) {
        core.onEvent = { [weak self] event in
            guard let self else { return }
            if event.kind == "result" { self.micTestText = event.string }
            self.handle(event)
            if event.kind == "state", event.phase == "idle", !self.micTestText.isEmpty {
                print("MICTEST OK 录音 \(seconds)s | 结果=\(self.micTestText)")
                self.core = nil
                exit(0)
            }
        }
        log("mic-test 开始：录 \(seconds) 秒")
        DispatchQueue.main.asyncAfter(deadline: .now() + Double(seconds) + 20) { [weak self] in
            print("MICTEST FAIL 超时（监听阶段无结果）。若首次运行，请在系统设置里给 Eryu 麦克风权限后重试")
            self?.core = nil
            exit(2)
        }
        core.debugRecord(seconds: seconds)
    }

    private func runMicTest() {
        log("麦克风自测：录 3 秒")
        core?.debugRecord(seconds: 3)
    }

    private func copyLastResult() {
        guard let text = core?.takeLastResult(), !text.isEmpty else {
            status?.showNotice("还没有结果可取回")
            return
        }
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(text, forType: .string)
        core?.dropLastResult()   // 用户已经拿到了，清掉崩溃恢复暂存
        status?.showNotice("已复制上次结果（\(text.count) 字）")
    }

    // MARK: - 无 UI 自测（--self-test）

    private func startSelfTest(_ core: Core) {
        core.onEvent = { [weak self] event in self?.handle(event) }
        log("self-test 开始")
        DispatchQueue.main.asyncAfter(deadline: .now() + 5) { [weak self] in
            self?.failSelfTest("超时：5 秒内没有走完 识别→上屏→空闲")
        }
        core.debugSubmit("自测：你好，Eryu。")
    }

    private func checkSelfTest(_ event: CoreEvent) {
        if event.phase == "idle" && selfTestSeen.contains(where: { $0 == "state:inserting" }) {
            passSelfTest()
        }
    }

    private func passSelfTest() {
        let text = core?.takeLastResult() ?? ""
        print("SELFTEST OK 事件序列: " + selfTestSeen.joined(separator: " → ") + " | 结果=\(text)")
        core = nil
        exit(0)
    }

    private func failSelfTest(_ why: String) {
        print("SELFTEST FAIL: " + why + " | 序列: " + selfTestSeen.joined(separator: " → "))
        core = nil
        exit(2)
    }
}

// MARK: - 一次真实按键的验收（--acceptance）

/// 用户按一次真实的按住说话，把「能看到什么、最后上屏什么」记录下来。
/// 只观察，不注入（dryRun）：避免把测试句子粘进用户正在写的文档里。
struct AcceptanceRun {
    var started = Date()
    var sawListening = false
    var partials: [String] = []
    var result: String?
    var resultLatency: Int?
    var resultWait: Int = 0
    var resultEngineWait: Int = 0
    var errorCode: String?
    var errorMessage: String?
    var overlayShown = false
}

extension AppDelegate {
    func runAcceptance() {
        var run = AcceptanceRun.init()
        run.started = Date()
        acceptance = run
        injector.dryRun = true

        let perms = Permission.allCases.map { "\($0.title)=\($0.state.text)" }.joined(separator: " / ")
        print("ACCEPTANCE 权限：\(perms)")
        print("ACCEPTANCE 模型：\(model?.config.engine ?? "?") / 热键：\(model?.config.hotkey ?? "?")")
        print("ACCEPTANCE 请在 20 秒内按住右 Option 说一句话（说完松开）…")

        // --acceptance 分支早于正式热键创建点，这里自己建一次并接上
        if hotkey == nil {
            let hk = Hotkey()
            hk.onDown = { [weak self] in self?.handleHotkeyDown() }
            hk.onUp = { [weak self] in self?.handleHotkeyUp() }
            hotkey = hk
        }
        applyHotkey(model?.config.hotkey ?? "ALT_R")

        // 自测用：--acceptance-synthetic <wav> 用合成按键 + WAV 当麦克风跑一遍（仍然不注入）
        if let i = CommandLine.arguments.firstIndex(of: "--acceptance-synthetic"),
           i + 1 < CommandLine.arguments.count {
            let wav = CommandLine.arguments[i + 1]
            let size = ((try? FileManager.default.attributesOfItem(atPath: wav))?[.size] as? Int64) ?? 0
            if size < 1000 {
                print("ACCEPTANCE ⚠️ 模拟麦克风 WAV 读不出来或太短（\(wav)，\(size) 字节）——会退到真实麦克风")
            }
            setenv("ERYU_MIC_WAV", wav, 1)
            let secs = (try? AVAudioFile(forReading: URL(fileURLWithPath: wav)))
                .map { Double($0.length) / $0.fileFormat.sampleRate } ?? 3
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
                Self.postModifier(code: 61, down: true)
                DispatchQueue.main.asyncAfter(deadline: .now() + secs) {
                    Self.postModifier(code: 61, down: false)
                }
            }
        }

        DispatchQueue.main.asyncAfter(deadline: .now() + 20) { [weak self] in
            self?.finishAcceptance()
        }
    }

    private func recordAcceptance(_ event: CoreEvent) {
        guard var run = acceptance else { return }
        switch event.kind {
        case "state":
            if event.phase == "listening" {
                run.sawListening = true
                run.overlayShown = overlay?.isVisible == true
            }
        case "partial_text":
            run.partials.append(event.string)
        case "result":
            run.result = event.string
            run.resultLatency = event.latencyMs
            run.resultWait = Int(event.payload["wait_ms"] as? Double ?? 0)
            run.resultEngineWait = Int(event.payload["engine_wait_ms"] as? Double ?? 0)
        case "error":
            run.errorCode = event.code
            run.errorMessage = event.message
        default:
            break
        }
        acceptance = run
        // 拿到结果就提前收工，不用等满 20 秒
        if event.kind == "result" || (event.kind == "error" && event.code != "polish_failed") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { [weak self] in self?.finishAcceptance() }
        }
    }

    private func finishAcceptance() {
        guard let run = acceptance else { return }
        acceptance = nil
        var report: [String: Any] = [:]
        func verdict(_ ok: Bool, _ name: String, _ detail: String) {
            print("ACCEPTANCE \(ok ? "✅" : "❌") \(name)：\(detail)")
            var list = (report["checks"] as? [[String: Any]]) ?? []
            list.append(["name": name, "ok": ok, "detail": detail])
            report["checks"] = list
        }

        let perms = Permission.allCases
        let permOk = perms.allSatisfy { $0.state.ok }
        verdict(permOk, "三项权限", perms.map { "\($0.title)=\($0.state.text)" }.joined(separator: " / "))
        verdict(run.sawListening, "接住按键开始录音", run.sawListening ? "收到 listening" : "20 秒内没收到按键（权限或热键没生效）")
        verdict(run.overlayShown, "浮层出现", run.overlayShown ? "listening 时浮层可见" : "没看到浮层")
        verdict(!run.partials.isEmpty, "边说边出字", run.partials.isEmpty ? "没有流式片段" : "\(run.partials.count) 段，最后一段：\(run.partials.last ?? "")")
        verdict(run.result != nil, "松手出结果", run.result.map { "\($0.count) 字：\($0.prefix(40))" } ?? (run.errorMessage ?? "没有结果"))
        if let ms = run.resultLatency {
            // 产品目标是 ≤1500ms（空闲实测 ~400–1000ms）；这台机器常同时跑构建/DSH，
            // 用 ERYU_ACCEPT_THRESHOLD_MS 给验收脚本一个更宽的上限，避免把环境噪声当回归（I-057）。
            let limit = UInt64(ProcessInfo.processInfo.environment["ERYU_ACCEPT_THRESHOLD_MS"] ?? "") ?? 1500
            var detail = "\(ms)ms"
            if run.resultWait > 0 || run.resultEngineWait > 0 {
                detail += "（其中等预览块提交 \(run.resultWait)ms、等引擎锁 \(run.resultEngineWait)ms）"
            }
            verdict(ms <= limit, "延迟 ≤\(limit)ms", detail)
        }
        report["elapsedSecs"] = Int(Date().timeIntervalSince(run.started))
        report["partials"] = run.partials
        report["result"] = run.result ?? ""
        report["latencyMs"] = run.resultLatency ?? 0
        report["waitMs"] = run.resultWait
        report["engineWaitMs"] = run.resultEngineWait
        report["errorCode"] = run.errorCode ?? ""
        report["errorMessage"] = run.errorMessage ?? ""

        let path = "/tmp/eryu-acceptance.json"
        if let data = try? JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted]) {
            try? data.write(to: URL(fileURLWithPath: path))
        }
        let checks = (report["checks"] as? [[String: Any]]) ?? []
        let passed = checks.allSatisfy { ($0["ok"] as? Bool) == true }
        print("ACCEPTANCE \(passed ? "OK" : "FAIL") 报告写好了：\(path)")
        core?.cancel()
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { exit(passed ? 0 : 1) }
    }
}

// MARK: - 首次运行：缺模型时问一次，别让用户按了热键才发现没模型

extension AppDelegate {
    func maybeOfferModelDownload() {
        guard !CommandLine.arguments.contains(where: { $0.hasPrefix("--") && $0 != "--no-permission-prompt" }) else { return }
        // 模型清单是 core 异步回的：先问一次，落空再补问（否则首次启动时清单还是空的，弹窗永远不出现）
        offerModelIfKnown(attempt: 0)
    }

    private func offerModelIfKnown(attempt: Int) {
        guard let model = self.model else { return }
        model.refreshModels()
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) { [weak self] in
            guard let self, let model = self.model else { return }
            if model.models.isEmpty && attempt < 3 {
                self.offerModelIfKnown(attempt: attempt + 1)
                return
            }
            // 引擎在 models 清单里；标点/降噪在 config JSON 的就绪位里（它们不是引擎）
            var missing: [String] = model.models
                .filter { !$0.installed && ($0.id == "sense" || $0.id == "qwen3") }
                .map { $0.id }
            if !model.config.punct_ready { missing.append("punct") }
            if model.config.denoise && !model.config.denoise_ready { missing.append("gtcrn") }
            print("[eryu] 首次运行缺模型：\(missing)")
            guard !missing.isEmpty else { return }
            let sizes = ["sense": 247, "qwen3": 839, "punct": 65, "gtcrn": 1]
            let titles = ["sense": "语音识别（SenseVoice）", "qwen3": "高精度引擎（Qwen3）",
                          "punct": "标点模型（ct-transformer）", "gtcrn": "降噪模型（GTCRN）"]
            let mb = missing.reduce(0) { $0 + (sizes[$1] ?? 0) }
            let names = missing.map { titles[$0] ?? $0 }.joined(separator: "、")
            let alert = NSAlert()
            alert.messageText = "还需要下载语音模型"
            alert.informativeText = """
            要用它开始听写，还需要 \(names)（约 \(mb)MB）。
            下载走系统 curl，模型存在 ~/.eryu/models，之后就不需要网络了。
            """
            alert.addButton(withTitle: "现在下载")
            alert.addButton(withTitle: "稍后（在设置 → 引擎里下载）")
            if alert.runModal() == .alertFirstButtonReturn {
                for id in missing { model.pullModel(id) }
                self.status?.showNotice("开始下载 \(missing.count) 个模型（约 \(mb)MB），下完会提示")
            }
        }
    }
}

// MARK: - 录音中按 Esc 取消

extension AppDelegate {
    /// 只在「正在录 / 正在准备」时旁听 Esc：按住说话时想放弃，不用去菜单里点
    func updateEscCancel(phase: String) {
        let want = (phase == "starting" || phase == "listening")
        if want {
            guard escTap == nil else { return }
            let tap = KeyTap()
            tap.targetKeyCode = 53          // kVK_Escape
            tap.isModifier = false
            tap.onDown = { [weak self] in
                guard let self else { return }
                self.core?.cancel()
                self.status?.showNotice("已取消这次录音（不会上屏）")
                self.overlay?.showError("已取消")
            }
            if let reason = tap.start() {
                log("Esc 取消不可用：\(reason)")
                return
            }
            escTap = tap
        } else {
            escTap?.stop()
            escTap = nil
        }
    }
}

extension AppDelegate {
    /// 在自己进程里找第一个可编辑的 AX 元素（输入框/多行框），给自测用
    static func firstEditable(in pid: pid_t) -> AXUIElement? {
        func children(_ el: AXUIElement) -> [AXUIElement] {
            var raw: CFTypeRef?
            guard AXUIElementCopyAttributeValue(el, kAXChildrenAttribute as CFString, &raw) == .success,
                  let arr = raw as? [AXUIElement] else { return [] }
            return arr
        }
        func role(_ el: AXUIElement) -> String {
            var raw: CFTypeRef?
            _ = AXUIElementCopyAttributeValue(el, kAXRoleAttribute as CFString, &raw)
            return raw as? String ?? ""
        }
        func search(_ el: AXUIElement, depth: Int) -> AXUIElement? {
            if depth > 8 { return nil }
            let r = role(el)
            if r == "AXTextArea" || r == "AXTextField" { return el }
            for c in children(el) {
                if let hit = search(c, depth: depth + 1) { return hit }
            }
            return nil
        }
        let app = AXUIElementCreateApplication(pid)
        let wins = children(app)
        if ProcessInfo.processInfo.environment["ERYU_AX_DEBUG"] != nil {
            for w in wins {
                print("AXDEBUG win role=\(role(w)) children=\(children(w).count)")
                for c in children(w) { print("AXDEBUG   child role=\(role(c))") }
            }
        }
        for w in wins {
            if let hit = search(w, depth: 0) { return hit }
        }
        return nil
    }
}
