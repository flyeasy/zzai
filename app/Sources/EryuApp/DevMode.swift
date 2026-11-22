import Foundation

/// 开发版开关：`open -a Eryu --args --dev` 或 `ERYU_DEV=1`。
/// 只影响「界面里要不要露出自测/排障入口」，不改变产品行为。
enum DevMode {
    static var on: Bool {
        CommandLine.arguments.contains("--dev") || ProcessInfo.processInfo.environment["ERYU_DEV"] == "1"
    }
}
