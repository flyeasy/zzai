import Foundation
import ServiceManagement

/// 开机启动（macOS 13+ SMAppService）。ad-hoc 签名的包可能注册失败，失败要如实告诉用户。
enum LoginItem {
    static func isEnabled() -> Bool {
        SMAppService.mainApp.status == .enabled
    }

    @discardableResult
    static func setEnabled(_ on: Bool) -> Bool {
        do {
            if on {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
            return true
        } catch {
            NSLog("Eryu: 开机启动设置失败 %@", String(describing: error))
            return false
        }
    }
}
