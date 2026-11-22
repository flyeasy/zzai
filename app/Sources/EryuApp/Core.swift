import Foundation
import EryuC

/// core → 壳 的事件（协议 v1，见 DESIGN §3.1 / D-006）
struct CoreEvent {
    let kind: String
    let seq: UInt64
    let session: UInt64
    let generation: UInt64
    let payload: [String: Any]

    var string: String { (payload["text"] as? String) ?? "" }
    var taskId: String { (payload["task_id"] as? String) ?? "" }
    var code: String { (payload["code"] as? String) ?? "" }
    var message: String { (payload["message"] as? String) ?? "" }
    var phase: String { (payload["phase"] as? String) ?? "" }
    var mode: String { (payload["mode"] as? String) ?? "paste" }
    var historyId: String { (payload["history_id"] as? String) ?? "" }
    var toml: String { (payload["toml"] as? String) ?? "" }
    var engine: String { (payload["engine"] as? String) ?? "" }
    var partial: Bool { (payload["partial"] as? Bool) ?? false }
    var meterRms: Double { (payload["rms"] as? NSNumber)?.doubleValue ?? 0 }
    var meterPeak: Double { (payload["peak"] as? NSNumber)?.doubleValue ?? 0 }
    var meterSilent: Bool { (payload["silent"] as? Bool) ?? true }
    /// 松手 → 出文本的毫秒数（D-038 口径；没带就不算）
    var latencyMs: Int? { (payload["latency_ms"] as? NSNumber)?.intValue }
}

/// EryuCore 的 Swift 包装：只在主线程创建/释放；事件回到主线程处理。
final class Core {
    private var handle: OpaquePointer?
    private var lastText: String?

    /// 事件在 core 派发线程产生，这里切到主线程后再回调（硬规则：壳侧不得阻塞 core 线程）
    var onEvent: ((CoreEvent) -> Void)?

    static var abi: UInt32 { eryu_abi_version() }

    /// 静态库按需链接：显式引用每个 ABI 符号，保证全部进二进制（scripts/smoke.sh 会核对）
    static func keepABISymbolsAlive() {
        _ = eryu_abi_version
        _ = eryu_core_new
        _ = eryu_core_free
        _ = eryu_set_event_cb
        _ = eryu_clear_event_cb
        _ = eryu_hotkey_down
        _ = eryu_hotkey_up
        _ = eryu_cancel
        _ = eryu_submit_result
        _ = eryu_get_hotwords
        _ = eryu_set_hotwords
        _ = eryu_polish_test
        _ = eryu_run_command
        _ = eryu_polish_models
        _ = eryu_lan_log
        _ = eryu_set_test_mode
        _ = eryu_failed_segments
        _ = eryu_retry_segment
        _ = eryu_decode_wav
        _ = eryu_get_profiles
        _ = eryu_set_profiles
        _ = eryu_set_active_profile
        _ = eryu_models_pull
        _ = eryu_get_last_result
        _ = eryu_drop_last_result
        _ = eryu_string_free
        _ = eryu_debug_submit_text
        _ = eryu_debug_panic
        _ = eryu_debug_record
        _ = eryu_get_config_toml
        _ = eryu_set_config_toml
        _ = eryu_get_config_json
        _ = eryu_set_config_json
        _ = eryu_get_stats
        _ = eryu_get_history
        _ = eryu_delete_history
        _ = eryu_clear_history
        _ = eryu_retry_failed
        _ = eryu_models_list
        _ = eryu_set_engine
        _ = eryu_list_devices
        _ = eryu_note_foreground_app
        _ = eryu_meter_start
        _ = eryu_meter_stop
        _ = eryu_diagnose
        _ = eryu_voice_state
        _ = eryu_voice_enroll_wav
        _ = eryu_voice_clear
        _ = eryu_voice_enroll_mic
    }
    static var abiMajor: UInt32 { abi >> 16 }
    static var abiMinor: UInt32 { abi & 0xFFFF }

