import AppKit
import ApplicationServices

/// 选区指令管线（DESIGN §10.2）
///
/// 流程：读当前选中文字 → 浮层列出指令 → 后台调 core `eryu_run_command` → 用改写结果替换选区。
/// 替换优先走辅助功能直接写 `kAXSelectedTextAttribute`（不经剪贴板、不留痕），
/// 失败再退回「写剪贴板 + ⌘V」（与上屏同一条路）。
final class SelectionLane {
    struct Command {
        let title: String
        let detail: String
        let instruction: String
    }

    static let commands: [Command] = [
        Command(title: "润色", detail: "更通顺自然，意思不变",
                instruction: "把这段文字润色得更通顺自然，保持原意、原语言，只输出结果。"),
        Command(title: "正式", detail: "改成书面语",
                instruction: "把这段文字改写成正式、得体的书面语，保持原意，只输出结果。"),
        Command(title: "精炼", detail: "压到七成以内",
                instruction: "把这段文字精炼到原长度的七成以内，保留全部关键信息，只输出结果。"),
        Command(title: "英文", detail: "翻译成英文",
                instruction: "把这段文字翻译成地道的英文，只输出译文。"),
        Command(title: "总结", detail: "三句话以内",
                instruction: "用不超过三句话总结这段文字的核心内容，用原文语言，只输出总结。"),
    ]

    private weak var core: Core?
    private weak var model: AppModel?
    private let onStatus: (String) -> Void
    private let onError: (String) -> Void
    private var palette: CommandPalette?
    private var busy = false

    init(core: Core?, model: AppModel?, onStatus: @escaping (String) -> Void, onError: @escaping (String) -> Void) {
        self.core = core
        self.model = model
        self.onStatus = onStatus
        self.onError = onError
    }

    /// 菜单/快捷键入口
    func begin() {
        guard !busy else {
            onStatus("上一次改写还在跑")
            return
        }
        guard let selection = AX.selection(), !selection.text.isEmpty else {
            onError("没有选中文字：先在应用里选中一段话，再按「选区改写」")
            return
        }
        let text = selection.text
        let palette = CommandPalette(commands: SelectionLane.commands, preview: text) { [weak self] command in
            self?.run(command, selection: text, element: selection.element)
        }
        self.palette = palette
        palette.show()
        onStatus("已选中 \(text.count) 字，选一条指令")
    }

    private func run(_ command: Command, selection text: String, element: AXUIElement?) {
        guard let core else {
            onError("core 未初始化")
            return
        }
        busy = true
        palette?.close()
        palette = nil
        onStatus("正在\(command.title)…（\(text.count) 字）")
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let (out, err) = core.runCommand(instruction: command.instruction, text: text)
            DispatchQueue.main.async {
                guard let self else { return }
                self.busy = false
                guard let out, !out.isEmpty else {
                    self.onError("\(command.title)失败：\(err ?? "服务没返回内容")")
                    return
                }
                if AX.replaceSelection(out, element: element) {
                    self.onStatus("已\(command.title)：替换了 \(text.count) 字")
                } else if self.pasteReplace(out) {
                    self.onStatus("已\(command.title)：用粘贴替换（\(out.count) 字）")
                } else {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(out, forType: .string)
                    self.onError("\(command.title)好了，但没能自动替换：结果已放进剪贴板，⌘V 即可")
                }
            }
        }
    }

    private func pasteReplace(_ text: String) -> Bool {
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(text, forType: .string)
        guard let src = CGEventSource(stateID: .hidSystemState),
              let down = CGEvent(keyboardEventSource: src, virtualKey: 0x09, keyDown: true),
              let up = CGEvent(keyboardEventSource: src, virtualKey: 0x09, keyDown: false) else {
            return false
        }
        down.flags = .maskCommand
        up.flags = .maskCommand
        down.post(tap: .cghidEventTap)
        up.post(tap: .cghidEventTap)
        return true
    }
}

