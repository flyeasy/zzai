import AppKit

// 进程启动时刻（main 的第一行，尽量贴近 exec）：给 --baseline 量「冷启动到引擎就绪」
let eryuProcessStart = Date()

// 无 storyboard、无 nib：纯代码 AppKit 菜单栏应用（D-004）
let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
