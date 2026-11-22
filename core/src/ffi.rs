//! extern "C" 包装层（ABI v1.2，见 core/include/eryu_core.h）
//!
//! 每个入口都 `catch_unwind`：panic 不外泄，返回错误码；worker 线程内的 panic
//! 另外会转成 error 事件并停用实例。

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use crate::{Cmd, EryuCore, EventCb, ABI_MAJOR, ABI_MINOR};

struct CoreHandle(Arc<EryuCore>);

fn handle<'a>(ptr: *mut EryuCore) -> Option<&'a CoreHandle> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: 指针由 eryu_core_new 产生，调用方保证在 free 之前有效
    Some(unsafe { &*(ptr as *const CoreHandle) })
}

fn cstr<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    // SAFETY: 调用方保证 NUL 结尾
    unsafe { CStr::from_ptr(p).to_str().ok() }
}

fn guard<F: FnOnce() -> i32>(f: F) -> i32 {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(-1)
}

fn owned(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// 把 Rust 分配的字符串写到 out；失败写 NULL
unsafe fn write_out(out: *mut *mut c_char, value: Option<String>) {
    if out.is_null() {
        return;
    }
    *out = match value {
        Some(s) => owned(s),
        None => std::ptr::null_mut(),
    };
}

#[no_mangle]
pub extern "C" fn eryu_abi_version() -> u32 {
    (ABI_MAJOR << 16) | ABI_MINOR
}

#[no_mangle]
pub extern "C" fn eryu_core_new(config_dir: *const c_char) -> *mut EryuCore {
    let dir = cstr(config_dir).map(|s| s.to_string());
    match catch_unwind(move || {
        let path = dir.as_deref().map(std::path::Path::new);
        EryuCore::start(path)
    }) {
        Ok(Ok(core)) => Box::into_raw(Box::new(CoreHandle(core))) as *mut EryuCore,
        _ => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn eryu_core_free(ptr: *mut EryuCore) {
    if ptr.is_null() {
        return;
    }
    let _ = catch_unwind(|| {
        // SAFETY: 由 eryu_core_new 分配，且只释放一次；Arc drop → worker join
        let boxed = unsafe { Box::from_raw(ptr as *mut CoreHandle) };
        drop(boxed);
    });
}

#[no_mangle]
pub extern "C" fn eryu_set_event_cb(
    ptr: *mut EryuCore,
    cb: Option<EventCb>,
    ctx: *mut c_void,
) -> i32 {
    guard(|| match (handle(ptr), cb) {
        (Some(h), Some(cb)) => h.0.set_cb(cb, ctx),
        _ => -1,
    })
}

#[no_mangle]
pub extern "C" fn eryu_clear_event_cb(ptr: *mut EryuCore) -> i32 {
    guard(|| match handle(ptr) {
        Some(h) => {
            h.0.clear_cb();
            0
        }
        None => -1,
    })
}

#[no_mangle]
pub extern "C" fn eryu_hotkey_down(ptr: *mut EryuCore) -> i32 {
    guard(|| match handle(ptr) {
        Some(h) => h.0.send(Cmd::Down),
        None => -1,
    })
}

#[no_mangle]
pub extern "C" fn eryu_hotkey_up(ptr: *mut EryuCore) -> i32 {
    guard(|| match handle(ptr) {
        Some(h) => h.0.send(Cmd::Up),
        None => -1,
    })
}

#[no_mangle]
pub extern "C" fn eryu_cancel(ptr: *mut EryuCore) -> i32 {
    guard(|| match handle(ptr) {
        Some(h) => h.0.send(Cmd::Cancel),
        None => -1,
    })
}

#[no_mangle]
pub extern "C" fn eryu_submit_result(
    ptr: *mut EryuCore,
    task_id: *const c_char,
    status: *const c_char,
    detail: *const c_char,
) -> i32 {
    guard(|| match (handle(ptr), cstr(task_id), cstr(status)) {
        (Some(h), Some(task_id), Some(status)) => h.0.send(Cmd::Submit {
            task_id: task_id.to_string(),
            status: status.to_string(),
            detail: cstr(detail).unwrap_or("").to_string(),
        }),
        _ => -1,
    })
}

#[no_mangle]
pub extern "C" fn eryu_get_hotwords(ptr: *mut EryuCore) -> *mut c_char {
    match handle(ptr) {
        Some(h) => owned(h.0.hotwords_json()),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn eryu_set_hotwords(
    ptr: *mut EryuCore,
    json: *const c_char,
    err_out: *mut *mut c_char,
) -> i32 {
    guard(|| {
        let Some(h) = handle(ptr) else { return -1 };
        let Some(text) = cstr(json) else {
            unsafe { write_out(err_out, Some("热词表为空".into())) };
            return -1;
        };
        match h.0.set_hotwords_json(text) {
            Ok(()) => 0,
            Err(e) => {
                unsafe { write_out(err_out, Some(e)) };
                -3
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn eryu_polish_test(ptr: *mut EryuCore, err_out: *mut *mut c_char) -> *mut c_char {
    match handle(ptr) {
        Some(h) => match h.0.polish_test() {
            Ok(text) => owned(text),
            Err(e) => {
                unsafe { write_out(err_out, Some(e)) };
                std::ptr::null_mut()
            }
        },
        None => {
            unsafe { write_out(err_out, Some("core 未初始化".into())) };
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn eryu_drop_last_result(ptr: *mut EryuCore) -> i32 {
    guard(|| {
        let Some(h) = handle(ptr) else { return -1 };
        h.0.clear_pending_result_files();
        0
    })
}

/// I-156：清掉内存里的「上次识别结果」，测麦克风前后用，避免把启动时恢复的旧文字当成刚听到的。
#[no_mangle]
pub extern "C" fn eryu_clear_last_result(ptr: *mut EryuCore) -> i32 {
    guard(|| {
        let Some(h) = handle(ptr) else { return -1 };
        h.0.clear_last_result();
        0
    })
}

#[no_mangle]
pub extern "C" fn eryu_get_last_result(ptr: *mut EryuCore) -> *mut c_char {
    match handle(ptr).and_then(|h| h.0.last_result()) {
        Some(t) => owned(t),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn eryu_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    // SAFETY: 由本库的 CString::into_raw 产生
    unsafe { drop(CString::from_raw(s)) };
}

// ======================== v1.1 ========================

#[no_mangle]
pub extern "C" fn eryu_get_config_toml(ptr: *mut EryuCore) -> *mut c_char {
    match handle(ptr) {
        Some(h) => owned(h.0.config_snapshot().to_toml()),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn eryu_set_config_toml(
    ptr: *mut EryuCore,
    toml: *const c_char,
    err_out: *mut *mut c_char,
) -> i32 {
    guard(|| {
        let Some(h) = handle(ptr) else { return -1 };
        let Some(text) = cstr(toml) else {
            unsafe { write_out(err_out, Some("配置文本为空".into())) };
            return -1;
        };
        let base = h.0.config_snapshot();
        match crate::config::Config::validate_toml(&base, text) {
            Ok(cfg) => {
                // 走统一入口：写共享快照 + 带版本号，避免排队中的旧配置把它改回去
                let code = h.0.apply_config_now(cfg);
                if code != 0 {
                    unsafe { write_out(err_out, Some("核心已关机".into())) };
                }
                code
            }
            Err(e) => {
                unsafe { write_out(err_out, Some(e)) };
                -3
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn eryu_get_config_json(ptr: *mut EryuCore) -> *mut c_char {
    match handle(ptr) {
        Some(h) => owned(h.0.config_snapshot().to_json()),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn eryu_set_config_json(
    ptr: *mut EryuCore,
    json: *const c_char,
    err_out: *mut *mut c_char,
) -> i32 {
    guard(|| {
        let Some(h) = handle(ptr) else { return -1 };
        let Some(text) = cstr(json) else {
            unsafe { write_out(err_out, Some("配置内容为空".into())) };
            return -1;
        };
        let mut next = h.0.config_snapshot();
        match next.merge_json(text) {
            Ok(()) => {
                let code = h.0.apply_config_now(next);
                if code != 0 {
                    unsafe { write_out(err_out, Some("核心已关机".into())) };
                }
                code
            }
            Err(e) => {
                unsafe { write_out(err_out, Some(e)) };
                -3
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn eryu_get_stats(ptr: *mut EryuCore) -> *mut c_char {
    match handle(ptr) {
        Some(h) => owned(h.0.store_json_stats()),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn eryu_get_history(ptr: *mut EryuCore, limit: i32) -> *mut c_char {
    match handle(ptr) {
        Some(h) => owned(h.0.store_json_history(limit)),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn eryu_delete_history(ptr: *mut EryuCore, id: *const c_char) -> i32 {
    guard(|| match (handle(ptr), cstr(id)) {
        (Some(h), Some(id)) => h.0.store_delete(id),
        _ => -1,
    })
}

#[no_mangle]
pub extern "C" fn eryu_clear_history(ptr: *mut EryuCore) -> i32 {
    guard(|| match handle(ptr) {
        Some(h) => h.0.store_clear(),
        None => -1,
    })
}

#[no_mangle]
pub extern "C" fn eryu_retry_failed(ptr: *mut EryuCore) -> i32 {
    guard(|| match handle(ptr) {
        Some(h) => h.0.send(Cmd::RetryFailed),
        None => -1,
    })
}

/// 可重试的失败段序号（JSON 数组，如 `[1,3]`）
#[no_mangle]
pub extern "C" fn eryu_failed_segments(ptr: *mut EryuCore) -> *mut c_char {
    match handle(ptr) {
        Some(h) => owned(serde_json::json!(h.0.failed_indexes()).to_string()),
        None => std::ptr::null_mut(),
    }
}

/// 只重试某一段：0 已受理 / -1 参数 / -3 该段没有可重试音频
#[no_mangle]
pub extern "C" fn eryu_retry_segment(
    ptr: *mut EryuCore,
    index: i32,
    err_out: *mut *mut c_char,
) -> i32 {
    guard(|| {
        let Some(h) = handle(ptr) else { return -1 };
        if index < 0 {
            return -1;
        }
        match h.0.retry_segment(index as usize) {
            Ok(()) => 0,
            Err(e) => {
                unsafe { write_out(err_out, Some(e)) };
                -3
            }
        }
    })
}

/// 自测：把 16k 单声道 WAV 当成一次录音走完整链路。0 受理 / -1 参数 / -3 读不出来
#[no_mangle]
pub extern "C" fn eryu_decode_wav(
    ptr: *mut EryuCore,
    path: *const c_char,
    err_out: *mut *mut c_char,
) -> i32 {
    guard(|| {
        let Some(h) = handle(ptr) else { return -1 };
        let Some(p) = cstr(path) else { return -1 };
        match h.0.decode_wav(&p) {
            Ok(()) => 0,
            Err(e) => {
                unsafe { write_out(err_out, Some(e)) };
                -3
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn eryu_models_list(ptr: *mut EryuCore) -> *mut c_char {
    match handle(ptr) {
        Some(h) => owned(h.0.models_list_json()),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn eryu_set_engine(ptr: *mut EryuCore, id: *const c_char) -> i32 {
    guard(|| match (handle(ptr), cstr(id)) {
        (Some(h), Some(id)) => {
            if !matches!(id, "sense" | "qwen3" | "stub") {
                return -1;
            }
            if id == "sense" && !h.0.config_snapshot().models_ready() {
                return -4;
            }
            if id == "qwen3"
                && !h
                    .0
                    .config_snapshot()
                    .qwen3_dir
                    .join("encoder.int8.onnx")
                    .exists()
            {
                return -4;
            }
            h.0.send(Cmd::SetEngine(id.to_string()))
        }
        _ => -1,
    })
}

#[no_mangle]
pub extern "C" fn eryu_meter_start(ptr: *mut EryuCore) -> i32 {
    guard(|| match handle(ptr) {
        Some(h) => h.0.send(Cmd::MeterStart),
        None => -1,
    })
}

#[no_mangle]
pub extern "C" fn eryu_meter_stop(ptr: *mut EryuCore) -> i32 {
    guard(|| match handle(ptr) {
        Some(h) => h.0.send(Cmd::MeterStop),
        None => -1,
    })
}

#[no_mangle]
pub extern "C" fn eryu_diagnose(ptr: *mut EryuCore) -> *mut c_char {
    match handle(ptr) {
        Some(h) => owned(h.0.diagnose()),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn eryu_list_devices(ptr: *mut EryuCore) -> *mut c_char {
    match handle(ptr) {
        Some(h) => owned(h.0.devices_json()),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn eryu_note_foreground_app(ptr: *mut EryuCore, name: *const c_char) -> i32 {
    guard(|| match (handle(ptr), cstr(name)) {
        (Some(h), Some(name)) => h.0.send(Cmd::NoteApp(name.to_string())),
        _ => -1,
    })
}

// ---- 调试 ----

#[no_mangle]
pub extern "C" fn eryu_debug_submit_text(ptr: *mut EryuCore, text: *const c_char) -> i32 {
    guard(|| match (handle(ptr), cstr(text)) {
        (Some(h), Some(text)) => {
            let _ = h.0.send(Cmd::DebugText(text.to_string()));
            0
        }
        _ => -1,
    })
}

#[no_mangle]
pub extern "C" fn eryu_debug_panic(ptr: *mut EryuCore) -> i32 {
    guard(|| match handle(ptr) {
        Some(h) => h.0.send(Cmd::PanicTest),
        None => -1,
    })
}

#[no_mangle]
pub extern "C" fn eryu_debug_record(ptr: *mut EryuCore, seconds: i32) -> i32 {
    guard(|| match handle(ptr) {
        Some(h) => h.0.send(Cmd::DebugRecord(seconds as f32)),
        None => -1,
    })
}

/// v1.4：选区指令改写（同步；壳在后台队列里调用）
///
/// 成功返回改写后的文字（需 `eryu_string_free`），失败返回 NULL 并写 `*err_out`。
#[no_mangle]
pub extern "C" fn eryu_run_command(
    ptr: *mut EryuCore,
    instruction: *const c_char,
    text: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    let Some(h) = handle(ptr) else {
        unsafe { write_out(err_out, Some("core 未初始化".into())) };
        return std::ptr::null_mut();
    };
    let Some(ins) = cstr(instruction) else {
        unsafe { write_out(err_out, Some("指令不是合法 UTF-8".into())) };
        return std::ptr::null_mut();
    };
    let Some(txt) = cstr(text) else {
        unsafe { write_out(err_out, Some("文本不是合法 UTF-8".into())) };
        return std::ptr::null_mut();
    };
    match catch_unwind(AssertUnwindSafe(|| h.0.run_command(ins, txt))) {
        Ok(Ok(out)) => owned(out),
        Ok(Err(e)) => {
            unsafe { write_out(err_out, Some(e)) };
            std::ptr::null_mut()
        }
        Err(_) => {
            unsafe { write_out(err_out, Some("内部错误（panic，已拦截）".into())) };
            std::ptr::null_mut()
        }
    }
}

/// v1.5：下载并安装模型。0=已开始，-1 参数，-3 未知 id，-4 已有下载，-5 已安装。
#[no_mangle]
pub extern "C" fn eryu_models_pull(
    ptr: *mut EryuCore,
    id: *const c_char,
    err_out: *mut *mut c_char,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(core) = handle(ptr) else { return -1 };
        let Some(id) = cstr(id) else { return -1 };
        let rc = core.0.models_pull(&id);
        if rc != 0 {
            let msg = match rc {
                -3 => format!("未知的模型：{id}"),
                -4 => "已经有模型在下载了，等它完成".to_string(),
                -5 => "这个模型已经装好了".to_string(),
                _ => "参数错误".to_string(),
            };
            unsafe { write_out(err_out, Some(msg)) };
        }
        rc
    }))
    .unwrap_or(-2)
}

// ---------- v1.6：润色 profile ----------

#[no_mangle]
pub extern "C" fn eryu_get_profiles(ptr: *mut EryuCore) -> *mut c_char {
    match handle(ptr) {
        Some(h) => owned(h.0.profiles_json()),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn eryu_set_profiles(
    ptr: *mut EryuCore,
    json: *const c_char,
    err_out: *mut *mut c_char,
) -> i32 {
    guard(|| {
        let Some(h) = handle(ptr) else { return -1 };
        let Some(text) = cstr(json) else {
            unsafe { write_out(err_out, Some("profile 列表为空".into())) };
            return -1;
        };
        match h.0.set_profiles_json(text) {
            Ok(()) => 0,
            Err(e) => {
                unsafe { write_out(err_out, Some(e)) };
                -3
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn eryu_set_active_profile(
    ptr: *mut EryuCore,
    name: *const c_char,
    err_out: *mut *mut c_char,
) -> i32 {
    guard(|| {
        let Some(h) = handle(ptr) else { return -1 };
        let Some(name) = cstr(name) else {
            unsafe { write_out(err_out, Some("profile 名为空".into())) };
            return -1;
        };
        match h.0.set_active_profile(name) {
            Ok(()) => 0,
            Err(e) => {
                unsafe { write_out(err_out, Some(e)) };
                -3
            }
        }
    })
}

// ---------- v1.10：用麦克风录一段当声纹 ----------
/// 同步阻塞 `seconds` 秒（默认 8），成功 0 / 参数 -1 / 失败 -3 / panic -2
#[no_mangle]
pub extern "C" fn eryu_voice_enroll_mic(
    ptr: *mut EryuCore,
    seconds: i32,
    err_out: *mut *mut c_char,
) -> i32 {
    guard(|| {
        let Some(h) = handle(ptr) else { return -1 };
        let secs = if seconds <= 0 { 8 } else { seconds as u32 };
        match h.0.voice_enroll_mic(secs) {
            Ok(_) => 0,
            Err(e) => {
                unsafe { write_out(err_out, Some(e)) };
                -3
            }
        }
    })
}

// ---------- v1.9：只录我的声音（声纹门控） ----------

#[no_mangle]
pub extern "C" fn eryu_voice_state(ptr: *mut EryuCore) -> *mut c_char {
    let Some(h) = handle(ptr) else {
        return std::ptr::null_mut();
    };
    owned(h.0.voice_state_json())
}

#[no_mangle]
pub extern "C" fn eryu_voice_enroll_wav(
    ptr: *mut EryuCore,
    wav_path: *const c_char,
    err_out: *mut *mut c_char,
) -> i32 {
    guard(|| {
        let Some(h) = handle(ptr) else { return -1 };
        let Some(path) = cstr(wav_path) else {
            unsafe { write_out(err_out, Some("WAV 路径为空".into())) };
            return -1;
        };
        match h.0.voice_enroll_wav(path) {
            Ok(_) => 0,
            Err(e) => {
                unsafe { write_out(err_out, Some(e)) };
                -3
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn eryu_voice_clear(ptr: *mut EryuCore, err_out: *mut *mut c_char) -> i32 {
    guard(|| {
        let Some(h) = handle(ptr) else { return -1 };
        match h.0.voice_clear() {
            Ok(()) => 0,
            Err(e) => {
                unsafe { write_out(err_out, Some(e)) };
                -3
            }
        }
    })
}

/// v1.14：上传诊断包（DESIGN §12.8）。成功 0，失败 -1（空指针）/ -3（原因写入 err_out）。
#[no_mangle]
pub extern "C" fn eryu_upload_diag(
    ptr: *mut EryuCore,
    url: *const c_char,
    body: *const c_char,
    detail_out: *mut *mut c_char,
) -> i32 {
    guard(|| {
        let Some(h) = handle(ptr) else { return -1 };
        let Some(url) = cstr(url) else {
            unsafe { write_out(detail_out, Some("上传地址为空".to_string())) };
            return -3;
        };
        let Some(body) = cstr(body) else {
            unsafe { write_out(detail_out, Some("诊断包内容为空".to_string())) };
            return -3;
        };
        match h.0.upload_diag(url, body) {
            Ok(text) => {
                unsafe { write_out(detail_out, Some(text)) };
                0
            }
            Err(e) => {
                unsafe { write_out(detail_out, Some(e)) };
                -3
            }
        }
    })
}

/// v1.11：自测模式（不写历史、不落盘）。0 = 已设置，-1 = 空指针。
#[no_mangle]
pub extern "C" fn eryu_set_test_mode(ptr: *mut EryuCore, on: i32) -> i32 {
    let Some(core) = handle(ptr) else { return -1 };
    core.0.set_test_mode(on != 0);
    0
}

/// 最近 40 条局域网请求摘要（不记内容）：JSON 数组。
/// 返回的串由调用方 `eryu_string_free`。
#[no_mangle]
pub extern "C" fn eryu_lan_log(ptr: *mut EryuCore) -> *mut c_char {
    let Some(core) = handle(ptr) else {
        return std::ptr::null_mut();
    };
    let _ = core;
    owned(crate::lan::recent_json())
}

/// v1.13：问润色服务端有哪些模型（用已保存的 Key）。
/// 成功返回 JSON 数组字符串（调用方 `eryu_string_free`）；失败返回 NULL 并写 err_out。
#[no_mangle]
pub extern "C" fn eryu_polish_models(ptr: *mut EryuCore, err_out: *mut *mut c_char) -> *mut c_char {
    if err_out.is_null() {
        return std::ptr::null_mut();
    }
    unsafe { *err_out = std::ptr::null_mut() };
    let Some(core) = handle(ptr) else {
        unsafe { write_out(err_out, Some("参数错误：core 为空".to_string())) };
        return std::ptr::null_mut();
    };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| core.0.polish_models())) {
        Ok(Ok(list)) => owned(serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())),
        Ok(Err(e)) => {
            unsafe { write_out(err_out, Some(e)) };
            std::ptr::null_mut()
        }
        Err(_) => {
            unsafe {
                write_out(err_out, Some("内部错误：取模型列表时崩了".to_string()))
            };
            std::ptr::null_mut()
        }
    }
}
