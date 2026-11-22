import AppKit
import Combine
import Darwin
import Foundation
import Security

// MARK: - 与 core 对齐的数据结构（JSON 字段名与 config.rs / store.rs 一致）

struct EryuConfig: Decodable {
    var engine: String = "sense"
    var num_threads: Int = 2
    var model_dir: String = ""
    var vad_model: String = ""
    var qwen3_dir: String = ""
    var model_mirror: String = ""
    var model_proxy: String = ""
    var onboarding_done: Bool = false
    var onboarding_probe_done: Bool = false
    var min_record_ms: Int = 400
    var max_record_secs: Int = 120
    var device: String = ""
    var history_enabled: Bool = true
    var history_limit: Int = 200
    var privacy_mode: Bool = false
    var pause_media: Bool = false
    var denoise: Bool = true
    var punct: Bool = true
    var source_lock: Bool = false
    var voice_lock: Bool = false
    var voice_lock_threshold: Double = 0.62
    var voice_enrolled: Bool = false
    var voice_model_ready: Bool = false
    var punct_ready: Bool = false
    var lan_enabled: Bool = false
    var lan_port: Int = 8765
    var lan_has_token: Bool = false
    var lan_allow_lan: Bool = false
    var denoise_ready: Bool = false
    var insert_mode: String = "paste"
    var insert_per_app: [String: String] = [:]
    var hotkey: String = "CTRL+ALT+SPACE"
    var polish_enabled: Bool = false
    var polish_level: String = "off"
    var polish_endpoint: String = ""
    var polish_model: String = ""
    var polish_has_key: Bool = false
    var polish_timeout_ms: Int = 3000
    var launch_at_login: Bool = false
    var mini_window: Bool = true
    var load_warning: String?
}

struct ModelInfo: Decodable, Identifiable {
    var id: String
    var name: String
    var installed: Bool
    var active: Bool
    var size_bytes: Int64
    var note: String
    /// 未安装时显示的预估体积（来自 core 的清单）
    var approx_mb: Int = 0

    var sizeHintMB: Int { approx_mb > 0 ? approx_mb : 0 }

    var sizeText: String {
        if size_bytes <= 0 { return "—" }
        let mb = Double(size_bytes) / 1_048_576
        return mb >= 1024 ? String(format: "%.1f GB", mb / 1024) : String(format: "%.0f MB", mb)
    }
}

struct DeviceInfo: Decodable, Identifiable {
    var name: String
    var `default`: Bool
    var id: String { name }
}

struct DeviceList: Decodable {
    var devices: [DeviceInfo]
    var selected: String
    var `default`: String?
}

struct DayStat: Decodable, Identifiable {
    var date: String
    var sessions: Int
    var chars: Int
    var speech_ms: Int64
    var id: String { date }
}

struct LatencyInfo: Decodable {
    var count: Int
    var min: Int?
    var p50: Int?
    var p95: Int?
    var max: Int?
}

struct LanLogEntry: Decodable, Identifiable {
    let at: Int
    let src: String
    let path: String
    let code: Int
    let ms: Int
    let bytes: Int
    var id: String { "\(at)-\(src)-\(path)-\(code)-\(ms)" }
    var timeText: String {
        let f = DateFormatter(); f.dateFormat = "HH:mm:ss"
        return f.string(from: Date(timeIntervalSince1970: TimeInterval(at)))
    }
}

struct StatsInfo: Decodable {
    var sessions: Int
    var segments: Int
    var chars: Int
    var speech_ms: Int64
    var decode_ms: Int64
    var since: String
    var today: DayStat
    var days: [DayStat]
    var history_count: Int
    var history_enabled: Bool
    var latency: LatencyInfo?

    /// 按 40 分钟/千字 折算的"省下打字时间"
    var savedMinutes: Int { Int(Double(chars) / 1000.0 * 40.0) }
    var speechMinutes: Int { Int(speech_ms / 60_000) }
}

struct HistoryItem: Decodable, Identifiable {
    var id: String
    var at: String
    var text: String
    var status: String
    var app: String
    var chars: Int
    var ms: Int64
    var retryable: Bool

    var statusText: String {
        switch status {
        case "inserted": return "已上屏"
        case "pending": return "待复制"
        case "failed": return "识别失败"
        default: return status
        }
    }

