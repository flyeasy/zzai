import AppKit

/// 菜单栏：状态 + 设置 + 取回结果 + 退出（M1b）
final class StatusController {
    private let statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    private let stateItem = NSMenuItem(title: "就绪", action: nil, keyEquivalent: "")
    private let detailItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
    /// 最近一次转写预览（DESIGN §5.1：菜单栏状态弹窗保留「最近转写预览 + 复制」）
    private let recentItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
    /// 上一次的文本与耗时合并成一行（R-129：两行「上次：」看着乱）
    private var lastRecentText = ""
    private var lastLatencyMs: Double = 0
    /// 部分成功后的「只重试这一段」子菜单
    private let failedSegmentsItem = NSMenuItem(title: "重识别失败的段", action: nil, keyEquivalent: "")
    private var retrySegmentHandler: ((Int) -> Void)?
    private let core: Core
    private let onSelfTest: () -> Void
    private let onCancel: () -> Void
    private let onCopyLast: () -> Void
    private let onMicTest: () -> Void
    private let onSettings: () -> Void
    private let onHistory: () -> Void
    private let onShowMain: () -> Void
    private let onRetryFailed: () -> Void
    private let onSelectionCommand: () -> Void
    private let onRelaunch: () -> Void
    private let onPermReport: () -> Void
    private let onResetPermissions: () -> Void
    private var hotkeyHint: String

    init(core: Core, hotkeyHint: String,
         onSelfTest: @escaping () -> Void, onCancel: @escaping () -> Void,
         onCopyLast: @escaping () -> Void, onMicTest: @escaping () -> Void,
         onSettings: @escaping () -> Void, onHistory: @escaping () -> Void,
         onShowMain: @escaping () -> Void = {},
         onRetryFailed: @escaping () -> Void,
         onSelectionCommand: @escaping () -> Void,
         onRelaunch: @escaping () -> Void,
         onPermReport: @escaping () -> Void,
         onResetPermissions: @escaping () -> Void) {
        self.core = core
        self.hotkeyHint = hotkeyHint
        self.onSelfTest = onSelfTest
        self.onCancel = onCancel
        self.onCopyLast = onCopyLast
        self.onMicTest = onMicTest
        self.onSettings = onSettings
        self.onHistory = onHistory
        self.onShowMain = onShowMain
        self.onRetryFailed = onRetryFailed
        self.onSelectionCommand = onSelectionCommand
        self.onRelaunch = onRelaunch
        self.onPermReport = onPermReport
        self.onResetPermissions = onResetPermissions
        configure()
    }

