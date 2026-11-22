import AppKit
import CoreGraphics

/// 单键按住说话（用户 m01947 / m02028：右 Option 只是例子，任何单键都该能当键）
///
/// Carbon 的 `RegisterEventHotKey` 既不接受纯修饰键（paramErr），也不接受「仅字母」这种
/// 会和打字抢的裸键，所以单键一律走 `CGEventTap`（`.listenOnly`，只旁听不拦截）：
///  - 修饰键（右 Option / Fn…）听 `flagsChanged`
///  - 其它键（F13 / 分号 / 字母…）听 `keyDown` + `keyUp`，并忽略长按自动重复
///  - 需要「辅助功能」或「输入监控」权限，没有就返回中文原因
///  - 安全输入（密码框）期间系统不投递事件，这是系统行为
final class KeyTap {
    /// 要监听的键码
    var targetKeyCode: CGKeyCode = 61
    /// 目标是不是修饰键（决定听 flagsChanged 还是 keyDown/keyUp）
    var isModifier = true
    var onDown: (() -> Void)?
    var onUp: (() -> Void)?

    private var tap: CFMachPort?
    private var source: CFRunLoopSource?
    private var isDown = false

    var isRunning: Bool { tap != nil }

    /// 返回 nil 表示成功，否则是给用户看的中文原因
    func start() -> String? {
        stop()
        isDown = false
        // 测试用：模拟「没有辅助功能/输入监控权限」时的失败，验证降级路径不会把应用带崩
        if ProcessInfo.processInfo.environment["ERYU_FORCE_TAP_FAIL"] != nil {
            return "单键热键需要「辅助功能」或「输入监控」权限（模拟权限不足）"
        }
        var types: [CGEventType] = [.keyDown, .keyUp]
        if isModifier { types.append(.flagsChanged) }
        let mask = types.reduce(CGEventMask(0)) { $0 | (CGEventMask(1) << $1.rawValue) }
        guard let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .listenOnly,
            eventsOfInterest: mask,
            callback: { _, type, event, userInfo in
                guard let userInfo else { return Unmanaged.passUnretained(event) }
                let monitor = Unmanaged<KeyTap>.fromOpaque(userInfo).takeUnretainedValue()
                monitor.handle(type: type, event: event)
                return Unmanaged.passUnretained(event)
            },
            userInfo: Unmanaged.passUnretained(self).toOpaque()
        ) else {
            return "单键热键需要「辅助功能」或「输入监控」权限：系统设置 → 隐私与安全性 → 辅助功能，勾选 Eryu 后重开应用"
        }
        guard let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0) else {
            return "无法为键盘监听创建事件源"
        }
        CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        self.tap = tap
        self.source = source
        return nil
    }

    func stop() {
        if let tap {
            CGEvent.tapEnable(tap: tap, enable: false)
            CFMachPortInvalidate(tap)
        }
        if let source {
            CFRunLoopRemoveSource(CFRunLoopGetMain(), source, .commonModes)
        }
        tap = nil
        source = nil
        isDown = false
    }

    private func handle(type: CGEventType, event: CGEvent) {
        let code = CGKeyCode(event.getIntegerValueField(.keyboardEventKeycode))
        guard code == targetKeyCode else { return }

        if type == .flagsChanged {
            // 同一修饰键左右共用一个 flag 位，按 keyCode 记上下沿
            let flags = event.flags
            let stillDown: Bool
            switch code {
            case 58, 61: stillDown = flags.contains(.maskAlternate)
            case 55, 54: stillDown = flags.contains(.maskCommand)
            case 56, 60: stillDown = flags.contains(.maskShift)
            case 59, 62: stillDown = flags.contains(.maskControl)
            default: stillDown = !isDown
            }
            set(down: stillDown)
            return
        }

        if event.getIntegerValueField(.keyboardEventAutorepeat) != 0 { return }
        set(down: type == .keyDown)
    }

    private func set(down: Bool) {
        guard down != isDown else { return }
        isDown = down
        if down { onDown?() } else { onUp?() }
    }
}