    var timeText: String {
        // 2026-11-19T01:02:03+08:00 → 11-19 01:02
        guard at.count >= 16 else { return at }
        let s = at.dropFirst(5).prefix(11)      // 11-19T01:02
        return s.replacingOccurrences(of: "T", with: " ")
    }
}

/// 模型下载进度
struct PullProgress: Equatable {
    var received: Double
    var total: Double
    var phase: String
    var fraction: Double { total > 0 ? min(1, received / total) : 0 }
}

struct HotwordRule: Decodable, Identifiable, Equatable {
    var from: String
    var to: String
    /// 行身份必须和内容无关：以前用 from+to 当 id，用户在输入框里每敲一个字 id 就变，
    /// `ForEach` 以为整行是新的、销毁重建文本框 → 打两个字焦点就飞出去（I-174 的真根因）。
    let id = UUID()

    private enum CodingKeys: String, CodingKey { case from, to }

    init(from: String, to: String) {
        self.from = from
        self.to = to
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        from = try c.decode(String.self, forKey: .from)
        to = try c.decode(String.self, forKey: .to)
    }
}

struct PolishProfile: Decodable, Identifiable, Equatable {
    var name: String
    var icon: String = ""
    var level: String = "light"
    var prompt: String = ""
    var slot: String = ""
    var active: Bool = false
    var id: String { name }
}

struct ProfileTable: Decodable {
    var active: String?
    var profiles: [PolishProfile]
    var path: String?
}

struct HotwordTable: Decodable {
    var rules: [HotwordRule]
    var path: String?
    /// 文件本身有格式问题时核心给的一句说明（规则照用，但要让用户看见）
    var error: String?
}

// MARK: - 设置页/统计页的视图模型

final class AppModel: ObservableObject {
    let core: Core

    @Published var config = EryuConfig()
    @Published var models: [ModelInfo] = []
    @Published var deviceList = DeviceList(devices: [], selected: "", default: nil)
    @Published var stats: StatsInfo?
    @Published var history: [HistoryItem] = []
    @Published var diagnose = ""
    /// 局域网请求摘要（不记内容，只给排查用）
    @Published var lanLog: [LanLogEntry] = []
    /// 热词表：设置页直接编辑，保存即生效（DESIGN §4.3）
    @Published var hotwords: [HotwordRule] = []
    /// 注入后学到、还没采纳的改法（DESIGN §10.2 热词自动学习；落盘在 suggestions.json）
    @Published var suggestions: [EditObserver.Suggestion] = []
    /// 正在下载的模型：id → 进度（DESIGN §9.2）
    @Published var pulling: [String: PullProgress] = [:]
    @Published var hotwordsPath = ""
    /// 热词表文件解析出问题时给界面看的说明（空 = 正常）
    @Published var hotwordsError = ""
    @Published var profiles: [PolishProfile] = []
    @Published var profilesPath = ""
    @Published var polishTestResult = ""
    @Published var polishModels: [String] = []
    @Published var polishModelsHint = ""
    @Published var pendingPolishLevel: String?
    @Published var pendingPolishEnabled = false
    var polishConsent = false

