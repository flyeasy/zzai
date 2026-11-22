import AppKit

/// 录音浮层（胶囊）
///
/// 形态（D-103/D-104）：
///  · 胶囊里只有：三根电平条 + 当前润色档图标 + 最新识别文本 + 一个**闪动的省略号**
///  · 省略号本身就表示「还在等录入」，不再另加状态词或跳动点
///  · 操作提示（松开上屏 · Esc 取消）放在胶囊**外面下方**，10pt 小灰字
///  · 胶囊宽度只增不减、按 24pt 台阶量化；位置在一次录音里锁死——避免「大小闪变、位置闪变」
final class OverlayController {
    private var panel: NSPanel?
    private var view: OverlayContentView?
    private var hideWork: DispatchWorkItem?

    var isVisible: Bool { panel?.isVisible == true && (panel?.alphaValue ?? 0) > 0 }

    private static let hintHeight: CGFloat = 12
    private static let hintGap: CGFloat = 4
    private static let capsuleHeight: CGFloat = 40
    private var size = NSSize(width: 220, height: 56)
    private var currentPhase = "idle"
    /// 一次会话里锁定的左上角位置：录音过程中绝不移动
    private var lockedOrigin: NSPoint?

    var profileProvider: (() -> (String, String))?

    func show(phase: String) {
        hideWork?.cancel()
        let previous = currentPhase
        currentPhase = phase
        if let p = profileProvider?() { view?.setProfile(icon: p.0, name: p.1) }
        switch phase {
        case "starting", "listening":
            // 「新的一段」= 上一刻不在录音里（idle/processing/inserting 都算）。上一段结束到下一次按下之间
            // 可能只隔几百毫秒，旧句子还挂在胶囊里，用户会觉得「怎么还留着上一段，甚至是别的内容」。
            let newSession = (previous != "starting" && previous != "listening")
            if newSession {
                // 新的一次录音：先清掉上一段的文字，只留一个闪动的光标；宽度回到基础值、位置重新取插入点
                view?.partial = ""
                view?.redacted = nil
                lockedOrigin = nil
                size = NSSize(width: baseWidth(), height: size.height)
                view?.resetWidth()
                if ProcessInfo.processInfo.environment["ERYU_OV_TRACE"] != nil {
                    NSLog("[ov-session] 新一段录音：清掉上一段文字（previous=\(previous)）")
                }
                resize(to: baseWidth(), reset: true)
            }
            view?.phase = phase
            present()
        case "processing":
            view?.phase = phase
            view?.level = 0
            present()
        case "inserting":
            view?.phase = phase
            view?.level = 0
            present()
        case "idle":
            view?.phase = "idle"
            view?.level = 0
            let work = DispatchWorkItem { [weak self] in
                guard let self, self.view?.phase == "idle" else { return }
                self.hide()
            }
            hideWork = work
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.6, execute: work)
        default:
            break
        }
    }

    func setProfile(icon: String, name: String) {
        view?.setProfile(icon: icon, name: name)
    }

    func setLevel(_ rms: Float) {
        view?.level = rms
    }

    func setSecs(_ secs: Double) {}

    func setPartial(_ text: String) {
        if text.isEmpty { view?.partial = ""; return }
        view?.partial = text
        view?.phase = currentPhase
        growIfNeeded(for: text)
    }

    func setFinal(_ text: String) {
        view?.phase = "processing"
        view?.partial = text
        growIfNeeded(for: text)
    }

    func setFinalRedacted(_ chars: Int) {
        view?.partial = ""
        view?.redacted = "已上屏 \(chars) 字 · 隐私模式不显示内容"
        view?.phase = "processing"
        growIfNeeded(for: "已上屏 000 字 · 隐私模式不显示内容")
    }

    func setPartialRedacted() {
        view?.redacted = "识别中…（隐私模式）"
    }

    func showError(_ message: String) {
        view?.partial = message
        view?.phase = "error"
        growIfNeeded(for: message)
        present()
        let work = DispatchWorkItem { [weak self] in self?.hide() }
        hideWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 3.0, execute: work)
    }

    // MARK: - 宽度（只增不减，台阶量化）

    private func baseWidth() -> CGFloat { 190 }
    /// 宽度上限：到这儿就不再长，多出来的字从**头部**压掉，末尾（最新内容 + 闪动省略号）永远可见
    private let maxWidth: CGFloat = 420

    private func requiredWidth(for text: String) -> CGFloat {
        let font = NSFont.systemFont(ofSize: 14)
        let textW = (text as NSString).size(withAttributes: [.font: font]).width
        // 左：12 + 电平条 3×3.5+2×2.5=15.5 + 9；图标 18 + 8；闪烁下划线 12；右留白 14
        let w = 12 + 15.5 + 9 + 18 + 8 + textW + 12 + 14
        return min(max(w, baseWidth()), maxWidth)
    }

    private func growIfNeeded(for text: String) {
        let need = requiredWidth(for: text)
        guard need > size.width + 12 else { return }
        let stepped = min(maxWidth, ceil(need / 24) * 24)
        resize(to: stepped, reset: false)
    }

    private func resize(to width: CGFloat, reset: Bool) {
        guard abs(width - size.width) > 0.5 || reset else { return }
        let oldWidth = size.width
        size = NSSize(width: width, height: size.height)
        guard let panel else { return }
        var frame = panel.frame
        // 改宽时保持中心 x 与顶边不变（视觉上不「跳」）
        frame.origin.x += (oldWidth - width) / 2
        frame.size = size
        if var o = lockedOrigin, !reset {
            // 注意：`o` 是「上一次定位」时的原点，宽度却是「当前」宽度。只拿它算这一次的新原点还不够——
            // 下一次再长宽时会又用旧 `o` 重算，中心会一格一格往右挪（2026-11-21 用户报「胶囊变长时跑右边」）。
            // 所以把新原点写回 lockedOrigin，保证中心 x 在整个会话里恒定。
            let newX = o.x + (oldWidth - width) / 2
            frame.origin = NSPoint(x: newX, y: o.y)
            o.x = newX
            lockedOrigin = o
        }
        if ProcessInfo.processInfo.environment["ERYU_OV_TRACE"] != nil {
            NSLog("[ov-size] w=\(width) x=\(frame.origin.x) center=\(frame.origin.x + width / 2)")
        }
        if reset || !panel.isVisible {
            panel.setFrame(frame, display: true)
            panel.contentView?.frame.size = size
            panel.contentView?.layoutSubtreeIfNeeded()
        } else {
            // 宽度变化走一段短动画（0.16s ease-out）：像胶囊「长出来」，不是瞬间变宽
            panel.contentView?.frame.size = size
            panel.contentView?.layoutSubtreeIfNeeded()
            NSAnimationContext.runAnimationGroup { ctx in
                ctx.duration = 0.16
                ctx.timingFunction = CAMediaTimingFunction(name: .easeOut)
                panel.animator().setFrame(frame, display: true)
            }
        }
    }

    // MARK: - 面板

    private func present() {
        if panel == nil { panel = makePanel() }
        guard let panel else { return }
        if !panel.isVisible {
            position(panel)
            panel.orderFrontRegardless()
        }
        view?.startAnimating()
    }

    private func hide() {
        view?.stopAnimating()
        panel?.orderOut(nil)
        lockedOrigin = nil
    }

    private func makePanel() -> NSPanel {
        let p = NSPanel(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        p.isFloatingPanel = true
        p.level = .statusBar
        p.isOpaque = false
        p.backgroundColor = .clear
        p.hasShadow = false
        p.ignoresMouseEvents = true
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
        p.hidesOnDeactivate = false
        p.contentView = OverlayContentView(frame: NSRect(origin: .zero, size: size))
        view = p.contentView as? OverlayContentView
        return p
    }

    private func targetScreen() -> NSScreen? {
        if let caret = caretScreenPoint(),
           let s = NSScreen.screens.first(where: { NSMouseInRect(caret, $0.frame, false) }) {
            return s
        }
        let mouse = NSEvent.mouseLocation
        if let s = NSScreen.screens.first(where: { NSMouseInRect(mouse, $0.frame, false) }) { return s }
        return NSScreen.main
    }

    private func caretScreenPoint() -> CGPoint? {
        guard let front = NSWorkspace.shared.frontmostApplication else { return nil }
        let app = AXUIElementCreateApplication(front.processIdentifier)
        var focused: CFTypeRef?
        guard AXUIElementCopyAttributeValue(app, kAXFocusedUIElementAttribute as CFString, &focused) == .success,
              let el = focused, CFGetTypeID(el) == AXUIElementGetTypeID() else { return nil }
        let element = el as! AXUIElement

        var range: CFTypeRef?
        if AXUIElementCopyAttributeValue(element, kAXSelectedTextRangeAttribute as CFString, &range) == .success,
           let r = range, CFGetTypeID(r) == AXValueGetTypeID() {
            var cfRange = CFRange()
            if AXValueGetValue(r as! AXValue, .cfRange, &cfRange),
               let rect = boundsForRange(element, cfRange) {
                return screenPoint(fromAX: CGPoint(x: rect.origin.x + rect.width, y: rect.origin.y + rect.height))
            }
        }
        var pos: CFTypeRef?
        var sz: CFTypeRef?
        if AXUIElementCopyAttributeValue(element, kAXPositionAttribute as CFString, &pos) == .success,
           AXUIElementCopyAttributeValue(element, kAXSizeAttribute as CFString, &sz) == .success,
           let p = pos, let s = sz, CFGetTypeID(p) == AXValueGetTypeID(), CFGetTypeID(s) == AXValueGetTypeID() {
            var point = CGPoint.zero
            var ssize = CGSize.zero
            if AXValueGetValue(p as! AXValue, .cgPoint, &point), AXValueGetValue(s as! AXValue, .cgSize, &ssize) {
                return screenPoint(fromAX: CGPoint(x: point.x + ssize.width / 2, y: point.y + ssize.height))
            }
        }
        return nil
    }

    private func boundsForRange(_ element: AXUIElement, _ range: CFRange) -> CGRect? {
        var value: CFTypeRef?
        var cfRange = range
        guard let axRange = AXValueCreate(.cfRange, &cfRange) else { return nil }
        let err = AXUIElementCopyParameterizedAttributeValue(
            element, "AXBoundsForRange" as CFString, axRange, &value
        )
        guard err == .success, let v = value, CFGetTypeID(v) == AXValueGetTypeID() else { return nil }
        var rect = CGRect.zero
        guard AXValueGetValue(v as! AXValue, .cgRect, &rect) else { return nil }
        return rect
    }

    /// AX 坐标（左上原点、向下为正）→ 屏幕坐标（左下原点、向上为正）
    private func screenPoint(fromAX p: CGPoint) -> CGPoint {
        let main = NSScreen.screens.first?.frame ?? .zero
        return CGPoint(x: p.x, y: main.maxY - p.y)
    }

    private func position(_ panel: NSPanel) {
        if let o = lockedOrigin {
            panel.setFrame(NSRect(origin: o, size: size), display: true)
            return
        }
        guard let screen = targetScreen() else { return }
        let visible = screen.visibleFrame
        var y = visible.minY + 88
        if let caret = caretScreenPoint() {
            // 优先放光标下方；下面塞不下（含提示行）就翻到光标上方，保证提示始终看得见
            let below = caret.y - size.height - 6
            if below >= visible.minY + 28 {
                y = below
            } else {
                y = caret.y + 22
            }
        }
        let x = visible.midX - size.width / 2
        let clampedX = min(max(x, visible.minX + 8), visible.maxX - size.width - 8)
        let clampedY = min(max(y, visible.minY + 8), visible.maxY - size.height - 8)
        let origin = NSPoint(x: clampedX, y: clampedY)
        lockedOrigin = origin
        panel.setFrame(NSRect(origin: origin, size: size), display: true)
    }
}

