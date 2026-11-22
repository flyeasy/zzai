import AppKit
import Carbon.HIToolbox

/// 全局热键（D-005：首版用 Carbon RegisterEventHotKey，pressed + released 成对）
///
/// 已知限制（docs/ISSUES.md I-003）：
///  - 安全输入（Secure Event Input，如密码框）期间系统不投递热键
///  - 若某些应用吃掉按键组合，Carbon 仍能收到（系统级注册），但不保证 released
final class Hotkey {
    private var hotKeyRef: EventHotKeyRef?
    private var handlerRef: EventHandlerRef?
    private var registered = false
    /// 任意单键（右 Option / Fn / F13 / 分号…）走 CGEventTap，见 KeyTap.swift
    private let keyTap = KeyTap()
    private var mode: Mode = .none

    private enum Mode { case none, carbon, singleKey }

    var onDown: (() -> Void)?
    var onUp: (() -> Void)?

    private static let signature: OSType = 0x4552_5955 // 'ERYU'

    /// 默认 ⌃⌥Space（避开 Spotlight 的 ⌘Space 与输入法的 ⌃Space）
    static let defaultKeyCode = UInt32(kVK_Space)
    static let defaultModifiers = UInt32(controlKey | optionKey)

    /// 返回 (成功, 失败原因)
    func register(keyCode: UInt32 = Hotkey.defaultKeyCode,
                  modifiers: UInt32 = Hotkey.defaultModifiers) -> (Bool, String?) {
        guard !registered else { return (true, nil) }

        var spec = [
            EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed)),
            EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyReleased)),
        ]
        let ctx = Unmanaged.passUnretained(self).toOpaque()
        let installStatus = InstallEventHandler(
            GetApplicationEventTarget(),
            hotkeyHandler,
            spec.count,
            &spec,
            ctx,
            &handlerRef
        )
        guard installStatus == noErr else {
            return (false, "无法安装热键事件处理器（\(installStatus)）")
        }

        var ref: EventHotKeyRef?
        let id = EventHotKeyID(signature: Hotkey.signature, id: 1)
        let status = RegisterEventHotKey(keyCode, modifiers, id, GetApplicationEventTarget(), 0, &ref)
        guard status == noErr, let ref else {
            return (false, Self.describe(status, keyCode: keyCode, modifiers: modifiers))
        }
        hotKeyRef = ref
        registered = true
        return (true, nil)
    }

    /// 按配置文本注册/重注册。支持组合键（Carbon）与单修饰键（事件监听，如 ALT_R）
    func apply(spec: String) -> (Bool, String?) {
        guard let binding = HotkeySpec.parseBinding(spec) else {
            return (false, "快捷键格式不对：例 CTRL+ALT+SPACE、ALT_R、F13")
        }
        unregister()
        switch binding {
        case let .combo(keyCode, modifiers):
            let (ok, reason) = register(keyCode: keyCode, modifiers: modifiers)
            if ok { mode = .carbon }
            return (ok, reason)
        case let .singleKey(code, isModifier):
            keyTap.targetKeyCode = code
            keyTap.isModifier = isModifier
            keyTap.onDown = { [weak self] in self?.onDown?() }
            keyTap.onUp = { [weak self] in self?.onUp?() }
            if let reason = keyTap.start() {
                keyTap.onDown = nil
                keyTap.onUp = nil
                return (false, reason)
            }
            mode = .singleKey
            let warn = HotkeySpec.singleKeyWarning(code)
            return (true, warn)
        }
    }

    func unregister() {
        if let ref = hotKeyRef { UnregisterEventHotKey(ref) }
        if let h = handlerRef { RemoveEventHandler(h) }
        hotKeyRef = nil
        handlerRef = nil
        registered = false
        if mode == .singleKey {
            keyTap.stop()
            keyTap.onDown = nil
            keyTap.onUp = nil
        }
        mode = .none
    }

    /// 当前热键是否需要额外权限（诊断页用）
    var needsAccessibility: Bool { mode == .singleKey }

    /// 热键注册失败的具体原因（对应 DESIGN §10.1「快捷键注册错误枚举」）
    static func describe(_ status: OSStatus, keyCode: UInt32, modifiers: UInt32) -> String {
        switch status {
        case OSStatus(eventHotKeyExistsErr):
            let hint = (keyCode == UInt32(kVK_Space) && modifiers & UInt32(cmdKey) != 0)
                ? "⌘Space 很可能被「聚焦搜索」占用"
                : "该组合已被其他应用占用"
            return "热键冲突：\(hint)。换一个组合，或在系统设置里关掉占用它的功能。"
        case OSStatus(paramErr):
            return "热键参数无效：需要至少一个修饰键（⌃⌥⌘⇧），不能是纯字母键。"
        default:
            return "热键注册失败（错误码 \(status)）。"
        }
    }
}