// MARK: - 辅助功能读写选区

enum AX {
    static func focusedElement() -> AXUIElement? {
        let system = AXUIElementCreateSystemWide()
        var raw: CFTypeRef?
        let err = AXUIElementCopyAttributeValue(system, kAXFocusedUIElementAttribute as CFString, &raw)
        if err != .success {
            NSLog("[eryu] AX focused error=%d trusted=%d", err.rawValue, AXIsProcessTrusted() ? 1 : 0)
            return nil
        }
        guard let raw else { return nil }
        // SAFETY: 该属性返回 AXUIElement
        return (raw as! AXUIElement)
    }

    static func selection() -> (text: String, element: AXUIElement?)? {
        guard let el = focusedElement() else { return nil }
        var raw: CFTypeRef?
        let err = AXUIElementCopyAttributeValue(el, kAXSelectedTextAttribute as CFString, &raw)
        if err != .success {
            NSLog("[eryu] AX selectedText error=%d trusted=%d", err.rawValue, AXIsProcessTrusted() ? 1 : 0)
            return nil
        }
        guard let text = raw as? String, !text.isEmpty else {
            NSLog("[eryu] AX selectedText empty")
            return nil
        }
        return (text, el)
    }

    /// 直接写回选区（不动剪贴板）；成功返回 true
    static func replaceSelection(_ text: String, element: AXUIElement?) -> Bool {
        guard let el = element ?? focusedElement() else { return false }
        let ok = AXUIElementSetAttributeValue(el, kAXSelectedTextAttribute as CFString, text as CFString) == .success
        return ok
    }
}

// MARK: - 指令浮层

private final class PalettePanel: NSPanel {
    weak var keys: CommandPalette?

    override var canBecomeKey: Bool { true }

    override func keyDown(with event: NSEvent) {
        guard let keys else {
            super.keyDown(with: event)
            return
        }
        switch event.keyCode {
        case 53: keys.cancel()             // esc
        case 36, 76: keys.confirm()        // return / enter
        case 125: keys.move(1)             // down
        case 126: keys.move(-1)            // up
        default:
            if let ch = event.charactersIgnoringModifiers, let n = Int(ch), n >= 1, n <= SelectionLane.commands.count {
                keys.pick(n - 1)
            } else {
                super.keyDown(with: event)
            }
        }
    }
}

final class CommandPalette {
    private let commands: [SelectionLane.Command]
    private let preview: String
    private let onPick: (SelectionLane.Command) -> Void
    private var panel: PalettePanel?
    private var views: [RowView] = []
    private var selected = 0

    init(commands: [SelectionLane.Command], preview: String, onPick: @escaping (SelectionLane.Command) -> Void) {
        self.commands = commands
        self.preview = preview
        self.onPick = onPick
    }

