import AppKit
import Combine
import SwiftUI

/// 迷你主窗（R-103，DESIGN §12.3）：170×230，一个麦克风大按钮 + 当前按键提示 + 今日节省 + 设置。
/// 点击只切换「这一颗按钮」的录音，全局按住/点击模式的选择留给 R-110（D-111）。
final class MiniWindowController: NSObject, NSWindowDelegate {
    private let model: AppModel
    private let state = MiniWindowState()
    private var panel: NSPanel?
    private var stateObserver: AnyCancellable?
    private var positioned = false

    var onToggleTalk: (() -> Void)?
    var onSettings: (() -> Void)?
    /// 用户点关闭按钮：隐藏窗口并把 `[ui] mini_window` 记成 false
    var onClose: (() -> Void)?

    init(model: AppModel) {
        self.model = model
        super.init()
        state.hotkeyName = HotkeySpec.displayName(for: model.config.hotkey)
        stateObserver = model.$config
            .receive(on: RunLoop.main)
            .sink { [weak self] cfg in
                self?.state.hotkeyName = HotkeySpec.displayName(for: cfg.hotkey)
            }
    }

    var isVisible: Bool { panel?.isVisible ?? false }

    func present() {
        let p = panel ?? makePanel()
        panel = p
        if !positioned {
            positioned = true
            position(p)
        }
        p.orderFrontRegardless()
    }

    func hide() {
        panel?.orderOut(nil)
    }

    func updatePhase(_ phase: String) {
        state.phase = phase
    }

    // MARK: - 窗口

    private func makePanel() -> NSPanel {
        let p = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 170, height: 230),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        p.isOpaque = false
        p.backgroundColor = .clear
        p.hasShadow = true
        p.isMovableByWindowBackground = true
        p.isReleasedWhenClosed = false
        p.hidesOnDeactivate = false
        p.becomesKeyOnlyIfNeeded = true
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        p.delegate = self
        p.contentView = NSHostingView(rootView: MiniWindowView(
            model: model,
            state: state,
            onToggle: { [weak self] in self?.onToggleTalk?() },
            onSettings: { [weak self] in self?.onSettings?() },
            onClose: { [weak self] in self?.close() }
        ))
        return p
    }

    private func close() {
        hide()
        onClose?()
    }

    /// 第一次出现放到屏幕右上角（菜单栏下），之后记住用户拖到的位置。
    private func position(_ p: NSPanel) {
        guard let screen = NSScreen.main else { return }
        let visible = screen.visibleFrame
        let x = visible.maxX - p.frame.width - 18
        let y = visible.maxY - p.frame.height - 12
        p.setFrameOrigin(NSPoint(x: x, y: y))
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        close()
        return false
    }
}

final class MiniWindowState: ObservableObject {
    @Published var phase = "idle"
    @Published var hotkeyName = "F2"

    var listening: Bool { phase == "starting" || phase == "listening" }
    var busy: Bool {
        phase == "decoding" || phase == "processing" || phase == "inserting" || phase == "polishing"
    }
}

private struct MiniWindowView: View {
    @ObservedObject var model: AppModel
    @ObservedObject var state: MiniWindowState
    let onToggle: () -> Void
    let onSettings: () -> Void
    let onClose: () -> Void

    /// 今日节省：`today.chars ÷ 1000 × 40` 分钟，向下取整（DESIGN §12.3）
    private var todayMinutes: Int {
        ((model.stats?.today.chars ?? 0) / 1000) * 40
    }

    private var hint: String {
        if state.listening { return "正在听，点击停止" }
        if state.busy { return "识别中…" }
        return "按 \(state.hotkeyName) 或点击开始"
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Spacer()
                Button(action: onClose) {
                    Image(systemName: "xmark")
                        .font(.system(size: 9, weight: .bold))
                        .foregroundStyle(.secondary)
                        .frame(width: 18, height: 18)
                        .background(Color.primary.opacity(0.06), in: Circle())
                }
                .buttonStyle(.plain)
                .help("关闭主窗（菜单栏「显示主窗」可再打开）")
            }
            .padding(.top, 8)
            .padding(.trailing, 8)

            Spacer(minLength: 4)

            Button(action: onToggle) {
                ZStack {
                    Circle()
                        .fill(state.listening ? Color.red.opacity(0.92) : Color.accentColor)
                        .shadow(color: (state.listening ? Color.red : Color.accentColor).opacity(0.28),
                                radius: 8, x: 0, y: 3)
                    Image(systemName: state.listening ? "stop.fill" : "mic.fill")
                        .font(.system(size: 28, weight: .medium))
                        .foregroundStyle(.white)
                }
                .frame(width: 76, height: 76)
                .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .disabled(state.busy)
            .help(state.listening ? "停止并识别" : "开始录音")
            .padding(.bottom, 10)

            Text(hint)
                .font(.system(size: 11, weight: .medium))
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
                .frame(height: 30, alignment: .top)
                .padding(.horizontal, 10)

            Spacer(minLength: 4)

            HStack(spacing: 4) {
                Text("今日已节省")
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
                Text(todayMinutes > 0 ? "\(todayMinutes) 分" : "0 分")
                    .font(.system(size: 12, weight: .semibold))
                    .monospacedDigit()
            }
            .padding(.bottom, 8)

            HStack {
                Spacer()
                Button(action: onSettings) {
                    Image(systemName: "gearshape")
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .help("设置")
            }
            .padding(.horizontal, 12)
            .padding(.bottom, 8)
        }
        .frame(width: 170, height: 230)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(Color(nsColor: .windowBackgroundColor))
        )
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
    }
}