private func hotkeyHandler(_ callRef: EventHandlerCallRef?,
                           _ event: EventRef?,
                           _ userData: UnsafeMutableRawPointer?) -> OSStatus {
    guard let event, let userData else { return OSStatus(eventNotHandledErr) }
    let hotkey = Unmanaged<Hotkey>.fromOpaque(userData).takeUnretainedValue()
    switch GetEventKind(event) {
    case UInt32(kEventHotKeyPressed):
        hotkey.onDown?()
        return noErr
    case UInt32(kEventHotKeyReleased):
        hotkey.onUp?()
        return noErr
    default:
        return OSStatus(eventNotHandledErr)
    }
}

/// 快捷键文本 ↔ Carbon 键码（config.toml 里的 `hotkey` 用它读写）
///
/// 语法：修饰键 CTRL/CONTROL、ALT/OPTION、SHIFT、CMD/COMMAND（+ 连接），最后一段是主键。
/// 例：`CTRL+ALT+SPACE`、`CMD+SHIFT+K`、`F6+ALT`。
enum HotkeySpec {
    static let modifierNames: [(String, UInt32)] = [
        ("CTRL", UInt32(controlKey)),
        ("CONTROL", UInt32(controlKey)),
        ("ALT", UInt32(optionKey)),
        ("OPTION", UInt32(optionKey)),
        ("SHIFT", UInt32(shiftKey)),
        ("CMD", UInt32(cmdKey)),
        ("COMMAND", UInt32(cmdKey)),
    ]

    /// 主键名 → 键码（覆盖常用键；不认识的直接写数字键码）
    static let keyNames: [String: UInt32] = [
        "SPACE": UInt32(kVK_Space), "TAB": UInt32(kVK_Tab), "RETURN": UInt32(kVK_Return),
        "ENTER": UInt32(kVK_Return), "ESC": UInt32(kVK_Escape), "ESCAPE": UInt32(kVK_Escape),
        "BACKSPACE": UInt32(kVK_Delete), "DELETE": UInt32(kVK_ForwardDelete),
        "UP": UInt32(kVK_UpArrow), "DOWN": UInt32(kVK_DownArrow),
        "LEFT": UInt32(kVK_LeftArrow), "RIGHT": UInt32(kVK_RightArrow),
        "HOME": UInt32(kVK_Home), "END": UInt32(kVK_End),
        "PAGEUP": UInt32(kVK_PageUp), "PAGEDOWN": UInt32(kVK_PageDown),
        "`": UInt32(kVK_ANSI_Grave), "-": UInt32(kVK_ANSI_Minus), "=": UInt32(kVK_ANSI_Equal),
        "[": UInt32(kVK_ANSI_LeftBracket), "]": UInt32(kVK_ANSI_RightBracket),
        "\\": UInt32(kVK_ANSI_Backslash), ";": UInt32(kVK_ANSI_Semicolon),
        "'": UInt32(kVK_ANSI_Quote), ",": UInt32(kVK_ANSI_Comma), ".": UInt32(kVK_ANSI_Period),
        "/": UInt32(kVK_ANSI_Slash),
        "F1": UInt32(kVK_F1), "F2": UInt32(kVK_F2), "F3": UInt32(kVK_F3), "F4": UInt32(kVK_F4),
        "F5": UInt32(kVK_F5), "F6": UInt32(kVK_F6), "F7": UInt32(kVK_F7), "F8": UInt32(kVK_F8),
        "F9": UInt32(kVK_F9), "F10": UInt32(kVK_F10), "F11": UInt32(kVK_F11), "F12": UInt32(kVK_F12),
        "F13": UInt32(kVK_F13), "F14": UInt32(kVK_F14), "F15": UInt32(kVK_F15), "F16": UInt32(kVK_F16),
        "F17": UInt32(kVK_F17), "F18": UInt32(kVK_F18), "F19": UInt32(kVK_F19), "F20": UInt32(kVK_F20),
    ]