    func show() {
        let width: CGFloat = 460
        let header: CGFloat = 46
        let rowH: CGFloat = 46
        let height = header + rowH * CGFloat(commands.count) + 10
        let panel = PalettePanel(contentRect: NSRect(x: 0, y: 0, width: width, height: height),
                                 styleMask: [.borderless, .nonactivatingPanel],
                                 backing: .buffered, defer: false)
        panel.keys = self
        panel.isFloatingPanel = true
        panel.level = .modalPanel
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        panel.hidesOnDeactivate = false

        let root = NSView(frame: NSRect(x: 0, y: 0, width: width, height: height))
        root.wantsLayer = true
        root.layer?.backgroundColor = NSColor.textBackgroundColor.withAlphaComponent(0.98).cgColor
        root.layer?.cornerRadius = 16
        root.layer?.borderWidth = 0.8
        root.layer?.borderColor = NSColor.separatorColor.cgColor
        root.layer?.masksToBounds = true

        let title = NSTextField(labelWithString: "改写选中的 \(preview.count) 字")
        title.font = .systemFont(ofSize: 11, weight: .medium)
        title.textColor = .secondaryLabelColor
        title.frame = NSRect(x: 16, y: height - 26, width: width - 32, height: 16)
        root.addSubview(title)

        let body = NSTextField(labelWithString: preview.replacingOccurrences(of: "\n", with: " "))
        body.font = .systemFont(ofSize: 12)
        body.textColor = .labelColor
        body.lineBreakMode = .byTruncatingTail
        body.maximumNumberOfLines = 1
        body.frame = NSRect(x: 16, y: height - 44, width: width - 32, height: 16)
        root.addSubview(body)

        for (i, cmd) in commands.enumerated() {
            let y = height - header - rowH * CGFloat(i + 1)
            let row = RowView(index: i, command: cmd, width: width, height: rowH)
            row.frame = NSRect(x: 6, y: y, width: width - 12, height: rowH - 4)
            root.addSubview(row)
            views.append(row)
        }
        panel.contentView = root

        let screen = NSScreen.main?.visibleFrame ?? .zero
        panel.setFrameOrigin(NSPoint(x: screen.midX - width / 2, y: screen.midY - height / 2))
        self.panel = panel
        highlight(0)
        NSApp.activate(ignoringOtherApps: true)
        panel.orderFrontRegardless()
        panel.makeKey()
        NSLog("[eryu] palette shown=%d frame=%@", panel.isVisible ? 1 : 0, NSStringFromRect(panel.frame))
    }

    func move(_ delta: Int) {
        highlight((selected + delta + commands.count) % commands.count)
    }

    func pick(_ index: Int) {
        guard index >= 0, index < commands.count else { return }
        let cmd = commands[index]
        close()
        onPick(cmd)
    }

    func confirm() { pick(selected) }

    func cancel() { close() }

    func close() {
        panel?.orderOut(nil)
        panel = nil
        views = []
    }

    private func highlight(_ index: Int) {
        selected = index
        for (i, row) in views.enumerated() {
            row.isHighlighted = (i == index)
        }
    }

    /// 单条指令行
    private final class RowView: NSView {
        private let title = NSTextField(labelWithString: "")
        private let detail = NSTextField(labelWithString: "")
        private let number = NSTextField(labelWithString: "")
        let index: Int

        var isHighlighted = false {
            didSet { refresh() }
        }

        init(index: Int, command: SelectionLane.Command, width: CGFloat, height: CGFloat) {
            self.index = index
            super.init(frame: NSRect(x: 0, y: 0, width: width, height: height))
            wantsLayer = true
            layer?.cornerRadius = 10

            number.stringValue = "\(index + 1)"
            number.font = .monospacedDigitSystemFont(ofSize: 11, weight: .semibold)
            number.textColor = .tertiaryLabelColor
            number.frame = NSRect(x: 10, y: height / 2 - 8, width: 14, height: 16)
            addSubview(number)

            title.stringValue = command.title
            title.font = .systemFont(ofSize: 13, weight: .medium)
            title.frame = NSRect(x: 30, y: height / 2 - 10, width: 120, height: 18)
            addSubview(title)

            detail.stringValue = command.detail
            detail.font = .systemFont(ofSize: 12)
            detail.textColor = .secondaryLabelColor
            detail.frame = NSRect(x: 130, y: height / 2 - 9, width: width - 150, height: 18)
            addSubview(detail)

            refresh()
        }

        required init?(coder: NSCoder) { fatalError("not supported") }

        private func refresh() {
            layer?.backgroundColor = isHighlighted
                ? NSColor.controlAccentColor.withAlphaComponent(0.22).cgColor
                : NSColor.clear.cgColor
            title.textColor = isHighlighted ? .labelColor : .labelColor
        }

        override func mouseDown(with event: NSEvent) {
            // 单选行：点击也要能选中执行（焦点在浮层上时最省事）
            NSApp.sendAction(#selector(NSResponder.keyDown(with:)), to: nil, from: event)
        }
    }
}
