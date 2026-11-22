import AppKit

/// 录音时暂停其它播放器（DESIGN §10.2「录音静音其他应用」）
///
/// 只用 AppleScript，不依赖私有框架、不需要额外权限：
/// 支持 Music 与 Spotify；只在「当时真的在播」时才暂停，松手后只恢复我们自己暂停的那些。
final class MediaPause {
    private struct Target {
        let app: String
        let resumeScript: String
    }

    private var paused: [Target] = []

    private static let candidates: [(app: String, playing: String, pause: String, resume: String)] = [
        ("Music", "tell application \"Music\" to player state as string", "tell application \"Music\" to pause", "tell application \"Music\" to play"),
        ("Spotify", "tell application \"Spotify\" to player state as string", "tell application \"Spotify\" to pause", "tell application \"Spotify\" to play"),
    ]

    var isPaused: Bool { !paused.isEmpty }

    /// 录开始：把正在播放的播放器暂停，记下来
    func pauseIfPlaying() {
        guard paused.isEmpty else { return }
        for c in Self.candidates {
            guard isRunning(c.app) else { continue }
            let state = run(c.playing)?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() ?? ""
            guard state.contains("playing") else { continue }
            if run(c.pause) != nil {
                paused.append(Target(app: c.app, resumeScript: c.resume))
            }
        }
        if !paused.isEmpty {
            NSLog("[eryu] 已暂停：%@", paused.map(\.app).joined(separator: "、"))
        }
    }

    /// 录结束：只恢复我们暂停过的
    func resumeIfPaused() {
        guard !paused.isEmpty else { return }
        for t in paused {
            _ = run(t.resumeScript)
        }
        NSLog("[eryu] 已恢复：%@", paused.map(\.app).joined(separator: "、"))
        paused = []
    }

    private func isRunning(_ app: String) -> Bool {
        !NSRunningApplication.runningApplications(withBundleIdentifier: bundleID(app)).isEmpty
    }

    private func bundleID(_ app: String) -> String {
        switch app {
        case "Music": return "com.apple.Music"
        case "Spotify": return "com.spotify.client"
        default: return app
        }
    }

    @discardableResult
    private func run(_ source: String) -> String? {
        var error: NSDictionary?
        guard let script = NSAppleScript(source: source) else { return nil }
        let out = script.executeAndReturnError(&error)
        if let error {
            NSLog("[eryu] AppleScript 失败：%@", error)
            return nil
        }
        return out.stringValue
    }
}
