import AppKit
import CoreGraphics

/// 壳侧注入器（DESIGN §4.1：core 发 submit_request，壳串行执行并回报 ok/undelivered/unknown）
///
/// 首版只做 paste 模式：
///  1. 先检查辅助功能/事件投递权限（缺权限 → undelivered，不猜）
///  2. 快照剪贴板（所有 item、所有 type；有读不到的懒加载类型 → 放弃恢复）
///  3. 写入文本 → CGEvent ⌘V → 750ms 后按 changeCount 双校验决定是否恢复
///  4. 任何不确定都回报 unknown，绝不重试注入
final class Injector {
    static let restoreDelay: TimeInterval = 0.75
    /// 粘贴后多久去读输入框（给小应用留出处理时间）
    static let verifyDelay: TimeInterval = 0.35
    /// 自测开关：把校验结果打印出来
    var logVerification = false

    enum Result {
        case ok
        case undelivered(String)
        case unknown(String)
    }

    /// 剪贴板快照；unreadable 为 true 表示有类型读不到，恢复不可保真
    struct Snapshot {
        var items: [[NSPasteboard.PasteboardType: Data]]
        var changeCount: Int
        var unreadable: Bool
    }

    /// 仅自测用：不真的按键
    var dryRun = false
    /// 仅自测用：把 ⌘V 直接投给指定进程（不发到全局，避免影响别的应用）
    var targetPid: pid_t?

    /// 按热键时锁定的目标（I-173）：说话中弹别的窗口，松手也不能把字塞进那个新窗口。
    /// 记 pid + 当时的焦点控件；提交前若前台已经换了应用，先把它切回来再粘。
    struct Target {
        let pid: pid_t
        let name: String
        let element: AXUIElement?
    }

    /// 按下热键那一刻调用：记下前台应用与焦点控件
    static func captureTarget() -> Target? {
        guard let app = NSWorkspace.shared.frontmostApplication else { return nil }
        // 自己是前台（比如在设置里自测）时不锁定，按老路走
        if app.processIdentifier == ProcessInfo.processInfo.processIdentifier { return nil }
        return Target(
            pid: app.processIdentifier,
            name: app.localizedName ?? "?",
            element: AX.focusedElement()
        )
    }

    /// 把前台切回锁定的应用；成功返回 true（最多等 0.6s）
    static func restore(_ target: Target, done: @escaping (Bool) -> Void) {
        guard let app = NSRunningApplication(processIdentifier: target.pid) else {
            done(false)
            return
        }
        if let el = target.element {
            // 同一个应用里用户点了别的框：尽量把焦点还给说话时那个控件
            _ = AXUIElementSetAttributeValue(el, kAXFocusedAttribute as CFString, kCFBooleanTrue)
        }
        app.activate(options: [.activateIgnoringOtherApps])
        var waited = 0.0
        func poll() {
            if NSWorkspace.shared.frontmostApplication?.processIdentifier == target.pid {
                done(true)
                return
            }
            if waited >= 0.6 {
                done(false)
                return
            }
            waited += 0.05
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { poll() }
        }
        poll()
    }

    /// 上屏入口：`mode` 来自 core 的 submit_request（M1b 只有 paste；
    /// keystroke 在 M3 实现，这里明确回报未送达而不是假装成功）
    func inject(
        text: String,
        mode: String = "paste",
        target: Target? = nil,
        completion: @escaping (Result) -> Void
    ) {
        let body = { [weak self] in
            guard let self else { return }
            self.injectNow(text: text, mode: mode, completion: completion)
        }
        // 说话期间前台换了应用：先切回原应用；切不回来就明确回报，别把文字塞进别人窗口
        guard let target, NSWorkspace.shared.frontmostApplication?.processIdentifier != target.pid else {
            body()
            return
        }
        NSLog("[eryu][inject] 前台已换：说话时 %@(%d)，现在切回去再粘", target.name, target.pid)
        Self.restore(target) { ok in
            guard ok else {
                completion(.undelivered("说话时焦点在「\(target.name)」，后来换成了别的窗口；切不回去了，文字已复制到剪贴板，回到原窗口按 ⌘V 就能贴上"))
                return
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.08) { body() }
        }
    }

