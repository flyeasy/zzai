import AVFoundation
import AppKit
import CoreGraphics

/// 权限体检（DESIGN §5 M1c「权限丢失检测 + 指名提示」、§5.1 权限矩阵）
///
/// 三件事分开查，因为它们的系统开关不在同一个地方：
///  - 麦克风：系统设置 → 隐私与安全性 → 麦克风
///  - 辅助功能（AXIsProcessTrusted）：决定能不能读到前台控件/做焦点判定
///  - 输入监控（CGPreflightListenEventAccess / CGPreflightPostEventAccess）：决定能不能合成 ⌘V
enum Permission: String, CaseIterable {
    case microphone
    case accessibility
    case inputMonitoring

    var title: String {
        switch self {
        case .microphone: return "麦克风"
        case .accessibility: return "辅助功能"
        case .inputMonitoring: return "输入监控"
        }
    }

    /// 这个权限不全会怎样（直接说后果，不喊口号）
    var consequence: String {
        switch self {
        case .microphone: return "收不到声音，按快捷键会提示「麦克风没有收到声音」"
        case .accessibility: return "能识别但没法粘贴，文字只会存进历史等你自己复制"
        case .inputMonitoring: return "合成的 ⌘V 可能投递不出去（安全输入场景尤其明显）"
        }
    }

    var settingsURL: String {
        switch self {
        case .microphone:
            return "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        case .accessibility:
            return "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        case .inputMonitoring:
            return "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
        }
    }

    enum State {
        case granted
        case denied
        case notAsked
        case unknown

        var text: String {
            switch self {
            case .granted: return "已授权"
            case .denied: return "被拒绝"
            case .notAsked: return "还没请求"
            case .unknown: return "未知"
            }
        }

        var ok: Bool { self == .granted }
    }

    var state: State {
        switch self {
        case .microphone:
            switch AVCaptureDevice.authorizationStatus(for: .audio) {
            case .authorized: return .granted
            case .denied, .restricted: return .denied
            case .notDetermined: return .notAsked
            @unknown default: return .unknown
            }
        case .accessibility:
            return AXIsProcessTrusted() ? .granted : .denied
        case .inputMonitoring:
            // 两个探针：能监听（listen）与能合成（post）；任一为 false 都算没给全
            let listen = CGPreflightListenEventAccess()
            let post = CGPreflightPostEventAccess()
            if listen && post { return .granted }
            return .denied
        }
    }

    /// 一次性把所有系统弹窗都触发出来（麦克风弹窗、辅助功能弹窗、输入监控弹窗），
    /// 用户点「允许 / 打开系统设置」即可；TCC 不允许程序自己绕过，这是系统强制的。
    static func requestAll(openPanes: Bool = true) {
        AVCaptureDevice.requestAccess(for: .audio) { _ in }
        // 带 prompt 的版本会弹「Eryu 想控制这台电脑」并把开关列出来
        let opts = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        _ = AXIsProcessTrustedWithOptions(opts)
        // 输入监控：10.15+ 会弹「Eryu 想监视你的输入」
        _ = CGRequestListenEventAccess()
        _ = CGRequestPostEventAccess()
        if openPanes {
            for p in Permission.allCases where !p.state.ok {
                openSettings(p)
            }
        }
    }

    /// 请求权限（目前只有麦克风支持系统弹窗；AX/输入监控必须用户自己去设置里点）
    static func requestMicrophone(_ done: @escaping () -> Void) {
        AVCaptureDevice.requestAccess(for: .audio) { _ in
            DispatchQueue.main.async { done() }
        }
    }

    static func openSettings(_ p: Permission) {
        if let url = URL(string: p.settingsURL) { NSWorkspace.shared.open(url) }
    }

    /// 启动时一句话指名缺了什么（不要一次索要全部，也不要笼统说「权限不足」）
    static func missingSummary() -> String? {
        let missing = Permission.allCases.filter { !$0.state.ok }
        guard !missing.isEmpty else { return nil }
        let names = missing.map { $0.title }.joined(separator: "、")
        return "缺少权限：\(names)。设置 → 诊断 里有逐项说明和跳转按钮"
    }
}