    /// 学到但没采纳的改法落盘位置（与 core 用同一个配置目录）
    private var suggestionsPath: URL {
        let base = ProcessInfo.processInfo.environment["ERYU_CONFIG_DIR"].map { URL(fileURLWithPath: $0) }
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".eryu", isDirectory: true)
        return base.appendingPathComponent("suggestions.json")
    }

    @Published var meterRms: Double = 0
    @Published var meterPeak: Double = 0
    @Published var meterActive = false

    @Published var toast: String?
    /// 快捷键改动后通知 AppDelegate 重新注册
    var onHotkeyChanged: ((String) -> Void)?
    var onProfileSlots: (([PolishProfile]) -> Void)?
    /// 「打开系统设置」目标改动后通知（无需额外处理，直接开 URL）

    private var toastWork: DispatchWorkItem?

    init(core: Core) {
        self.core = core
    }

    // MARK: 读取

    func refreshAll() {
        refreshConfig()
        refreshModels()
        refreshDevices()
        refreshStats()
        refreshLanLog()
        refreshHistory()
        refreshHotwords()
        refreshProfiles()
        refreshVoice()
        loadSuggestions()
        diagnose = core.diagnose() ?? ""
    }

    func refreshConfig() {
        guard let json = core.configJSON(),
              let data = json.data(using: .utf8),
              let cfg = try? JSONDecoder().decode(EryuConfig.self, from: data) else { return }
        config = cfg
    }

    func refreshModels() {
        guard let json = core.modelsListJSON(), let data = json.data(using: .utf8),
              let list = try? JSONDecoder().decode([ModelInfo].self, from: data) else { return }
        models = list
    }

    func refreshDevices() {
        guard let json = core.devicesJSON(), let data = json.data(using: .utf8),
              let list = try? JSONDecoder().decode(DeviceList.self, from: data) else { return }
        deviceList = list
    }

    func refreshLanLog() {
        guard let json = core.lanLog(), let data = json.data(using: .utf8) else { lanLog = []; return }
        lanLog = (try? JSONDecoder().decode([LanLogEntry].self, from: data)) ?? []
    }

    func refreshStats() {
        guard let json = core.statsJSON(), let data = json.data(using: .utf8),
              let s = try? JSONDecoder().decode(StatsInfo.self, from: data) else { return }
        stats = s
    }

    func refreshHistory() {
        guard let json = core.historyJSON(), let data = json.data(using: .utf8),
              let list = try? JSONDecoder().decode([HistoryItem].self, from: data) else { return }
        history = list
    }

    func refreshHotwords() {
        guard let json = core.hotwordsJSON(), let data = json.data(using: .utf8),
              let table = try? JSONDecoder().decode(HotwordTable.self, from: data) else { return }
        hotwords = table.rules
        hotwordsPath = table.path ?? ""
        hotwordsError = table.error ?? ""
    }

    func refreshProfiles() {
        guard let json = core.profilesJSON(), let data = json.data(using: .utf8),
              let table = try? JSONDecoder().decode(ProfileTable.self, from: data) else { return }
        profiles = table.profiles
        profilesPath = table.path ?? ""
    }

    func saveProfiles(_ list: [PolishProfile], active: String? = nil) {
        let payload: [String: Any] = [
            "active": active ?? profiles.first(where: { $0.active })?.name ?? list.first?.name ?? "",
            "profiles": list.map {
                ["name": $0.name, "icon": $0.icon, "level": $0.level,
                 "prompt": $0.prompt, "slot": $0.slot]
            },
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: payload),
              let text = String(data: data, encoding: .utf8) else { return }
        let (code, err) = core.setProfilesJSON(text)
        if code != 0 {
            showToast("保存 profile 失败：\(err ?? "未知错误")")
        } else {
            refreshProfiles()
            showToast("profile 已保存（\(list.count) 档）")
        }
    }

    func activateProfile(_ name: String) {
        let (code, err) = core.setActiveProfile(name)
        if code != 0 {
            showToast("切换失败：\(err ?? "未知错误")")
        } else {
            refreshProfiles()
            showToast("润色档已切到「\(name)」，下一条录音生效")
        }
    }

    /// 把 profile 的快捷键槽注册进系统（保存后、启动时各调一次）
    func applyProfileSlots() {
        onProfileSlots?(profiles)
    }

    // MARK: 写入

    /// 保存热词表；失败保留旧值并 toast
    func saveHotwords(_ rules: [HotwordRule]) {
        let rows = rules
            .filter { !$0.from.trimmingCharacters(in: .whitespaces).isEmpty
                   && !$0.to.trimmingCharacters(in: .whitespaces).isEmpty }
        guard let data = try? JSONSerialization.data(withJSONObject: ["rules": rows.map { ["from": $0.from, "to": $0.to] }]),
              let json = String(data: data, encoding: .utf8) else { return }
        let (code, error) = core.setHotwords(json)
        if code == 0 {
            refreshHotwords()
            showToast("热词已保存（\(rows.count) 条），下一条录音生效")
        } else {
            showToast(error ?? "热词保存失败（错误码 \(code)）")
            refreshHotwords()
        }
    }

    /// 只提交改动的字段；失败保留旧配置并把错误显示出来
    private func patch(_ fields: [String: Any], toast: Bool = true) {
        guard let data = try? JSONSerialization.data(withJSONObject: fields),
              let json = String(data: data, encoding: .utf8) else { return }
        let (code, error) = core.setConfigJSON(json)
        if code == 0 {
            refreshConfig()
            refreshModels()
            if toast { showToast("已保存") }
        } else {
            showToast("保存失败：\(error ?? "未知错误")")
            refreshConfig()
        }
    }

    /// 模型下载镜像（留空走官方 GitHub）
    func setModelProxy(_ proxy: String) {
        patch(["model_proxy": proxy])
        showToast(proxy.isEmpty ? "下载代理已改为自动（跟随系统）" : "下载代理已保存：\(proxy)")
    }

    func setModelMirror(_ url: String) {
        patch(["model_mirror": url.trimmingCharacters(in: .whitespacesAndNewlines)])
        showToast("模型下载地址已保存")
    }

    /// 引导走完了（写 config，下次启动不再弹）
    func finishOnboarding() {
        patch(["onboarding_done": true], toast: false)
    }

    /// 引导第 3 步「试说一句」成功过一次（R-101）：之后启动不再回到这一步。
    func markProbePassed() {
        guard !config.onboarding_probe_done else { return }
        patch(["onboarding_probe_done": true], toast: false)
    }

    /// 迷你主窗开关（R-103）：关窗写 false，菜单栏「显示主窗」写回 true。
    func setMiniWindow(_ on: Bool) {
        guard config.mini_window != on else { return }
        patch(["mini_window": on], toast: false)
    }

    func setEngine(_ id: String) {
        let code = core.setEngine(id: id)
        switch code {
        case 0:
            showToast("已切换，下次录音生效")
            refreshModels()
            refreshConfig()
        case -4:
            showToast("模型未安装：先下载再切换")
        default:
            showToast("切换失败（\(code)）")
        }
    }

    func setMinRecordMs(_ ms: Int) { patch(["min_record_ms": ms]) }
    func setMaxRecordSecs(_ s: Int) { patch(["max_record_secs": s]) }
    func setDevice(_ name: String) { patch(["device": name]) }
    func setHistoryEnabled(_ on: Bool) { patch(["history_enabled": on]); refreshHistory(); refreshStats() }
    /// 开始下载一个模型
    func pullModel(_ id: String) {
        let (rc, message) = core.pullModel(id)
        switch rc {
        case 0:
            pulling[id] = PullProgress(received: 0, total: 0, phase: "starting")
            showToast("开始下载…")
        case -5:
            refreshModels()
            showToast("这个模型已经装好了")
        default:
            showToast(message ?? "下载没能开始（\(rc)）")
        }
    }

    func updatePull(id: String, received: Double, total: Double, phase: String) {
        pulling[id] = PullProgress(received: received, total: total, phase: phase)
    }

    func finishPull(id: String, ok: Bool, message: String?) {
        pulling.removeValue(forKey: id)
        refreshModels()
        showToast(ok ? "模型已装好，可在引擎列表里切换" : (message ?? "下载失败"))
    }

    /// 全局上屏方式：paste / keystroke（DESIGN §4.1）
    func setInsertMode(_ mode: String) { patch(["insert_mode": mode]) }

    /// 按应用覆盖上屏方式（整体替换，key = 应用名）
    func setInsertPerApp(_ map: [String: String]) { patch(["insert_per_app": map]) }

    /// 采纳一条学到的改法：写进热词表并移除候选
    func acceptSuggestion(_ s: EditObserver.Suggestion) {
        var rules = hotwords
        if let i = rules.firstIndex(where: { $0.from == s.from }) {
            rules[i] = HotwordRule(from: s.from, to: s.to)
        } else {
            rules.append(HotwordRule(from: s.from, to: s.to))
        }
        saveHotwords(rules)
        suggestions.removeAll { $0 == s }
        saveSuggestions()
    }

    func dismissSuggestion(_ s: EditObserver.Suggestion) {
        suggestions.removeAll { $0 == s }
        saveSuggestions()
    }

    func addSuggestion(_ s: EditObserver.Suggestion) {
        let exists = hotwords.contains { $0.from == s.from && $0.to == s.to }
        let already = suggestions.contains(s)
        if !exists && !already {
            suggestions.append(s)
            saveSuggestions()
        }
    }

    /// 把候选写盘：重启后还在，用户可以过几天再决定采纳/忽略（最多留 50 条）
    private func saveSuggestions() {
        if suggestions.count > 50 {
            suggestions = Array(suggestions.suffix(50))
        }
        let enc = JSONEncoder()
        enc.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? enc.encode(suggestions) else { return }
        let url = suggestionsPath
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: url, options: [.atomic])
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    }

    /// 启动时读回候选（文件坏了就当没有，不影响使用）
    func loadSuggestions() {
        guard let data = try? Data(contentsOf: suggestionsPath),
              let list = try? JSONDecoder().decode([EditObserver.Suggestion].self, from: data) else { return }
        // 已经进了热词表的不再当候选
        suggestions = list.filter { s in !hotwords.contains { $0.from == s.from && $0.to == s.to } }
    }

    /// 录音时暂停 Music / Spotify（DESIGN §10.2）
    func setPauseMedia(_ on: Bool) { patch(["pause_media": on]) }
    func setDenoise(_ on: Bool) { patch(["denoise": on]) }
    func setSourceLock(_ on: Bool) { patch(["source_lock": on]) }
    func setPunct(_ on: Bool) { patch(["punct": on]) }
    func setLanEnabled(_ on: Bool) { patch(["lan_enabled": on]) }
    func setLanPort(_ port: Int) { patch(["lan_port": port]) }
    func setLanToken(_ token: String) { patch(["lan_token": token]); refreshConfig() }

    /// 一键生成局域网 token（R-132）：不用用户自己想一个
    func generateLanToken() -> String {
        var bytes = [UInt8](repeating: 0, count: 12)
        _ = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        let t = bytes.map { String(format: "%02x", $0) }.joined()
        setLanToken(t)
        return t
    }

    /// 本机在局域网里的 IPv4（取第一个非回环、非 169.254 的地址）
    func localIPv4() -> String? {
        var ifaddr: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&ifaddr) == 0, let first = ifaddr else { return nil }
        defer { freeifaddrs(ifaddr) }
        var fallback: String?
        for ptr in sequence(first: first, next: { $0.pointee.ifa_next }) {
            let flags = Int32(ptr.pointee.ifa_flags)
            guard flags & IFF_UP != 0, flags & IFF_LOOPBACK == 0 else { continue }
            guard let sa = ptr.pointee.ifa_addr, sa.pointee.sa_family == UInt8(AF_INET) else { continue }
            var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            let r = getnameinfo(sa, socklen_t(sa.pointee.sa_len), &host, socklen_t(host.count),
                                nil, 0, NI_NUMERICHOST)
            guard r == 0 else { continue }
            let ip = String(cString: host)
            if ip.hasPrefix("169.254.") { fallback = fallback ?? ip; continue }
            return ip
        }
        return fallback
    }
    func setLanAllowLan(_ on: Bool) { patch(["lan_allow_lan": on]) }

    // MARK: - 只录我的声音（D-051）

    @Published var voiceBusy = false
    @Published var voiceDim: Int = 0
    @Published var voiceCreated: String = ""

    func setVoiceLock(_ on: Bool) { patch(["voice_lock": on]) }

    /// 刷新声纹状态（维度/注册时间/模型在不在）
    func refreshVoice() {
        guard let json = core.voiceState(), let d = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: d) as? [String: Any] else { return }
        voiceDim = (obj["dim"] as? Int) ?? 0
        voiceCreated = (obj["created"] as? String) ?? ""
    }

    /// 录 seconds 秒当声纹（阻塞调用，放后台）
    func enrollVoice(seconds: Int32 = 8) {
        guard !voiceBusy else { return }
        voiceBusy = true
        showToast("开始录 \(seconds) 秒：请用平时说话的音量念一句话…")
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self else { return }
            let (code, err) = self.core.voiceEnrollMic(seconds: seconds)
            DispatchQueue.main.async {
                self.voiceBusy = false
                if code == 0 {
                    self.refreshConfig()
                    self.refreshVoice()
                    self.showToast("声纹已保存（\(self.voiceDim) 维）")
                } else {
                    self.showToast(err ?? "声纹没录成（\(code)）")
                }
            }
        }
    }

    func clearVoice() {
        let (code, err) = core.voiceClear()
        if code == 0 {
            refreshConfig()
            refreshVoice()
            showToast("已删除声纹：不再区分谁在说话")
        } else {
            showToast(err ?? "删除失败（\(code)）")
        }
    }

    /// 隐私模式：不写历史、失败音频只留内存、浮层不显示识别文本（DESIGN §10.2）
    func setPrivacyMode(_ on: Bool) {
        patch(["privacy_mode": on])
        refreshHistory()
        refreshStats()
        showToast(on ? "隐私模式已开启：不写历史、不落盘、浮层不显示文本" : "隐私模式已关闭")
    }
    func setHistoryLimit(_ n: Int) { patch(["history_limit": n]) }
    func setLaunchAtLogin(_ on: Bool) {
        let ok = LoginItem.setEnabled(on)
        patch(["launch_at_login": ok ? on : !on])
        if !ok { showToast("开机启动设置失败：需要把 Eryu 放到「应用程序」并重新签名") }
    }

    func setHotkey(_ spec: String) {
        guard HotkeySpec.parse(spec) != nil else {
            showToast("快捷键格式不对：例如 CTRL+ALT+SPACE")
            return
        }
        patch(["hotkey": spec])
        onHotkeyChanged?(spec)
    }

    /// 润色总开关：先确认隐私（文本会离开本机），再落配置。
    /// 力度不在这里选——力度属于「润色档」，档里定（D-102）。
    func setPolishEnabled(_ on: Bool) {
        guard on != config.polish_enabled else { return }
        if on && !polishConsent {
            polishConsent = true
            pendingPolishEnabled = true
            return
        }
        // 打开润色但还没选档时默认「轻度」，否则开关开着文字却不变，像坏了
        if on && config.polish_level == "off" {
            patch(["polish_enabled": true, "polish_level": "light"])
        } else {
            patch(["polish_enabled": on])
        }
        if on && !config.polish_has_key {
            showToast("润色已开启，但还没填 API Key：填好之前不会生效")
        }
    }

    func setPolishLevel(_ level: String) {
        guard level != config.polish_level else { return }
        if level != "off" && !polishConsent {
            polishConsent = true
            pendingPolishLevel = level
            return
        }
        patch(["polish_level": level])
        if level != "off" && !config.polish_has_key {
            showToast("润色已开启，但还没填 API Key：填好之前不会生效")
        }
    }

    func confirmPolishConsent() {
        polishConsent = true
        if pendingPolishEnabled {
            pendingPolishEnabled = false
            patch(["polish_enabled": true])
            return
        }
        if let level = pendingPolishLevel {
            pendingPolishLevel = nil
            patch(["polish_level": level])
        }
    }

    func cancelPolishConsent() {
        pendingPolishLevel = nil
        pendingPolishEnabled = false
        refreshConfig()
    }

    func setPolishEndpoint(_ v: String) { patch(["polish_endpoint": v]) }
    func setPolishModel(_ v: String) { patch(["polish_model": v]) }
    func setPolishTimeout(_ ms: Int) { patch(["polish_timeout_ms": ms]) }

    func setPolishKey(_ v: String) {
        let key = v.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !key.isEmpty else { return }
        patch(["polish_api_key": key])
    }

    /// 检测服务端可用模型（GET /models）。用已保存的 Key，不在壳里另存一份。
    func detectPolishModels() {
        polishModelsHint = "正在问服务端…"
        DispatchQueue.global(qos: .userInitiated).async {
            let (list, err) = self.core.polishModels()
            DispatchQueue.main.async {
                if let err {
                    self.polishModels = []
                    self.polishModelsHint = "检测失败：\(err)"
                } else {
                    self.polishModels = list
                    self.polishModelsHint = "检测到 \(list.count) 个模型"
                }
            }
        }
    }

    /// 测试连接（后台跑，避免卡主线程）
    func testPolish() {
        DispatchQueue.global(qos: .userInitiated).async {
            let (text, err) = self.core.polishTest()
            DispatchQueue.main.async {
                if let text {
                    self.polishTestResult = "润色后：\(text)"
                } else {
                    self.polishTestResult = err ?? "测试失败"
                }
            }
        }
    }

    func deleteHistory(_ id: String) {
        let code = core.deleteHistory(id: id)
        if code == 0 { refreshHistory(); refreshStats() } else { showToast("没找到这条记录") }
    }

    func clearHistory() {
        _ = core.clearHistory()
        refreshHistory()
        refreshStats()
        showToast("历史已清空")
    }

    func retryFailed() {
        let code = core.retryFailed()
        if code != 0 { showToast("无法重试（\(code)）") }
    }

    // MARK: 电平

    func startMeter() {
        guard !meterActive else { return }
        meterActive = true
        core.meterStart()
    }

    func stopMeter() {
        guard meterActive else { return }
        meterActive = false
        core.meterStop()
        meterRms = 0
        meterPeak = 0
    }

    func handleMeterEvent(_ e: CoreEvent) {
        guard meterActive else { return }
        meterRms = e.meterRms
        meterPeak = e.meterPeak
    }

    // MARK: 提示

    func showToast(_ text: String) {
        toast = text
        toastWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.toast = nil }
        toastWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.4, execute: work)
    }
}