    private func injectNow(text: String, mode: String, completion: @escaping (Result) -> Void) {
        // 强制失败：验证「上屏没成功时结果还能取回」这条兜底路径（scripts/fallback.sh）
        if let why = ProcessInfo.processInfo.environment["ERYU_FORCE_UNDELIVERED"] {
            NSLog("[eryu][inject] 强制未送达（测试）：\(why)")
            completion(.undelivered("强制未送达：\(why)"))
            return
        }
        guard !text.isEmpty else {
            completion(.undelivered("没有可上屏的文本"))
            return
        }
        if mode == "keystroke" {
            injectKeystroke(text: text, completion: completion)
            return
        }
        if mode != "paste" {
            completion(.undelivered("不认识的上屏方式「\(mode)」"))
            return
        }
        if dryRun {
            completion(.ok)
            return
        }
        guard AXIsProcessTrusted() else {
            completion(.undelivered("缺少「辅助功能」权限：系统设置 → 隐私与安全性 → 辅助功能，勾选 Eryu 后重试"))
            return
        }
        if #available(macOS 10.15, *), !CGPreflightPostEventAccess() {
            completion(.undelivered("缺少「输入监控/辅助功能」权限，无法发送 ⌘V"))
            return
        }

        let pb = NSPasteboard.general
        let snapshot = capture(pb)
        pb.clearContents()
        let wrote = pb.setString(text, forType: .string)
        guard wrote else {
            completion(.unknown("写入剪贴板失败，文本已保存，可手动复制"))
            return
        }
        let ourChangeCount = pb.changeCount
        // 注入后校验（DESIGN §4.1）：先记住焦点控件，粘贴后读一次它的文本
        let before = AX.focusedElement()

