import AppKit
import ApplicationServices

/// 注入后编辑追踪（DESIGN §10.2「热词自动学习」）
///
/// 上屏成功后盯住那个输入框十几秒：如果用户把某个词改掉了，就把「原词 → 新词」当成一条待采纳的热词候选。
/// 只在辅助功能可读、且隐私模式关闭时工作；不落盘、不上传。
final class EditObserver {
    /// 一次候选：(听错/写错的原词, 用户改成的新词)
    struct Suggestion: Identifiable, Equatable, Codable {
        let from: String
        let to: String
        var id: String { from + "\u{1}" + to }
    }

    /// 盯多久（秒）
    private let window: TimeInterval = 15
    private let interval: TimeInterval = 1.0
    private var element: AXUIElement?
    private var expected: String = ""
    private var timer: Timer?
    private var startedAt: Date?

    var onSuggestion: ((Suggestion) -> Void)?

    var isRunning: Bool { timer != nil }

    /// 开始观察：`expected` 是刚上屏的文本
    func watch(expected: String, element: AXUIElement?) {
        stop()
        guard let element, !expected.isEmpty else { return }
        self.element = element
        self.expected = expected
        self.startedAt = Date()
        let t = Timer(timeInterval: interval, repeats: true) { [weak self] _ in
            self?.poll()
        }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    func stop() {
        timer?.invalidate()
        timer = nil
        element = nil
        expected = ""
    }

    private func poll() {
        guard let startedAt else { return stop() }
        if Date().timeIntervalSince(startedAt) > window { return stop() }
        guard let element else { return stop() }
        guard let actual = AX.value(of: element) else { return stop() }
        // 还没改过：值里仍然包含上屏文本（用户可能只是点了别处）
        if actual.contains(expected) { return }
        guard let s = Self.diff(expected: expected, actual: actual) else { return stop() }
        stop()
        onSuggestion?(s)
    }

    /// 找出「原词 → 新词」：取公共前后缀，中间那段就是改动
    static func diff(expected: String, actual: String) -> Suggestion? {
        let a = Array(expected)
        let b = Array(actual)
        // 长度差太大往往不是改词，是整体换掉了内容（或被别的插入影响）
        guard !b.isEmpty else { return nil }
        let ratio = Double(b.count) / Double(max(a.count, 1))
        guard ratio > 0.6, ratio < 1.6 else { return nil }

        var prefix = 0
        while prefix < a.count, prefix < b.count, a[prefix] == b[prefix] { prefix += 1 }
        var suffix = 0
        while suffix < a.count - prefix, suffix < b.count - prefix,
              a[a.count - 1 - suffix] == b[b.count - 1 - suffix] { suffix += 1 }

        let from = String(a[prefix..<(a.count - suffix)])
        let to = String(b[prefix..<(b.count - suffix)])
        guard !from.isEmpty, !to.isEmpty, from != to else { return nil }
        // 太小/太大的改动都不像「一个词」：单字差异多半是错别字或光标位置，超长是整句重写
        guard from.count <= 24, to.count <= 24 else { return nil }
        guard from.count >= 2 else { return nil }
        // 纯标点/空白不学
        let meaningful = from.unicodeScalars.contains { CharacterSet.alphanumerics.contains($0) || $0.value > 0x2E80 }
        guard meaningful else { return nil }
        // 已包含关系（"深度求索" → "深度求索AI"）学习价值低
        guard !to.contains(from) else { return nil }
        return Suggestion(from: from, to: to)
    }

    /// 从最近的编辑里补一个候选（供自测与手动触发）
    func proposeNow(actual: String) {
        guard let s = Self.diff(expected: expected, actual: actual) else { return }
        stop()
        onSuggestion?(s)
    }
}

extension AX {
    /// 读控件当前文本（NSTextView / NSTextField / WebKit 常见情况）
    static func value(of element: AXUIElement) -> String? {
        var raw: CFTypeRef?
        if AXUIElementCopyAttributeValue(element, kAXValueAttribute as CFString, &raw) == .success,
           let s = raw as? String {
            return s
        }
        return nil
    }
}