    static let letters: [String: Int] = {
        // kVK_ANSI_A ... 这些常量不是连续排列，逐个列
        [
            "A": kVK_ANSI_A, "B": kVK_ANSI_B, "C": kVK_ANSI_C, "D": kVK_ANSI_D,
            "E": kVK_ANSI_E, "F": kVK_ANSI_F, "G": kVK_ANSI_G, "H": kVK_ANSI_H,
            "I": kVK_ANSI_I, "J": kVK_ANSI_J, "K": kVK_ANSI_K, "L": kVK_ANSI_L,
            "M": kVK_ANSI_M, "N": kVK_ANSI_N, "O": kVK_ANSI_O, "P": kVK_ANSI_P,
            "Q": kVK_ANSI_Q, "R": kVK_ANSI_R, "S": kVK_ANSI_S, "T": kVK_ANSI_T,
            "U": kVK_ANSI_U, "V": kVK_ANSI_V, "W": kVK_ANSI_W, "X": kVK_ANSI_X,
            "Y": kVK_ANSI_Y, "Z": kVK_ANSI_Z,
            "0": kVK_ANSI_0, "1": kVK_ANSI_1, "2": kVK_ANSI_2, "3": kVK_ANSI_3,
            "4": kVK_ANSI_4, "5": kVK_ANSI_5, "6": kVK_ANSI_6, "7": kVK_ANSI_7,
            "8": kVK_ANSI_8, "9": kVK_ANSI_9,
        ].mapValues { Int($0) }
    }()

    /// 单修饰键名 → 键码（这些不能走 Carbon，只能走事件监听）
    static let modifierKeyNames: [(String, CGKeyCode)] = [
        ("ALT_R", 61), ("RIGHT_ALT", 61), ("OPTION_R", 61), ("RIGHT_OPTION", 61),
        ("ALT_L", 58), ("LEFT_ALT", 58), ("OPTION_L", 58), ("LEFT_OPTION", 58),
        ("CMD_R", 54), ("RIGHT_CMD", 54), ("COMMAND_R", 54), ("RIGHT_COMMAND", 54),
        ("CMD_L", 55), ("LEFT_CMD", 55), ("COMMAND_L", 55), ("LEFT_COMMAND", 55),
        ("CTRL_R", 62), ("RIGHT_CTRL", 62), ("CONTROL_R", 62), ("RIGHT_CONTROL", 62),
        ("CTRL_L", 59), ("LEFT_CTRL", 59), ("CONTROL_L", 59), ("LEFT_CONTROL", 59),
        ("SHIFT_R", 60), ("RIGHT_SHIFT", 60),
        ("SHIFT_L", 56), ("LEFT_SHIFT", 56),
        ("FN", 63),
    ]

    /// 单个键的友好显示名（设置页展示/录制回填用）
    static func modifierDisplayName(_ code: CGKeyCode) -> String {
        switch code {
        case 61: return "右 Option"
        case 58: return "左 Option"
        case 54: return "右 Command"
        case 55: return "左 Command"
        case 62: return "右 Control"
        case 59: return "左 Control"
        case 60: return "右 Shift"
        case 56: return "左 Shift"
        case 63: return "Fn"
        default: return "键码 \(code)"
        }
    }

    /// 规范化的写法（存进 config.toml 用 ASCII，便于手改）
    static func modifierToken(_ code: CGKeyCode) -> String {
        switch code {
        case 61: return "ALT_R"
        case 58: return "ALT_L"
        case 54: return "CMD_R"
        case 55: return "CMD_L"
        case 62: return "CTRL_R"
        case 59: return "CTRL_L"
        case 60: return "SHIFT_R"
        case 56: return "SHIFT_L"
        case 63: return "FN"
        default: return "\(code)"
        }
    }

    /// 设置页的快捷选择：(存进配置的写法, 给用户看的名字)
    static let quickPicks: [(String, String)] = [
        ("ALT_R", "右 Option"),
        ("FN", "Fn"),
        ("F13", "F13"),
        ("CTRL+ALT+SPACE", "⌃⌥Space"),
    ]

    enum Binding {
        case combo(keyCode: UInt32, modifiers: UInt32)
        /// 单键：code + 是否修饰键（修饰键听 flagsChanged，其余听 keyDown/keyUp）
        case singleKey(code: CGKeyCode, isModifier: Bool)
    }

    /// 某个键码是不是修饰键
    static func isModifierKeyCode(_ code: CGKeyCode) -> Bool {
        [58, 61, 55, 54, 56, 60, 59, 62, 63].contains(code)
    }