        postPaste { [weak self] posted in
            guard let self else { return }
            guard posted else {
                completion(.unknown("⌘V 未能投递（可能处于安全输入状态），文本已保存，可手动复制"))
                return
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + Self.verifyDelay) {
                let verdict = Self.verify(text: text, element: before)
                completion(verdict)
                // 只有「确认上去了」才把剪贴板还回去。
                // 校验不确定（输入框读不到、只上去一部分、或压根没看到）时**留着这段文字**，
                // 用户至少还能手动 ⌘V 一次——比「剪贴板被还原、文字凭空消失」好。
                var delivered = false
                if case .ok = verdict { delivered = true }
                guard delivered else {
                    NSLog("[eryu][inject] 校验未确认上屏，保留剪贴板里的结果以便手动粘贴")
                    return
                }
                DispatchQueue.main.asyncAfter(deadline: .now() + Self.restoreDelay) {
                    self.restoreIfUntouched(pb, snapshot: snapshot, ourChangeCount: ourChangeCount)
                }
            }
        }
    }

    /// 注入后校验：读到控件文本就判断是否包含上屏内容；读不到（终端/部分自绘控件）视为「无法校验，按已发送处理」。
    /// 不做二次注入——这是终审的硬约束（每轮只提交一次）。
    static func verify(text: String, element: AXUIElement?) -> Result {
        guard let element, let value = AX.value(of: element) else {
            return .ok
        }
        // 大文档里粘贴成功后整篇文本会包含上屏内容
        if value.contains(text) { return .ok }
        // 只粘贴了一部分（被截断）或根本没进去
        let head = String(text.prefix(min(8, text.count)))
        if value.contains(head) {
            return .unknown("只看到一部分文本落到输入框（可能被输入框长度限制截断）")
        }
        return .unknown("已发送粘贴，但输入框里没看到这段文字（可能应用不支持、或焦点已变）。文本已保存，可从菜单「复制上次结果」取回")
    }

    /// 是否有进程开启了「安全输入」（密码框等）；此时合成键盘事件会被系统丢弃
    static var isSecureInputEnabled: Bool {
        let lib = dlopen("/System/Library/Frameworks/Carbon.framework/Carbon", RTLD_LAZY)
        guard let lib, let sym = dlsym(lib, "IsSecureEventInputEnabled") else { return false }
        typealias Fn = @convention(c) () -> Bool
        return unsafeBitCast(sym, to: Fn.self)()
    }

    // MARK: - keystroke 模式（M3）

    /// 逐字打字：不发剪贴板、不动剪贴板历史，适合终端/编辑器/剪贴板被限制的应用
    func injectKeystroke(text: String, completion: @escaping (Result) -> Void) {
        guard !text.isEmpty else {
            completion(.undelivered("没有可上屏的文本"))
            return
        }
        if dryRun {
            completion(.ok)
            return
        }
        guard AXIsProcessTrusted() else {
            completion(.undelivered("缺少「辅助功能」权限：系统设置 → 隐私与安全性 → 辅助功能，勾选 Eryu 后重试"))
            return
        }
        if #available(macOS 10.15, *), !CGPreflightPostEventAccess() {
            completion(.undelivered("缺少「输入监控/辅助功能」权限，无法打字"))
            return
        }
        // 安全输入（密码框）下合成事件会被系统丢弃，明确回报而不是静默失败
        if Self.isSecureInputEnabled {
            completion(.undelivered("当前应用处于安全输入状态（可能在输密码），无法用打字方式上屏；文本已保存，可手动复制"))
            return
        }
        guard let src = CGEventSource(stateID: .combinedSessionState) else {
            completion(.unknown("无法创建事件源"))
            return
        }

        let units = Array(text.utf16)
        let batch = 16
        var index = 0
        while index < units.count {
            let end = min(index + batch, units.count)
            var chunk = Array(units[index..<end])
            // 不能把代理对（emoji）劈开
            if end < units.count, let last = chunk.last, (0xD800...0xDBFF).contains(Int(last)) {
                chunk.append(units[end])
                index += 1
            }
            guard let down = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: true),
                  let up = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: false) else {
                completion(.unknown("无法创建键盘事件"))
                return
            }
            chunk.withUnsafeBufferPointer { buf in
                down.keyboardSetUnicodeString(stringLength: chunk.count, unicodeString: buf.baseAddress)
                up.keyboardSetUnicodeString(stringLength: 0, unicodeString: nil)
            }
            down.post(tap: .cghidEventTap)
            up.post(tap: .cghidEventTap)
            usleep(6_000)
            index += chunk.count
        }
        completion(.ok)
    }

    // MARK: - 剪贴板

    private func capture(_ pb: NSPasteboard) -> Snapshot {
        var items: [[NSPasteboard.PasteboardType: Data]] = []
        var unreadable = false
        for item in pb.pasteboardItems ?? [] {
            var entry: [NSPasteboard.PasteboardType: Data] = [:]
            for type in item.types {
                if let data = item.data(forType: type) {
                    entry[type] = data
                } else {
                    unreadable = true
                }
            }
            items.append(entry)
        }
        return Snapshot(items: items, changeCount: pb.changeCount, unreadable: unreadable)
    }

    /// 双校验：changeCount 必须仍等于我们自己写入后的值，且快照可读，否则放弃恢复
    private func restoreIfUntouched(_ pb: NSPasteboard, snapshot: Snapshot, ourChangeCount: Int) {
        guard !snapshot.unreadable else { return }
        guard pb.changeCount == ourChangeCount else { return } // 别人动过剪贴板：让位，不覆盖
        guard !snapshot.items.isEmpty else {
            pb.clearContents()
            return
        }
        pb.clearContents()
        let restored = snapshot.items.map { entry -> NSPasteboardItem in
            let item = NSPasteboardItem()
            for (type, data) in entry {
                item.setData(data, forType: type)
            }
            return item
        }
        pb.writeObjects(restored)
    }

    // MARK: - 按键

    private func postPaste(completion: @escaping (Bool) -> Void) {
        let source = CGEventSource(stateID: .combinedSessionState)
        guard
            let down = CGEvent(keyboardEventSource: source, virtualKey: 0x09, keyDown: true),
            let up = CGEvent(keyboardEventSource: source, virtualKey: 0x09, keyDown: false)
        else {
            completion(false)
            return
        }
        down.flags = .maskCommand
        up.flags = .maskCommand
        if let pid = targetPid {
            down.postToPid(pid)
        } else {
            down.post(tap: .cghidEventTap)
        }
        // 给前台应用一点时间接收粘贴（首版 40ms；如果实测丢字再调）
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.04) {
            if let pid = self.targetPid {
                up.postToPid(pid)
            } else {
                up.post(tap: .cghidEventTap)
            }
            completion(true)
        }
    }
}