/// 胶囊绘制：电平条 + 档位图标 + 最新文本 + 闪动省略号；提示在胶囊下方
final class OverlayContentView: NSView {
    private let hintLabel = NSTextField(labelWithString: "松开上屏 · Esc 取消")
    private let capsule = NSView()
    private let iconView = NSImageView()
    private let textLabel = NSTextField(labelWithString: "")
    // 跟随文字的闪烁下划线：光标一样的东西，比省略号更像「正在录入」
    private let dotsLabel = NSTextField(labelWithString: "_")
    private var bars: [CALayer] = []
    private var timer: Timer?
    private var tick: Double = 0
    private var blinkOn = true
    private var lastBlinkAt: TimeInterval = 0

    var phase: String = "idle" { didSet { needsLayout = true } }
    var level: Float = 0
    var partial: String = "" { didSet { needsLayout = true } }
    var redacted: String? { didSet { needsLayout = true } }

    private var displayText: String {
        if let r = redacted { return r }
        return partial
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true

        // 不再用 NSVisualEffectView：它的颜色不可控（同一 alpha 在浅色壁纸上偏亮，
        // 叠子视图又会把电平条/文字一起压暗）。改成实色半透明胶囊，颜色可预期。
        capsule.wantsLayer = true
        capsule.layer?.cornerRadius = 20
        capsule.layer?.masksToBounds = true
        capsule.layer?.backgroundColor = NSColor(srgbRed: 0.22, green: 0.22, blue: 0.24, alpha: 0.94).cgColor
        capsule.layer?.borderWidth = 0.5
        capsule.layer?.borderColor = NSColor.white.withAlphaComponent(0.14).cgColor
        addSubview(capsule)

        hintLabel.font = .systemFont(ofSize: 10)
        hintLabel.textColor = .tertiaryLabelColor
        hintLabel.alignment = .center
        addSubview(hintLabel)

        iconView.imageScaling = .scaleProportionallyDown
        iconView.contentTintColor = .white.withAlphaComponent(0.95)
        capsule.addSubview(iconView)

        textLabel.font = .systemFont(ofSize: 14)
        textLabel.textColor = .white
        textLabel.lineBreakMode = .byClipping
        textLabel.maximumNumberOfLines = 1
        textLabel.cell?.truncatesLastVisibleLine = true
        textLabel.cell?.wraps = false
        capsule.addSubview(textLabel)

        dotsLabel.font = .systemFont(ofSize: 15, weight: .semibold)
        dotsLabel.textColor = .white.withAlphaComponent(0.9)
        capsule.addSubview(dotsLabel)

        for _ in 0..<5 {
            let bar = CALayer()
            bar.cornerRadius = 1.75
            bar.backgroundColor = NSColor(srgbRed: 1.0, green: 0.82, blue: 0.50, alpha: 1.0).cgColor
            capsule.layer?.addSublayer(bar)
            bars.append(bar)
        }
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    func resetWidth() {}

    func setProfile(icon: String, name: String) {
        let cfg = NSImage.SymbolConfiguration(pointSize: 14, weight: .semibold)
        iconView.image = NSImage(systemSymbolName: icon.isEmpty ? "sparkles" : icon, accessibilityDescription: name)?
            .withSymbolConfiguration(cfg)
        needsLayout = true
    }

    override func layout() {
        super.layout()
        CATransaction.begin()
        CATransaction.setDisableActions(true)

        let capsuleH: CGFloat = 40
        let hintH: CGFloat = 12
        let gap: CGFloat = 4
        capsule.frame = CGRect(x: 0, y: hintH + gap, width: bounds.width, height: capsuleH)
        capsule.layer?.cornerRadius = capsuleH / 2
        hintLabel.frame = CGRect(x: 0, y: 0, width: bounds.width, height: hintH)

        let barW: CGFloat = 3.5
        let barGap: CGFloat = 2.5
        let left: CGFloat = 12
        for (i, bar) in bars.enumerated() {
            bar.frame = CGRect(x: left + CGFloat(i) * (barW + barGap), y: (capsuleH - 24) / 2, width: barW, height: 24)
        }
        let iconX = left + CGFloat(bars.count) * (barW + barGap) + 9
        let iconSize: CGFloat = 18
        iconView.frame = CGRect(x: iconX, y: (capsuleH - iconSize) / 2, width: iconSize, height: iconSize)

        let dotsW = textWidth("_") + 3
        let right: CGFloat = 14
        let textX = iconView.frame.maxX + 8
        let avail = max(20, bounds.width - textX - dotsW - right)
        textLabel.font = .systemFont(ofSize: 14)

        // 没有文字时：胶囊保持最小宽度，「……」居中
        // 有文字时：文字 + 「……」紧跟其后；超宽就固定宽度、只留最近的尾部
        if displayText.isEmpty {
            textLabel.stringValue = ""
            textLabel.frame = CGRect(x: textX, y: (capsuleH - 19) / 2, width: 0, height: 19)
            dotsLabel.frame = CGRect(x: (bounds.width - dotsW) / 2, y: (capsuleH - 19) / 2 - 1, width: dotsW, height: 19)
        } else {
            let fitted = fitTail(displayText, width: avail)
            textLabel.stringValue = fitted
            textLabel.textColor = .white
            // 用标签自己的测量宽度，确保「……」紧贴最后一个字，中间不留缝
            let drawn = textLabel.intrinsicContentSize.width
            let fittedW = min(avail, drawn + 2)
            textLabel.frame = CGRect(x: textX, y: (capsuleH - 19) / 2, width: fittedW, height: 19)
            dotsLabel.frame = CGRect(x: textX + fittedW + 1, y: (capsuleH - 19) / 2 - 1, width: dotsW, height: 19)
        }
        if ProcessInfo.processInfo.environment["ERYU_OVERLAY_DEBUG"] != nil {
            NSLog("[ov2] full=\(displayText.count)字 fitted=\(textLabel.stringValue.count)字 avail=\(avail) label=\(self.textLabel.stringValue.prefix(14))")
        }

        CATransaction.commit()
        updateBars()
        if ProcessInfo.processInfo.environment["ERYU_OVERLAY_DEBUG"] != nil {
            let hs = self.bars.map { Int($0.frame.height) }
            NSLog("[ov] bars=\(hs) levels=\(self.level) bounds=\(self.bounds.size) capsule=\(self.capsule.frame) hint=\(self.hintLabel.frame) text='\(self.textLabel.stringValue)' textFrame=\(self.textLabel.frame) dots=\(self.dotsLabel.frame)")
        }
    }

    private static let textFont = NSFont.systemFont(ofSize: 14)

    private func textWidth(_ s: String) -> CGFloat {
        (s as NSString).size(withAttributes: [.font: Self.textFont]).width
    }

    /// 超宽时只留**最近的尾部**（前面的内容往前推走）；不额外加前缀省略号，
    /// 因为「……」本来就紧跟在这段文字后面，表示还在继续
    private func fitTail(_ s: String, width: CGFloat) -> String {
        if s.isEmpty { return s }
        if textWidth(s) <= width { return s }
        var chars = Array(s)
        var keep = chars.count
        while keep > 1 {
            let candidate = String(chars.suffix(keep))
            if textWidth(candidate) <= width { return candidate }
            keep -= 1
        }
        return String(chars.suffix(1))
    }

    func startAnimating() {
        guard timer == nil else { return }
        let t = Timer(timeInterval: 1.0 / 30.0, repeats: true) { [weak self] _ in
            guard let self else { return }
            self.tick += 1.0 / 30.0
            let now = Date().timeIntervalSinceReferenceDate
            if now - self.lastBlinkAt > 0.5 {
                self.lastBlinkAt = now
                self.blinkOn.toggle()
            }
            self.updateBars()
            self.updateDots()
        }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    func stopAnimating() {
        timer?.invalidate()
        timer = nil
    }

    private func updateDots() {
        let waiting = phase == "listening" || phase == "starting"
        dotsLabel.isHidden = !waiting
        guard waiting else { return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        dotsLabel.alphaValue = blinkOn ? 1.0 : 0.25
        CATransaction.commit()
    }

    private func updateBars() {
        let active = phase == "listening" || phase == "starting"
        // 声音大小 → 幅度：按 dB 映射（-60 dB 静音 → 0，-20 dB 以上 → 1）。
        // 之前用 level*9，说话小声时几乎看不出差别（用户反馈「看不出有没有收到音」）。
        let db = 20.0 * log10(max(Double(level), 1e-5))
        let amp = max(0.0, min(1.0, (db + 60.0) / 40.0))
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for (i, bar) in bars.enumerated() {
            let wave = active ? (sin(tick * 6.0 + Double(i) * 0.9) + 1) / 2 : 0.18
            // 始终在动（让人知道在听），但幅度跟着真实音量走：静音时只有很小的呼吸，
            // 说话时明显起伏。这样既能看出「在录音」，也能看出「有没有真的收到声音」。
            let energy = active ? 0.14 + 0.86 * amp : 0.0
            let base = active ? energy * (0.35 + 0.65 * wave) : 0.12
            let h = active ? min(20.0, max(8.0, 6.0 + 14.0 * base)) : 8.0
            bar.frame = CGRect(x: bar.frame.minX, y: (40.0 - h) / 2, width: 3.5, height: h)
            bar.opacity = Float(active ? 0.55 + 0.45 * amp : 0.35)
        }
        CATransaction.commit()
        if ProcessInfo.processInfo.environment["ERYU_OVERLAY_DEBUG"] != nil, Int(tick * 30) % 15 == 0 {
            NSLog("[ov3] level=\(level) bars=\(bars.map { Int($0.frame.height) })")
        }
    }
}