    private func configure() {
        if let button = statusItem.button {
            let image = NSImage(systemSymbolName: "waveform", accessibilityDescription: "字在")
            image?.isTemplate = true
            button.image = image
            button.title = image == nil ? "E" : ""
            button.toolTip = tooltip()
        }

        stateItem.isEnabled = false
        // 状态行不加图标：菜单里两个看不懂的符号比文字更费解（R-129）
        stateItem.image = nil

        detailItem.isEnabled = false
        detailItem.isHidden = true
        detailItem.image = nil

        let menu = NSMenu()
        menu.addItem(stateItem)
        menu.addItem(detailItem)
        recentItem.isEnabled = false
        recentItem.isHidden = true
        menu.addItem(recentItem)

        failedSegmentsItem.isHidden = true
        menu.addItem(failedSegmentsItem)
        menu.addItem(.separator())

        let settings = NSMenuItem(title: "设置…", action: #selector(openSettings), keyEquivalent: ",")
        settings.target = self
        menu.addItem(settings)

        let main = NSMenuItem(title: "显示主窗", action: #selector(showMain), keyEquivalent: "0")
        main.target = self
        menu.addItem(main)

        let history = NSMenuItem(title: "统计与历史…", action: #selector(openHistory), keyEquivalent: "h")
        history.target = self
        menu.addItem(history)

        menu.addItem(.separator())

        let copy = NSMenuItem(title: "复制上次结果", action: #selector(copyLast), keyEquivalent: "c")
        copy.target = self
        menu.addItem(copy)

        let retry = NSMenuItem(title: "重试上次失败的识别", action: #selector(retryFailed), keyEquivalent: "r")
        retry.target = self
        menu.addItem(retry)

        let sel = NSMenuItem(title: "选区改写…", action: #selector(selectionCommand), keyEquivalent: "e")
        sel.target = self
        menu.addItem(sel)

        let cancel = NSMenuItem(title: "取消当前录音", action: #selector(cancelRecording), keyEquivalent: ".")
        cancel.target = self
        menu.addItem(cancel)

        // 自测/测试/权限自助这些是给开发和客服排障用的：正常用户在菜单里看到只会问「这是什么」。
        // 要它们时用 `open -a Eryu --args --dev`，平时菜单只留「用户真正会点的」。
        if DevMode.on {
            menu.addItem(.separator())
            let test = NSMenuItem(title: "自测：上屏一段文字", action: #selector(selfTest), keyEquivalent: "t")
            test.target = self
            menu.addItem(test)

            let mic = NSMenuItem(title: "测试：录 3 秒并识别", action: #selector(micTest), keyEquivalent: "m")
            mic.target = self
            menu.addItem(mic)

            let perm = NSMenuItem(title: "权限状态…", action: #selector(permReport), keyEquivalent: "")
            perm.target = self
            menu.addItem(perm)
            let resetPerm = NSMenuItem(title: "重新授权（清掉旧副本的授权）…", action: #selector(resetPermissions), keyEquivalent: "")
            resetPerm.target = self
            menu.addItem(resetPerm)
            let relaunch = NSMenuItem(title: "退出并重新打开（授权后没生效时用）", action: #selector(relaunchApp), keyEquivalent: "R")
            relaunch.keyEquivalentModifierMask = [.command, .shift]
            relaunch.target = self
            menu.addItem(relaunch)
        }

        menu.addItem(.separator())
        let quit = NSMenuItem(title: "退出 Eryu", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        menu.addItem(quit)
        statusItem.menu = menu
    }

    private func tooltip() -> String {
        "字在 AI 输入法（\(hotkeyHint) 按住说话）"
    }

    func setHotkeyHint(_ spec: String) {
        hotkeyHint = spec
        statusItem.button?.toolTip = tooltip()
    }

    @objc private func permReport() { onPermReport() }

    @objc private func resetPermissions() { onResetPermissions() }

    @objc private func selfTest() { onSelfTest() }
    @objc private func micTest() { onMicTest() }
    @objc private func cancelRecording() { onCancel() }
    @objc private func copyLast() { onCopyLast() }
    @objc private func openSettings() { onSettings() }
    @objc private func openHistory() { onHistory() }
    @objc private func showMain() { onShowMain() }
    @objc private func retryFailed() { onRetryFailed() }
    @objc private func selectionCommand() { onSelectionCommand() }
    @objc private func relaunchApp() { onRelaunch() }

    func show(phase: String) {
        let text: String
        let symbol: String
        switch phase {
        case "starting":   text = "准备中…";      symbol = "circle.dotted"
        case "listening":  text = "在听…（松开结束）"; symbol = "waveform.circle.fill"
        case "processing": text = "识别中…";      symbol = "ellipsis.circle"
        case "inserting":  text = "上屏中…";      symbol = "arrow.up.circle"
        default:           text = "就绪";         symbol = "circle"
        }
        _ = symbol
        stateItem.title = "字在 · \(text)"
        stateItem.image = nil
        // 菜单栏图标也跟着变：扫一眼就知道现在是在录、在识别、还是闲着
        let icon: String
        switch phase {
        case "starting", "listening": icon = "waveform.circle.fill"
        case "processing": icon = "ellipsis.circle.fill"
        case "inserting": icon = "arrow.up.circle.fill"
        default: icon = "waveform"
        }
        if let button = statusItem.button {
            let img = NSImage(systemSymbolName: icon, accessibilityDescription: text)
            img?.isTemplate = true
            button.image = img
        }
        statusItem.button?.toolTip = "字在：\(text)" 
        if phase == "idle" { detailItem.title = ""; detailItem.isHidden = true }
    }

    /// 显示最近一次转写（超过一行就截断，完整内容用「复制上次结果」取）
    /// 菜单里显示最近一次「松手 → 出字」耗时，方便用户/我们判断是不是变慢了
    /// 部分成功时：把「重识别失败的段」放进菜单（无失败段就隐藏）
    func setFailedSegments(_ indexes: [Int], handler: @escaping (Int) -> Void) {
        failedSegmentsItem.isHidden = indexes.isEmpty
        failedSegmentsItem.submenu = indexes.isEmpty ? nil : NSMenu()
        guard let sub = failedSegmentsItem.submenu else { return }
        for i in indexes {
            let item = NSMenuItem(title: "第 #\(i) 段（重跑这一段的音频）",
                                  action: nil, keyEquivalent: "")
            item.representedObject = i
            item.target = self
            item.action = #selector(retrySegmentClicked(_:))
            sub.addItem(item)
        }
        retrySegmentHandler = handler
    }

    @objc private func retrySegmentClicked(_ sender: NSMenuItem) {
        if let i = sender.representedObject as? Int {
            retrySegmentHandler?(i)
        }
    }

    func setLatency(_ ms: Double) {
        lastLatencyMs = ms
        renderRecent()
    }

    /// 自测用：读回「最近」菜单项现在显示什么（隐私模式应恒为空）
    var recentText: String { recentItem.isHidden ? "" : recentItem.title }

    func setRecent(_ text: String) {
        let clean = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty else {
            recentItem.isHidden = true
            return
        }
        lastRecentText = clean
        renderRecent()
    }

    /// 「上次：……（N 字 · 1.2s）」——一条就够，不拆两行（R-129）
    private func renderRecent() {
        let clean = lastRecentText
        guard !clean.isEmpty else {
            recentItem.isHidden = true
            return
        }
        let limit = 28
        let shown = clean.count > limit ? String(clean.prefix(limit)) + "…" : clean
        var tail = "\(clean.count) 字"
        if lastLatencyMs > 0 {
            tail += String(format: " · %.1fs", lastLatencyMs / 1000)
        }
        recentItem.title = "上次：\(shown)（\(tail)）"
        recentItem.image = nil
        recentItem.isHidden = false
    }

    func showError(code: String, message: String) {
        detailItem.title = "⚠︎ " + message
        detailItem.isHidden = false
        setBadge("waveform.badge.exclamationmark", description: "字在 出错")
        _ = code
    }

    func showNotice(_ text: String) {
        detailItem.title = text
        detailItem.isHidden = text.isEmpty
    }

    private func setBadge(_ symbol: String, description: String) {
        if let button = statusItem.button {
            button.image = NSImage(systemSymbolName: symbol, accessibilityDescription: description)
            button.image?.isTemplate = true
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 3) { [weak self] in
            guard let self else { return }
            let image = NSImage(systemSymbolName: "waveform", accessibilityDescription: "字在")
            image?.isTemplate = true
            self.statusItem.button?.image = image
        }
    }
}