extension Permission {
    /// 用户点「重新授权」时走这条：确认 → 清掉旧副本记录 → 重新请求
    static func reauthorizeWithConfirm() {
        let alert = NSAlert()
        alert.messageText = "重新授权辅助功能 / 输入监控？"
        alert.informativeText = """
        会清掉系统里记着的旧 Eryu 授权（那些指向已经删掉的副本），然后重新弹出授权窗口。
        点「重新授权」后：① 允许「控制这台电脑」② 在「输入监控」里勾上 字在（系统设置 → 隐私与安全性）。
        麦克风不受影响。
        """
        alert.addButton(withTitle: "重新授权")
        alert.addButton(withTitle: "取消")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        resetStale()
        requestAll()
    }

    /// 清掉旧副本留下的授权记录（辅助功能 / 输入监控按路径记，旧副本删掉后记录还在）
    static func resetStale() {
        for service in ["Accessibility", "ListenEvent"] {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/usr/bin/tccutil")
            p.arguments = ["reset", service, "com.eryu.app"]
            try? p.run()
            p.waitUntilExit()
        }
    }
}

// MARK: - 权限报告（自己看真相，不猜）

/// 一次完整的权限检查结果：三项状态 + 真能做成什么。
/// 写 `/tmp/eryu-perm.json`：用户说「明明给了权限」时，这份报告能区分
/// 「系统真的没给」「给的是另一个副本」「给了但合成事件仍失败」。
enum PermReport {
    static func json() -> [String: Any] {
        var out: [String: Any] = [:]
        let bundle = Bundle.main
        out["path"] = bundle.bundlePath
        out["bundle_id"] = bundle.bundleIdentifier ?? ""
        out["version"] = bundle.infoDictionary?["CFBundleShortVersionString"] as? String ?? ""
        out["pid"] = Int(ProcessInfo.processInfo.processIdentifier)
        out["launched_by_launchservices"] = !(ProcessInfo.processInfo.environment["__CFBundleIdentifier"] == nil)
        var perms: [String: Any] = [:]
        for p in Permission.allCases {
            perms[p.rawValue] = p.state.text
        }
        out["permissions"] = perms
        out["missing"] = Permission.missingSummary() ?? ""
        // AX 真能读到东西吗（授权了但读不到 = 没生效）
        if let el = AX.focusedElement() {
            var raw: CFTypeRef?
            let err = AXUIElementCopyAttributeValue(el, "AXRole" as CFString, &raw)
            if err == .success, let role = raw as? String {
                out["ax_probe"] = "ok:\(role)"
            } else {
                out["ax_probe"] = "读到焦点控件但取不到 AXRole（err=\(err.rawValue)）"
            }
        } else {
            out["ax_probe"] = "读不到焦点控件（没授权 / 前台 App 是终端或自绘控件）"
        }
        // 能不能真的装一个 listen-only 的 tap（输入监控真生效的标志）
        let mask = (1 << CGEventType.keyDown.rawValue) | (1 << CGEventType.flagsChanged.rawValue)
        let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap, place: .headInsertEventTap, options: .listenOnly,
            eventsOfInterest: CGEventMask(mask), callback: { _, _, event, _ in Unmanaged.passUnretained(event) },
            userInfo: nil
        )
        if let tap {
            out["event_tap"] = "ok"
            CFMachPortInvalidate(tap)
        } else {
            out["event_tap"] = "装不上（需要「输入监控」；单键热键、Esc 取消都靠它）"
        }
        // 能不能合成一次事件（辅助功能真生效的标志）：不真发，只 preflight + 建事件
        let src = CGEventSource(stateID: .combinedSessionState)
        let canPost = CGPreflightPostEventAccess() && src != nil
        out["can_post_events"] = canPost
        return out
    }

    @discardableResult
    static func write(to path: String = "/tmp/eryu-perm.json") -> String {
        let dict = json()
        if let data = try? JSONSerialization.data(withJSONObject: dict, options: [.prettyPrinted, .sortedKeys]) {
            try? data.write(to: URL(fileURLWithPath: path))
        }
        return (try? String(contentsOfFile: path, encoding: .utf8)) ?? "{}"
    }
}
