import AppKit
import Carbon.HIToolbox

/// 润色档的快捷键槽（DESIGN §9.6「风格快捷键槽 ≤3」）
///
/// 只有组合键能当槽位：单键会和「按住说话」抢（按住 Option 时再按 1，语义会打架），
/// 所以槽位一律要求带修饰键，默认 ⌃⌥1 / ⌃⌥2 / ⌃⌥3。
final class ProfileHotkeys {
    private struct Slot {
        let name: String
        var ref: EventHotKeyRef?
    }

    private var slots: [Slot] = []
    private var handlerRef: EventHandlerRef?
    /// 按下某个槽时通知壳（切档 + 提示）
    var onTrigger: ((String) -> Void)?

    fileprivate static let signature: OSType = 0x4552_5953 // 'ERYS'

    deinit { unregister() }

    /// 用 profile 列表重建槽位；返回注册失败说明（可用于设置页提示）
    @discardableResult
    func apply(profiles: [(name: String, slot: String)]) -> [String] {
        unregister()
        var problems: [String] = []
        let wanted = profiles.filter { !$0.slot.trimmingCharacters(in: .whitespaces).isEmpty }
        guard !wanted.isEmpty else { return [] }

        var spec = [EventTypeSpec(eventClass: OSType(kEventClassKeyboard),
                                  eventKind: UInt32(kEventHotKeyPressed))]
        let ctx = Unmanaged.passUnretained(self).toOpaque()
        let installed = InstallEventHandler(GetApplicationEventTarget(), profileHotkeyHandler,
                                            spec.count, &spec, ctx, &handlerRef)
        guard installed == noErr else {
            return ["无法安装档位热键处理器（\(installed)）"]
        }

        for (index, p) in wanted.enumerated() {
            guard case let .combo(keyCode, modifiers)? = HotkeySpec.parseBinding(p.slot) else {
                problems.append("「\(p.name)」的槽位 \(p.slot) 不是带修饰键的组合，已跳过")
                continue
            }
            var ref: EventHotKeyRef?
            let id = EventHotKeyID(signature: ProfileHotkeys.signature, id: UInt32(index + 1))
            let status = RegisterEventHotKey(keyCode, modifiers, id, GetApplicationEventTarget(), 0, &ref)
            if status == noErr, let ref {
                slots.append(Slot(name: p.name, ref: ref))
            } else {
                problems.append("「\(p.name)」的槽位 \(p.slot) 注册失败：\(Hotkey.describe(status, keyCode: keyCode, modifiers: modifiers))")
            }
        }
        return problems
    }

    func unregister() {
        for slot in slots {
            if let ref = slot.ref { UnregisterEventHotKey(ref) }
        }
        slots.removeAll()
        if let h = handlerRef { RemoveEventHandler(h) }
        handlerRef = nil
    }

    /// 兼容旧签名（设置在保存后直接喂 profile 数组）
    @discardableResult
    func apply(profiles: [PolishProfile]) -> [String] {
        apply(profiles: profiles.map { (name: $0.name, slot: $0.slot) })
    }

    fileprivate func fire(_ id: UInt32) {
        let index = Int(id) - 1
        guard index >= 0, index < slots.count else { return }
        onTrigger?(slots[index].name)
    }

    /// 自测用：直接触发第 id 个槽（1 起），验证「槽 → 档名 → 切档」这条线
    func debugFire(id: UInt32) { fire(id) }

    /// 当前生效的槽位（设置页/诊断显示用）
    var activeSlots: [String] { slots.map(\.name) }
}

private func profileHotkeyHandler(_ callRef: EventHandlerCallRef?,
                                  _ event: EventRef?,
                                  _ userData: UnsafeMutableRawPointer?) -> OSStatus {
    guard let event, let userData else { return OSStatus(eventNotHandledErr) }
    let manager = Unmanaged<ProfileHotkeys>.fromOpaque(userData).takeUnretainedValue()
    guard GetEventKind(event) == UInt32(kEventHotKeyPressed) else {
        return OSStatus(eventNotHandledErr)
    }
    var id = EventHotKeyID()
    guard GetEventParameter(event, EventParamName(kEventParamDirectObject),
                            EventParamType(typeEventHotKeyID), nil,
                            MemoryLayout<EventHotKeyID>.size, nil, &id) == noErr,
          id.signature == ProfileHotkeys.signature else {
        return OSStatus(eventNotHandledErr)
    }
    manager.fire(id.id)
    return noErr
}