    init?() {
        guard Core.abiMajor == 1 else {
            NSLog("Eryu: ABI 不兼容 major=%u", Core.abiMajor)
            return nil
        }
        guard let h = eryu_core_new(nil) else { return nil }
        handle = h
        let ctx = Unmanaged.passUnretained(self).toOpaque()
        guard eryu_set_event_cb(h, eryuEventTrampoline, ctx) == 0 else {
            eryu_core_free(h)
            handle = nil
            return nil
        }
    }

    deinit {
        guard let h = handle else { return }
        _ = eryu_clear_event_cb(h)   // 停机屏障：此后不再有回调
        eryu_core_free(h)
        handle = nil
    }

    fileprivate func deliver(_ json: String) {
        guard
            let data = json.data(using: .utf8),
            let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return }
        let event = CoreEvent(
            kind: obj["type"] as? String ?? "unknown",
            seq: (obj["seq"] as? NSNumber)?.uint64Value ?? 0,
            session: (obj["session"] as? NSNumber)?.uint64Value ?? 0,
            generation: (obj["generation"] as? NSNumber)?.uint64Value ?? 0,
            payload: obj["payload"] as? [String: Any] ?? [:]
        )
        DispatchQueue.main.async { [weak self] in
            if event.kind == "result" { self?.lastText = event.string }
            self?.onEvent?(event)
        }
    }

    // MARK: - 命令（入队即返回）

    func hotkeyDown() { guard let h = handle else { return }; _ = eryu_hotkey_down(h) }
    func hotkeyUp() { guard let h = handle else { return }; _ = eryu_hotkey_up(h) }
    func cancel() { guard let h = handle else { return }; _ = eryu_cancel(h) }
    func debugRecord(seconds: Int32) {
        guard let h = handle else { return }
        _ = eryu_debug_record(h, seconds)
    }

    func debugSubmit(_ text: String) {
        guard let h = handle else { return }
        text.withCString { _ = eryu_debug_submit_text(h, $0) }
    }
    func submitResult(taskId: String, status: String, detail: String = "") {
        guard let h = handle else { return }
        taskId.withCString { tid in
            status.withCString { st in
                detail.withCString { d in
                    _ = eryu_submit_result(h, tid, st, d)
                }
            }
        }
    }

    // MARK: - 配置 / 统计 / 历史（ABI v1.1）

    /// 取回由 Rust 分配的字符串（调用方用 eryu_string_free 释放）
    private func takeString(_ raw: UnsafeMutablePointer<CChar>?) -> String? {
        guard let raw else { return nil }
        defer { eryu_string_free(raw) }
        return String(cString: raw)
    }

    func configToml() -> String? {
        guard let h = handle else { return nil }
        return takeString(eryu_get_config_toml(h))
    }

    func setConfigToml(_ toml: String) -> (code: Int32, error: String?) {
        guard let h = handle else { return (-1, "核心未初始化") }
        var errPtr: UnsafeMutablePointer<CChar>?
        let code = toml.withCString { eryu_set_config_toml(h, $0, &errPtr) }
        return (code, takeString(errPtr))
    }

    func configJSON() -> String? {
        guard let h = handle else { return nil }
        return takeString(eryu_get_config_json(h))
    }

    func setConfigJSON(_ json: String) -> (code: Int32, error: String?) {
        guard let h = handle else { return (-1, "核心未初始化") }
        var errPtr: UnsafeMutablePointer<CChar>?
        let code = json.withCString { eryu_set_config_json(h, $0, &errPtr) }
        return (code, takeString(errPtr))
    }

    func statsJSON() -> String? {
        guard let h = handle else { return nil }
        return takeString(eryu_get_stats(h))
    }

    func historyJSON(limit: Int = 200) -> String? {
        guard let h = handle else { return nil }
        return takeString(eryu_get_history(h, Int32(limit)))
    }

    func deleteHistory(id: String) -> Int32 {
        guard let h = handle else { return -1 }
        return id.withCString { eryu_delete_history(h, $0) }
    }

    func clearHistory() -> Int32 {
        guard let h = handle else { return -1 }
        return eryu_clear_history(h)
    }

    func retryFailed() -> Int32 {
        guard let h = handle else { return -1 }
        return eryu_retry_failed(h)
    }

    func modelsListJSON() -> String? {
        guard let h = handle else { return nil }
        return takeString(eryu_models_list(h))
    }

    func setEngine(id: String) -> Int32 {
        guard let h = handle else { return -1 }
        return id.withCString { eryu_set_engine(h, $0) }
    }

    func devicesJSON() -> String? {
        guard let h = handle else { return nil }
        return takeString(eryu_list_devices(h))
    }

    func noteForegroundApp(_ name: String) {
        guard let h = handle else { return }
        name.withCString { _ = eryu_note_foreground_app(h, $0) }
    }

    func meterStart() {
        guard let h = handle else { return }
        _ = eryu_meter_start(h)
    }

    func meterStop() {
        guard let h = handle else { return }
        _ = eryu_meter_stop(h)
    }

    func diagnose() -> String? {
        guard let h = handle else { return nil }
        return takeString(eryu_diagnose(h))
    }

    /// 把 JSON 文本解析成数组/字典（失败返回 nil）
    static func decodeJSON(_ text: String?) -> Any? {
        guard let text, let data = text.data(using: .utf8) else { return nil }
        return try? JSONSerialization.jsonObject(with: data)
    }

    /// 热词表（JSON 文本）
    func hotwordsJSON() -> String? {
        guard let h = handle else { return nil }
        return takeString(eryu_get_hotwords(h))
    }

    /// 保存热词表：返回 (错误码, 错误说明)
    func setHotwords(_ json: String) -> (Int32, String?) {
        guard let h = handle else { return (-1, "核心未就绪") }
        var err: UnsafeMutablePointer<CChar>?
        let code = json.withCString { eryu_set_hotwords(h, $0, &err) }
        return (code, takeString(err))
    }

    /// 用当前润色配置跑一次示例（设置页「测试连接」）；同步网络调用，别在主线程调
    func polishTest() -> (String?, String?) {
        guard let h = handle else { return (nil, "core 未初始化") }
        var err: UnsafeMutablePointer<CChar>?
        let out = eryu_polish_test(h, &err)
        let text = out.map { String(cString: $0) }
        if let out { eryu_string_free(out) }
        return (text, takeString(err))
    }

    /// 选区指令改写（同步网络调用：请在后台队列里调用）
    /// 下载并安装模型（DESIGN §9.2）：0 已开始 / -3 未知 / -4 已有下载 / -5 已安装
    func pullModel(_ id: String) -> (Int32, String?) {
        guard let handle else { return (-1, "core 未就绪") }
        var err: UnsafeMutablePointer<CChar>?
        let rc = eryu_models_pull(handle, id, &err)
        var message: String?
        if let err {
            message = String(cString: err)
            eryu_string_free(err)
        }
        return (rc, message)
    }

    func profilesJSON() -> String? {
        guard let h = handle, let p = eryu_get_profiles(h) else { return nil }
        defer { eryu_string_free(p) }
        return String(cString: p)
    }

    func setProfilesJSON(_ json: String) -> (Int32, String?) {
        guard let h = handle else { return (-1, "core 未初始化") }
        var err: UnsafeMutablePointer<CChar>?
        let code = json.withCString { eryu_set_profiles(h, $0, &err) }
        return (code, takeString(err))
    }

    func setActiveProfile(_ name: String) -> (Int32, String?) {
        guard let h = handle else { return (-1, "core 未初始化") }
        var err: UnsafeMutablePointer<CChar>?
        let code = name.withCString { eryu_set_active_profile(h, $0, &err) }
        return (code, takeString(err))
    }

    /// 声纹状态（JSON 字符串，原样交给界面解析）
    func voiceState() -> String? {
        guard let h = handle, let p = eryu_voice_state(h) else { return nil }
        defer { eryu_string_free(p) }
        return String(cString: p)
    }

    /// 用一段 16k 单声道 WAV 注册声纹：返回 (0 成功 / 其它, 说明)
    func voiceEnrollWav(_ path: String) -> (Int32, String?) {
        guard let h = handle else { return (-1, "core 未初始化") }
        var err: UnsafeMutablePointer<CChar>?
        let code = path.withCString { eryu_voice_enroll_wav(h, $0, &err) }
        return (code, takeString(err))
    }

    /// 录一段麦克风音频当声纹（阻塞 seconds 秒，调用方要在后台队列跑）
    func voiceEnrollMic(seconds: Int32) -> (Int32, String?) {
        guard let h = handle else { return (-1, "core 未初始化") }
        var err: UnsafeMutablePointer<CChar>?
        let code = eryu_voice_enroll_mic(h, seconds, &err)
        return (code, takeString(err))
    }

    func voiceClear() -> (Int32, String?) {
        guard let h = handle else { return (-1, "core 未初始化") }
        var err: UnsafeMutablePointer<CChar>?
        let code = eryu_voice_clear(h, &err)
        return (code, takeString(err))
    }

    /// 自测模式：不写历史、不落盘（自动化路径专用）
    func setTestMode(_ on: Bool) {
        guard let handle else { return }
        _ = eryu_set_test_mode(handle, on ? 1 : 0)
    }

    func runCommand(instruction: String, text: String) -> (String?, String?) {
        guard let h = handle else { return (nil, "core 未初始化") }
        var err: UnsafeMutablePointer<CChar>?
        let out = eryu_run_command(h, instruction, text, &err)
        let errText = takeString(err)
        if let out {
            return (takeString(out), nil)
        }
        return (nil, errText ?? "改写失败")
    }

    /// 用户已取回结果：清掉 pending 暂存（历史不动）
    /// 可重试的失败段序号（部分成功时才有内容）
    func failedSegments() -> [Int] {
        guard let c = handle, let p = eryu_failed_segments(c) else { return [] }
        defer { eryu_string_free(p) }
        let json = String(cString: p)
        guard let d = json.data(using: .utf8),
              let arr = try? JSONSerialization.jsonObject(with: d) as? [Int] else { return [] }
        return arr
    }

    /// 只重试某一段：返回 (0 成功受理 / 其它 错误码, 错误原因)
    func decodeWav(_ path: String) -> (Int32, String?) {
        var err: UnsafeMutablePointer<CChar>? = nil
        let code = eryu_decode_wav(handle, path, &err)
        defer { if let err { eryu_string_free(err) } }
        return (code, err.map { String(cString: $0) })
    }

    func retrySegment(_ index: Int) -> (Int32, String?) {
        guard let c = handle else { return (-1, nil) }
        var err: UnsafeMutablePointer<CChar>?
        let code = eryu_retry_segment(c, Int32(index), &err)
        var message: String?
        if let e = err {
            message = String(cString: e)
            eryu_string_free(e)
        }
        return (code, message)
    }

    /// 最近局域网请求摘要（JSON）
    func lanLog() -> String? {
        guard let p = eryu_lan_log(handle) else { return nil }
        defer { eryu_string_free(p) }
        return String(cString: p)
    }

    func dropLastResult() {
        guard let h = handle else { return }
        _ = eryu_drop_last_result(h)
    }

    func takeLastResult() -> String? {
        guard let h = handle, let raw = eryu_get_last_result(h) else { return lastText }
        defer { eryu_string_free(raw) }
        lastText = String(cString: raw)
        return lastText
    }

    /// 问润色服务端有哪些模型（用已保存的 Key）。返回 (模型列表, 失败原因)
    func polishModels() -> ([String], String?) {
        guard let handle else { return ([], "核心未初始化") }
        var err: UnsafeMutablePointer<CChar>?
        let raw = eryu_polish_models(handle, &err)
        let reason = err.map { String(cString: $0) }
        if let err { eryu_string_free(err) }
        guard let raw else { return ([], reason ?? "没有返回") }
        let text = String(cString: raw)
        eryu_string_free(raw)
        guard let data = text.data(using: .utf8),
              let list = try? JSONDecoder().decode([String].self, from: data) else {
            return ([], "返回格式不对：\(text.prefix(120))")
        }
        return (list, nil)
    }

}

/// C 回调转 Swift（顶层函数才有 C 函数指针，见 Core.init）
private func eryuEventTrampoline(_ json: UnsafePointer<CChar>?, _ ctx: UnsafeMutableRawPointer?) {
    guard let json, let ctx else { return }
    let core = Unmanaged<Core>.fromOpaque(ctx).takeUnretainedValue()
    core.deliver(String(cString: json))

}