    /// 单键里的「会吞打字」的键：只旁听不拦截，所以按下它仍会输入到当前应用
    static func singleKeyWarning(_ code: CGKeyCode) -> String? {
        if isModifierKeyCode(code) { return nil }
        let printable = letters.contains { CGKeyCode($0.value) == code }
            || keyNames.contains { CGKeyCode($0.value) == code }
        if printable {
            return "这个单键会和打字抢：按住它录音的同时，字符也会输入到当前应用。更稳的是右 Option、Fn 或 F13 这类不产生字符的键。"
        }
        return nil
    }

    /// 解析成「组合键」或「单键」
    static func parseBinding(_ spec: String) -> Binding? {
        let parts = spec.uppercased()
            .split(separator: "+")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
        if parts.count == 1 {
            let token = parts[0]
            if let hit = modifierKeyNames.first(where: { $0.0 == token }) {
                return .singleKey(code: hit.1, isModifier: true)
            }
            if let n = keyNames[token] {
                return .singleKey(code: CGKeyCode(n), isModifier: false)
            }
            if let idx = letters[token] {
                return .singleKey(code: CGKeyCode(idx), isModifier: false)
            }
            if let n = UInt32(token), n <= 200 {
                return .singleKey(code: CGKeyCode(n), isModifier: isModifierKeyCode(CGKeyCode(n)))
            }
            return nil
        }
        guard let combo = parse(spec) else { return nil }
        return .combo(keyCode: combo.keyCode, modifiers: combo.modifiers)
    }

    /// 给界面看的友好名字（「ALT_R」→「右 Option」，「F13」→「F13」，「;」→「；」）
    static func displayName(for spec: String) -> String {
        guard let binding = parseBinding(spec) else { return spec }
        switch binding {
        case let .singleKey(code, isModifier):
            if isModifier { return modifierDisplayName(code) }
            if let hit = keyNames.first(where: { CGKeyCode($0.value) == code }) {
                return hit.key == "SPACE" ? "空格" : hit.key
            }
            if let hit = letters.first(where: { CGKeyCode($0.value) == code }) { return hit.key }
            return "键码 \(code)"
        case let .combo(keyCode, modifiers):
            var parts: [String] = []
            if modifiers & UInt32(controlKey) != 0 { parts.append("⌃") }
            if modifiers & UInt32(optionKey) != 0 { parts.append("⌥") }
            if modifiers & UInt32(shiftKey) != 0 { parts.append("⇧") }
            if modifiers & UInt32(cmdKey) != 0 { parts.append("⌘") }
            parts.append(name(for: keyCode) ?? "键\(keyCode)")
            return parts.joined()
        }
    }

    static func parse(_ spec: String) -> (keyCode: UInt32, modifiers: UInt32)? {
        let parts = spec.uppercased()
            .split(separator: "+")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
        guard let last = parts.last else { return nil }
        var mods: UInt32 = 0
        for p in parts.dropLast() {
            guard let (_, bit) = modifierNames.first(where: { $0.0 == p }) else { return nil }
            mods |= bit
        }
        let key: UInt32?
        if let n = keyNames[last] {
            key = n
        } else if let idx = letters[last] {
            key = UInt32(idx)
        } else if let n = UInt32(last) {
            key = n
        } else {
            key = nil
        }
        guard let keyCode = key else { return nil }
        // 无修饰键只放行功能键：普通字母单键会把打字抢走
        if mods == 0 {
            let fnRange = UInt32(kVK_F1)...UInt32(kVK_F20)
            guard fnRange.contains(keyCode) else { return nil }
        }
        return (keyCode, mods)
    }

    /// 把捕获到的按键事件格式化成 `CTRL+ALT+SPACE`
    static func format(keyCode: UInt32, modifiers: NSEvent.ModifierFlags) -> String? {
        var parts: [String] = []
        if modifiers.contains(.control) { parts.append("CTRL") }
        if modifiers.contains(.option) { parts.append("ALT") }
        if modifiers.contains(.shift) { parts.append("SHIFT") }
        if modifiers.contains(.command) { parts.append("CMD") }
        // 纯修饰键由 KeyRecorder 的 flagsChanged 分支处理
        if parts.isEmpty {
            let fnRange = UInt32(kVK_F1)...UInt32(kVK_F20)
            if fnRange.contains(keyCode), let name = name(for: keyCode) { return name }
            return nil
        }
        guard let name = name(for: keyCode) else { return nil }
        parts.append(name)
        return parts.joined(separator: "+")
    }

    static func name(for keyCode: UInt32) -> String? {
        if let hit = keyNames.first(where: { $0.value == keyCode }) { return hit.key }
        if let hit = letters.first(where: { UInt32($0.value) == keyCode }) { return hit.key }
        return nil
    }
}
