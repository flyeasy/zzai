//! 动态加载 eryu_core.dll（C ABI v1.13），壳与 core 之间只有 JSON 事件与少量命令。

use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::Mutex;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

type FnCoreNew = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type FnCoreFree = unsafe extern "C" fn(*mut c_void);
type FnSetCb = unsafe extern "C" fn(*mut c_void, EryuEventCb, *mut c_void) -> i32;
type FnAbi = unsafe extern "C" fn() -> u32;
type FnHotkey = unsafe extern "C" fn(*mut c_void) -> i32;
type FnSubmit = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char) -> i32;
type FnStr = unsafe extern "C" fn(*mut c_void) -> *mut c_char;
type FnFreeStr = unsafe extern "C" fn(*mut c_char);
type FnDebugRecord = unsafe extern "C" fn(*mut c_void, i32) -> i32;
type FnDebugText = unsafe extern "C" fn(*mut c_void, *const c_char) -> i32;
type FnNoteApp = unsafe extern "C" fn(*mut c_void, *const c_char) -> i32;
type FnPull = unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_char) -> i32;
type FnFailedSegments = unsafe extern "C" fn(*mut c_void) -> *mut c_char;
type FnRetrySegment = unsafe extern "C" fn(*mut c_void, i32, *mut *mut c_char) -> i32;
type FnRunCommand =
    unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *mut *mut c_char) -> *mut c_char;
type FnWav = unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_char) -> i32;
type FnSetJson = unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_char) -> i32;

pub type EryuEventCb = Option<unsafe extern "C" fn(*const c_char, *mut c_void)>;

/// CLI 模式的事件落点（没有窗口循环，也要能看见 model_progress/done）
pub static CLI_EVENT: Mutex<String> = Mutex::new(String::new());

unsafe extern "C" fn cli_on_event(json: *const c_char, _ctx: *mut c_void) {
    if json.is_null() {
        return;
    }
    let s = CStr::from_ptr(json).to_string_lossy().into_owned();
    if let Ok(mut g) = CLI_EVENT.lock() {
        *g = s.clone();
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{}/eryu-events.log", crate::config_dir()))
    {
        use std::io::Write;
        let _ = writeln!(f, "{s}");
    }
    // CLI 自测也要把错误码记进诊断包（GUI 的那条路径在 main.rs 的 "error" 分支里）
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
        if v.get("type").and_then(|t| t.as_str()) == Some("error") {
            crate::diag::note_error(
                v.get("code").and_then(|c| c.as_str()).unwrap_or(""),
                v.get("message").and_then(|m| m.as_str()).unwrap_or(""),
            );
        }
    }
}

/// 编进 exe 的 core dll（构建脚本会把 dll 拷到 assets/ 再编译；没有就是空）
static EMBEDDED_CORE: &[u8] = include_bytes!("../assets/eryu_core.dll");

/// 编进 exe 的两个小模型（VAD 0.6MB、降噪 0.5MB）：不内置的话首次使用还得再下两个文件，
/// 用户看不懂「为什么要装四个模型」。大模型（语音识别）仍然按需下载。
static EMBEDDED_VAD: &[u8] = include_bytes!("../assets/silero_vad.onnx");
static EMBEDDED_GTCRN: &[u8] = include_bytes!("../assets/gtcrn_simple.onnx");

/// 把内置小模型解到 `{config_dir}/models/`（已存在且非空就跳过）。
pub fn ensure_bundled_models() {
    use std::io::Write;
    let dir = std::path::PathBuf::from(crate::config_dir()).join("models");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    for (name, data) in [("silero_vad.onnx", EMBEDDED_VAD), ("gtcrn_simple.onnx", EMBEDDED_GTCRN)] {
        let target = dir.join(name);
        let ok = std::fs::metadata(&target).map(|m| m.len() == data.len() as u64).unwrap_or(false);
        if ok {
            continue;
        }
        if let Ok(mut f) = std::fs::File::create(&target) {
            if f.write_all(data).is_ok() {
                crate::report(&format!("内置模型已解出：{}", target.display()));
            }
        }
    }
}

/// 确保磁盘上有一份和 exe 内置内容一致的 core dll，返回全路径。
/// 优先用 exe 同目录的 dll（开发/替换调试），否则解到 `%APPDATA%\Eryu\bin`。
fn ensure_core_dll() -> Result<String, String> {
    use std::io::Write;
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or_else(|| "拿不到 exe 所在目录".to_string())?;
    let local = exe_dir.join("eryu_core.dll");
    if local.is_file() {
        return Ok(local.to_string_lossy().into_owned());
    }
    if EMBEDDED_CORE.is_empty() {
        return Err("exe 里没有内置 eryu_core.dll，同目录也没有".to_string());
    }
    let dir = std::path::PathBuf::from(crate::config_dir()).join("bin");
    std::fs::create_dir_all(&dir).map_err(|e| format!("建目录失败：{e}"))?;
    // 指纹必须能代表内容：以前用「长度 + 首字节 + 末字节」，core 改了几百行长度不变、
    // 首末字节也不变，升级后仍加载旧 dll（实测 0.8.1 内部迭代时踩到，见 I-141）。
    let stamp = format!("{}-{:016x}", EMBEDDED_CORE.len(), fnv1a64(EMBEDDED_CORE));
    let target = dir.join("eryu_core.dll");
    let stamp_file = dir.join("eryu_core.stamp");
    let need_write = std::fs::read_to_string(&stamp_file)
        .map(|s| s.trim() != stamp)
        .unwrap_or(true)
        || !target.is_file();
    if need_write {
        let mut f = std::fs::File::create(&target).map_err(|e| format!("写 dll 失败：{e}"))?;
        f.write_all(EMBEDDED_CORE).map_err(|e| format!("写 dll 失败：{e}"))?;
        let _ = std::fs::write(&stamp_file, &stamp);
        crate::report(&format!("CORE 内置 dll 已解出：{}", target.display()));
    }
    Ok(target.to_string_lossy().into_owned())
}

/// FNV-1a 64：给内置 dll 算指纹（22MB 约 20ms，只算一次）
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 上传诊断包用的小句柄：只带三个不需要 &Core 生命周期的东西，可以送进后台线程。
#[derive(Clone, Copy)]
pub struct Uploader {
    handle: *mut c_void,
    f: unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        *const c_char,
        *mut *mut c_char,
    ) -> i32,
    free: FnFreeStr,
}

// SAFETY: 句柄指向核心实例内部共享状态（核心自身用 Mutex/Arc 保证跨线程）；
// FFI 层每个入口都 catch_unwind，且不要求调用发生在 UI 线程。
unsafe impl Send for Uploader {}

impl Uploader {
    pub fn upload(&self, url: &str, body: &str) -> (i32, String) {
        let u = std::ffi::CString::new(url).unwrap_or_default();
        let b = std::ffi::CString::new(body).unwrap_or_default();
        let mut out: *mut c_char = std::ptr::null_mut();
        let rc = unsafe { (self.f)(self.handle, u.as_ptr(), b.as_ptr(), &mut out) };
        let text = if out.is_null() {
            String::new()
        } else {
            let s = unsafe { CStr::from_ptr(out).to_string_lossy().into_owned() };
            unsafe { (self.free)(out) };
            s
        };
        (rc, text)
    }
}

pub struct Core {
    handle: *mut c_void,
    f_free: FnCoreFree,
    f_set_cb: FnSetCb,
    f_hotkey_down: FnHotkey,
    f_hotkey_up: FnHotkey,
    f_cancel: FnHotkey,
    f_submit: FnSubmit,
    f_last: FnStr,
    f_free_str: FnFreeStr,
    f_diagnose: FnStr,
    f_debug_record: FnDebugRecord,
    f_clear_last: unsafe extern "C" fn(*mut std::ffi::c_void) -> i32,
    f_debug_text: FnDebugText,
    f_note_app: FnNoteApp,
    f_pull: FnPull,
    f_decode_wav: FnWav,
    f_get_config: FnStr,
    f_set_config: FnSetJson,
    f_set_config_toml: FnSetJson,
    f_profiles: FnStr,
    f_run_command: FnRunCommand,
    f_failed_segments: FnFailedSegments,
    /// 删除一条历史（对齐 mac 历史列表的「删除」）
    f_delete_history: unsafe extern "C" fn(*mut c_void, *const std::ffi::c_char) -> i32,
    /// 重试上次失败的段（对齐 mac「重试上次失败的识别」）
    f_retry_failed: unsafe extern "C" fn(*mut c_void) -> i32,
    f_retry_segment: FnRetrySegment,
    f_models: FnStr,
    f_devices: FnStr,
    f_config_toml: FnStr,
    f_hotwords: FnStr,
    f_set_hotwords: FnSetJson,
    f_stats: FnStr,
    f_clear_history: unsafe extern "C" fn(*mut c_void) -> i32,
    f_history: unsafe extern "C" fn(*mut c_void, i32) -> *mut c_char,
    f_set_profiles: FnSetJson,
    f_set_active_profile: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
    f_meter_start: unsafe extern "C" fn(*mut c_void) -> i32,
    f_meter_stop: unsafe extern "C" fn(*mut c_void) -> i32,
    /// 声纹（只录我的声音）：登记 / 清空 / 状态
    f_voice_enroll_mic: unsafe extern "C" fn(*mut c_void, i32, *mut *mut std::ffi::c_char) -> i32,
    f_voice_clear: unsafe extern "C" fn(*mut c_void, *mut *mut std::ffi::c_char) -> i32,
    f_voice_state: unsafe extern "C" fn(*mut c_void) -> *mut std::ffi::c_char,
    /// 润色连通性测试（同步 HTTP，调用方应放到线程里）
    f_polish_test: unsafe extern "C" fn(*mut c_void, *mut *mut std::ffi::c_char) -> *mut std::ffi::c_char,
    /// 列出服务商可用模型（同步 HTTP）
    f_polish_models: unsafe extern "C" fn(*mut c_void, *mut *mut std::ffi::c_char) -> *mut std::ffi::c_char,
    f_set_test_mode: unsafe extern "C" fn(*mut c_void, i32) -> i32,
    f_lan_log: unsafe extern "C" fn(*mut c_void) -> *mut std::ffi::c_char,
    /// 用 WAV 登记（自测/无麦克风时用）
    f_voice_enroll_wav: unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut std::ffi::c_char) -> i32,
    /// 上传诊断包（DESIGN §12.8）
    f_upload_diag: unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        *const c_char,
        *mut *mut std::ffi::c_char,
    ) -> i32,
}

unsafe impl Send for Core {}
unsafe impl Sync for Core {}

/// 给后台线程用：拿裸句柄登记声纹（阻塞 8 秒，不能在 UI 线程里调）。
/// 函数指针在 `Core::new` 时存进静态，避免后台线程再碰 APP 锁。
type EnrollFn = unsafe extern "C" fn(*mut c_void, i32, *mut *mut c_char) -> i32;
type FreeStrFn = unsafe extern "C" fn(*mut c_char);
type PolishFn = unsafe extern "C" fn(*mut c_void, *mut *mut std::ffi::c_char) -> *mut std::ffi::c_char;
static POLISH_FN: Mutex<Option<PolishFn>> = Mutex::new(None);
static POLISH_MODELS_FN: Mutex<Option<PolishFn>> = Mutex::new(None);
static VOICE_ENROLL_FN: Mutex<Option<EnrollFn>> = Mutex::new(None);
static FREE_STR_FN: Mutex<Option<FreeStrFn>> = Mutex::new(None);

pub unsafe fn enroll_mic_raw(ptr: *mut c_void, secs: i32) -> (i32, String) {
    let f = { *crate::lock(&VOICE_ENROLL_FN) };
    let Some(f) = f else {
        return (-1, "声纹接口没初始化".to_string());
    };
    let mut err: *mut c_char = std::ptr::null_mut();
    let rc = f(ptr, secs, &mut err);
    let msg = if err.is_null() {
        String::new()
    } else {
        let s = CStr::from_ptr(err).to_string_lossy().into_owned();
        if let Some(free) = *crate::lock(&FREE_STR_FN) {
            free(err);
        }
        s
    };
    (rc, msg)
}


fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Core {
    pub fn load(dir: &str) -> Result<Self, String> {
        // 先把内置小模型（VAD / 降噪）解到数据目录：用户装完就能直接说话，不用再点两个下载。
        ensure_bundled_models();
        unsafe {
            // 绿色单文件：core 的 dll 被编进 exe，运行时解到本地再 LoadLibrary。
            // 开发时（exe 同目录已有 dll）直接用同目录那份，方便替换调试。
            let path = match ensure_core_dll() {
                Ok(p) => p,
                Err(e) => return Err(e),
            };
            let wpath = wide(&path);
            let module = LoadLibraryW(PCWSTR(wpath.as_ptr()))
                .map_err(|e| format!("LoadLibraryW({path}) 失败：{e}"))?;
            macro_rules! sym {
                ($name:literal, $ty:ty) => {{
                    let p = GetProcAddress(module, windows::core::PCSTR(concat!($name, "\0").as_ptr()));
                    match p {
                        Some(f) => std::mem::transmute::<_, $ty>(f),
                        None => return Err(format!("缺少导出 {}", $name)),
                    }
                }};
            }
            let abi: FnAbi = sym!("eryu_abi_version", FnAbi);
            let major = abi() >> 16;
            if major != 1 {
                return Err(format!("ABI 主版本不匹配：dll={major}，壳=1"));
            }
            let f_new: FnCoreNew = sym!("eryu_core_new", FnCoreNew);
            let f_free: FnCoreFree = sym!("eryu_core_free", FnCoreFree);
            let f_set_cb: FnSetCb = sym!("eryu_set_event_cb", FnSetCb);
            let f_hotkey_down: FnHotkey = sym!("eryu_hotkey_down", FnHotkey);
            let f_hotkey_up: FnHotkey = sym!("eryu_hotkey_up", FnHotkey);
            let f_cancel: FnHotkey = sym!("eryu_cancel", FnHotkey);
            let f_submit: FnSubmit = sym!("eryu_submit_result", FnSubmit);
            let f_last: FnStr = sym!("eryu_get_last_result", FnStr);
            let f_free_str: FnFreeStr = sym!("eryu_string_free", FnFreeStr);
            *crate::lock(&FREE_STR_FN) = Some(f_free_str);
            let f_diagnose: FnStr = sym!("eryu_diagnose", FnStr);
            let f_debug_record: FnDebugRecord = sym!("eryu_debug_record", FnDebugRecord);
            let f_clear_last: unsafe extern "C" fn(*mut std::ffi::c_void) -> i32 =
                sym!("eryu_clear_last_result", unsafe extern "C" fn(*mut std::ffi::c_void) -> i32);
            let f_debug_text: FnDebugText = sym!("eryu_debug_submit_text", FnDebugText);
            let f_note_app: FnNoteApp = sym!("eryu_note_foreground_app", FnNoteApp);
            let f_pull: FnPull = sym!("eryu_models_pull", FnPull);
            let f_decode_wav: FnWav = sym!("eryu_decode_wav", FnWav);
            let f_get_config: FnStr = sym!("eryu_get_config_json", FnStr);
            let f_set_config: FnSetJson = sym!("eryu_set_config_json", FnSetJson);
            let f_set_config_toml: FnSetJson = sym!("eryu_set_config_toml", FnSetJson);

            let c_dir = CString::new(dir).map_err(|e| e.to_string())?;
            let handle = f_new(c_dir.as_ptr());
            if handle.is_null() {
                return Err("核心初始化失败：eryu_core_new 返回空。\n最常见原因：已经有一份 Eryu 在运行、占着数据目录；其次是数据目录不可写或磁盘满。".into());
            }
            Ok(Self {
                handle,
                f_free,
                f_set_cb,
                f_hotkey_down,
                f_hotkey_up,
                f_cancel,
                f_submit,
                f_last,
                f_free_str,
                f_diagnose,
                f_debug_record,
                f_clear_last,
                f_debug_text,
                f_note_app,
                f_pull,
                f_decode_wav,
                f_get_config,
                f_set_config,
                f_set_config_toml,
                f_profiles: sym!("eryu_get_profiles", FnStr),
                f_run_command: sym!("eryu_run_command", FnRunCommand),
                f_failed_segments: sym!("eryu_failed_segments", FnFailedSegments),
                f_delete_history: sym!(
                    "eryu_delete_history",
                    unsafe extern "C" fn(*mut c_void, *const std::ffi::c_char) -> i32
                ),
                f_retry_failed: sym!("eryu_retry_failed", unsafe extern "C" fn(*mut c_void) -> i32),
                f_retry_segment: sym!("eryu_retry_segment", FnRetrySegment),
                f_models: sym!("eryu_models_list", FnStr),
                f_devices: sym!("eryu_list_devices", FnStr),
                f_config_toml: sym!("eryu_get_config_toml", FnStr),
                f_hotwords: sym!("eryu_get_hotwords", FnStr),
                f_set_hotwords: sym!("eryu_set_hotwords", FnSetJson),
                f_stats: sym!("eryu_get_stats", FnStr),
                f_clear_history: sym!("eryu_clear_history", unsafe extern "C" fn(*mut c_void) -> i32),
                f_history: sym!("eryu_get_history", unsafe extern "C" fn(*mut c_void, i32) -> *mut c_char),
                f_set_profiles: sym!("eryu_set_profiles", FnSetJson),
                f_set_active_profile: sym!("eryu_set_active_profile", unsafe extern "C" fn(*mut c_void, *const c_char) -> i32),
                f_meter_start: sym!("eryu_meter_start", unsafe extern "C" fn(*mut c_void) -> i32),
                f_meter_stop: sym!("eryu_meter_stop", unsafe extern "C" fn(*mut c_void) -> i32),
                f_voice_enroll_mic: sym!(
                    "eryu_voice_enroll_mic",
                    unsafe extern "C" fn(*mut c_void, i32, *mut *mut std::ffi::c_char) -> i32
                ),
                f_voice_clear: sym!(
                    "eryu_voice_clear",
                    unsafe extern "C" fn(*mut c_void, *mut *mut std::ffi::c_char) -> i32
                ),
                f_voice_state: sym!("eryu_voice_state", unsafe extern "C" fn(*mut c_void) -> *mut std::ffi::c_char),
                f_polish_test: sym!("eryu_polish_test", unsafe extern "C" fn(*mut c_void, *mut *mut std::ffi::c_char) -> *mut std::ffi::c_char),
                f_polish_models: sym!("eryu_polish_models", unsafe extern "C" fn(*mut c_void, *mut *mut std::ffi::c_char) -> *mut std::ffi::c_char),
                f_lan_log: sym!("eryu_lan_log", unsafe extern "C" fn(*mut c_void) -> *mut std::ffi::c_char),
                f_set_test_mode: sym!("eryu_set_test_mode", unsafe extern "C" fn(*mut c_void, i32) -> i32),
                f_voice_enroll_wav: sym!("eryu_voice_enroll_wav", unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut std::ffi::c_char) -> i32),
                f_upload_diag: sym!(
                    "eryu_upload_diag",
                    unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *mut *mut std::ffi::c_char) -> i32
                ),
            })
            .map(|c: Core| {
                *crate::lock(&VOICE_ENROLL_FN) = Some(c.f_voice_enroll_mic);
                *crate::lock(&POLISH_FN) = Some(c.f_polish_test);
                *crate::lock(&POLISH_MODELS_FN) = Some(c.f_polish_models);
                c
            })
        }
    }

    pub fn set_event_cb_cli(&self) {
        unsafe {
            (self.f_set_cb)(self.handle, Some(cli_on_event), std::ptr::null_mut());
        }
    }

    /// 拉模型时轮询：还在下载就返回 true
    pub fn pulling(&self) -> bool {
        let g = crate::lock(&CLI_EVENT);
        if g.contains("model_done") || g.contains("model_failed") {
            return false;
        }
        true
    }

    pub fn last_event(&self) -> String {
        crate::lock(&CLI_EVENT).clone()
    }

    pub fn set_event_cb(&self, hwnd: HWND) {
        unsafe {
            (self.f_set_cb)(self.handle, Some(on_event), hwnd.0 as *mut c_void);
        }
    }

    pub fn hotkey_down(&self) {
        unsafe { (self.f_hotkey_down)(self.handle); }
    }
    pub fn hotkey_up(&self) {
        unsafe { (self.f_hotkey_up)(self.handle); }
    }
    pub fn cancel(&self) {
        unsafe { (self.f_cancel)(self.handle); }
    }
    /// 当前激活的润色档名字（浮层徽标用）；读不到就返回空
    /// 当前润色档的（名字, 图标）。图标是 macOS 那侧的 SF Symbol 名（message/envelope/…），
    /// 浮层在 Windows 上把它映射成 emoji 画出来。
    pub fn active_profile_pair(&self) -> (String, String) {
        let json = unsafe {
            let p = (self.f_profiles)(self.handle);
            if p.is_null() {
                return (String::new(), String::new());
            }
            let s = std::ffi::CStr::from_ptr(p).to_string_lossy().to_string();
            (self.f_free_str)(p);
            s
        };
        let v: serde_json::Value = match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(_) => return (String::new(), String::new()),
        };
        let active = v["profiles"]
            .as_array()
            .and_then(|list| {
                list.iter()
                    .find(|p| p["active"].as_bool().unwrap_or(false))
                    .cloned()
            });
        match active {
            Some(p) => (
                p["name"].as_str().unwrap_or_default().to_string(),
                p["icon"].as_str().unwrap_or_default().to_string(),
            ),
            None => (
                v["active"].as_str().unwrap_or_default().to_string(),
                String::new(),
            ),
        }
    }

    pub fn note_app(&self, name: &str) {
        if let Ok(c) = CString::new(name) {
            unsafe { (self.f_note_app)(self.handle, c.as_ptr()); }
        }
    }
    pub fn submit(&self, task: &str, status: &str, detail: &str) {
        let (t, s, d) = (
            CString::new(task).unwrap_or_default(),
            CString::new(status).unwrap_or_default(),
            CString::new(detail).unwrap_or_default(),
        );
        unsafe { (self.f_submit)(self.handle, t.as_ptr(), s.as_ptr(), d.as_ptr()); }
    }
    pub fn last_result(&self) -> String {
        unsafe {
            let p = (self.f_last)(self.handle);
            if p.is_null() {
                return String::new();
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f_free_str)(p);
            s
        }
    }
    /// 当前配置（JSON 文本）
    pub fn hotwords_json(&self) -> String {
        unsafe {
            let p = (self.f_hotwords)(self.handle);
            if p.is_null() {
                return String::new();
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f_free_str)(p);
            s
        }
    }

    pub fn set_hotwords(&self, json: &str) -> (i32, String) {
        let Ok(c) = CString::new(json) else {
            return (-1, "热词里有非法字符".into());
        };
        unsafe {
            let mut err: *mut c_char = std::ptr::null_mut();
            let code = (self.f_set_hotwords)(self.handle, c.as_ptr(), &mut err);
            let msg = if err.is_null() {
                String::new()
            } else {
                let s = CStr::from_ptr(err).to_string_lossy().into_owned();
                (self.f_free_str)(err);
                s
            };
            (code, msg)
        }
    }

    pub fn models_json(&self) -> String {
        unsafe {
            let p = (self.f_models)(self.handle);
            if p.is_null() {
                return String::new();
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f_free_str)(p);
            s
        }
    }

    /// 用麦克风录 `secs` 秒登记声纹（阻塞；调用方应放到线程里）
    /// 读走 core 写出来的错误串（调用方负责释放）
    fn take_err(&self, p: *mut std::ffi::c_char) -> String {
        if p.is_null() {
            return String::new();
        }
        let s = unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() };
        unsafe { (self.f_free_str)(p) };
        s
    }

    /// 后台线程用：拿裸句柄（core 的 FFI 句柄），别跨线程用 &self
    pub fn raw_handle(&self) -> *mut c_void {
        self.handle
    }

    pub fn voice_enroll_mic(&self, secs: i32) -> (i32, String) {
        let mut err: *mut std::ffi::c_char = std::ptr::null_mut();
        let rc = unsafe { (self.f_voice_enroll_mic)(self.handle, secs, &mut err) };
        (rc, self.take_err(err))
    }

    /// 服务商可用模型（JSON 数组字符串；拿不到返回空 + 错误）
    /// 自测模式：识别结果只进内存，不写历史（core 侧 DESIGN §10.2 同款逻辑）。
    pub fn set_test_mode(&self, on: bool) -> i32 {
        unsafe { (self.f_set_test_mode)(self.handle, if on { 1 } else { 0 }) }
    }

    pub fn polish_models(&self) -> (Vec<String>, String) {
        let mut err: *mut std::ffi::c_char = std::ptr::null_mut();
        let p = unsafe { (self.f_polish_models)(self.handle, &mut err) };
        let err = self.take_err(err);
        if p.is_null() {
            return (Vec::new(), err);
        }
        let s = unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() };
        unsafe { (self.f_free_str)(p) };
        let list = serde_json::from_str::<Vec<String>>(&s).unwrap_or_default();
        (list, err)
    }

    pub fn polish_test(&self) -> (String, String) {
        let mut err: *mut std::ffi::c_char = std::ptr::null_mut();
        let p = unsafe { (self.f_polish_test)(self.handle, &mut err) };
        let text = if p.is_null() {
            String::new()
        } else {
            let s = unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() };
            unsafe { (self.f_free_str)(p) };
            s
        };
        (text, self.take_err(err))
    }

    pub fn voice_enroll_wav(&self, path: &str) -> (i32, String) {
        let c = std::ffi::CString::new(path).unwrap_or_default();
        let mut err: *mut std::ffi::c_char = std::ptr::null_mut();
        let rc = unsafe { (self.f_voice_enroll_wav)(self.handle, c.as_ptr(), &mut err) };
        (rc, self.take_err(err))
    }

    /// 上传诊断包：返回 (rc, 服务端响应或原因)。同步 HTTP，调用方应放到线程里。
    pub fn upload_diag(&self, url: &str, body: &str) -> (i32, String) {
        let u = std::ffi::CString::new(url).unwrap_or_default();
        let b = std::ffi::CString::new(body).unwrap_or_default();
        let mut err: *mut std::ffi::c_char = std::ptr::null_mut();
        let rc = unsafe { (self.f_upload_diag)(self.handle, u.as_ptr(), b.as_ptr(), &mut err) };
        (rc, self.take_err(err))
    }

    /// 取出可跨线程的上传句柄（诊断包上传在后台线程做，避免卡 UI）。
    pub fn uploader(&self) -> Uploader {
        Uploader {
            handle: self.handle,
            f: self.f_upload_diag,
            free: self.f_free_str,
        }
    }

    pub fn voice_clear(&self) -> (i32, String) {
        let mut err: *mut std::ffi::c_char = std::ptr::null_mut();
        let rc = unsafe { (self.f_voice_clear)(self.handle, &mut err) };
        (rc, self.take_err(err))
    }

    pub fn voice_state(&self) -> String {
        let p = unsafe { (self.f_voice_state)(self.handle) };
        if p.is_null() {
            return String::new();
        }
        let s = unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() };
        unsafe { (self.f_free_str)(p) };
        s
    }

    /// 只开麦克风看电平（不录音、不落盘）；事件走 "meter"
    pub fn meter_start(&self) -> i32 {
        unsafe { (self.f_meter_start)(self.handle) }
    }

    pub fn meter_stop(&self) -> i32 {
        unsafe { (self.f_meter_stop)(self.handle) }
    }

    pub fn devices_json(&self) -> String {
        unsafe {
            let p = (self.f_devices)(self.handle);
            if p.is_null() {
                return String::new();
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f_free_str)(p);
            s
        }
    }

    pub fn profiles_json(&self) -> String {
        unsafe {
            let p = (self.f_profiles)(self.handle);
            if p.is_null() {
                return String::new();
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f_free_str)(p);
            s
        }
    }

    /// 段级失败：可重试的段号
    pub fn failed_segments(&self) -> Vec<usize> {
        unsafe {
            let p = (self.f_failed_segments)(self.handle);
            if p.is_null() {
                return Vec::new();
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f_free_str)(p);
            serde_json::from_str::<Vec<usize>>(&s).unwrap_or_default()
        }
    }

    /// 只重跑某一段（结果通过 segment_result 事件回来）
    pub fn retry_segment(&self, index: i32) -> (i32, String) {
        unsafe {
            let mut err: *mut c_char = std::ptr::null_mut();
            let rc = (self.f_retry_segment)(self.handle, index, &mut err);
            let e = if err.is_null() {
                String::new()
            } else {
                let s = CStr::from_ptr(err).to_string_lossy().into_owned();
                (self.f_free_str)(err);
                s
            };
            (rc, e)
        }
    }

    /// 选区改写：让 core 按指令重写一段文字（同步，会走网络）。
    pub fn run_command(&self, instruction: &str, text: &str) -> Result<String, String> {
        let ins = std::ffi::CString::new(instruction).unwrap_or_default();
        let txt = std::ffi::CString::new(text).unwrap_or_default();
        unsafe {
            let mut err: *mut c_char = std::ptr::null_mut();
            let p = (self.f_run_command)(self.handle, ins.as_ptr(), txt.as_ptr(), &mut err);
            let e = if err.is_null() {
                String::new()
            } else {
                let s = CStr::from_ptr(err).to_string_lossy().into_owned();
                (self.f_free_str)(err);
                s
            };
            if p.is_null() {
                return Err(if e.is_empty() { "改写失败".into() } else { e });
            }
            let out = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f_free_str)(p);
            Ok(out)
        }
    }

    pub fn lan_log(&self) -> String {
        unsafe {
            let p = (self.f_lan_log)(self.handle);
            if p.is_null() {
                return String::new();
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f_free_str)(p);
            s
        }
    }

    pub fn stats_json(&self) -> String {
        unsafe {
            let p = (self.f_stats)(self.handle);
            if p.is_null() {
                return String::new();
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f_free_str)(p);
            s
        }
    }

    pub fn clear_history(&self) -> i32 {
        unsafe { (self.f_clear_history)(self.handle) }
    }

    /// 删除一条历史；返回 (code, err)
    pub fn delete_history(&self, id: &str) -> (i32, String) {
        let c = std::ffi::CString::new(id).unwrap_or_default();
        let rc = unsafe { (self.f_delete_history)(self.handle, c.as_ptr()) };
        (rc, String::new())
    }

    /// 重试上次失败的段
    pub fn retry_failed(&self) -> i32 {
        unsafe { (self.f_retry_failed)(self.handle) }
    }

    pub fn history_json(&self, limit: i32) -> String {
        unsafe {
            let p = (self.f_history)(self.handle, limit);
            if p.is_null() {
                return String::new();
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f_free_str)(p);
            s
        }
    }

    pub fn set_profiles(&self, json: &str) -> (i32, String) {
        let Ok(c) = CString::new(json) else {
            return (-1, "档位里有非法字符".into());
        };
        unsafe {
            let mut err: *mut c_char = std::ptr::null_mut();
            let code = (self.f_set_profiles)(self.handle, c.as_ptr(), &mut err);
            let msg = if err.is_null() {
                String::new()
            } else {
                let s = CStr::from_ptr(err).to_string_lossy().into_owned();
                (self.f_free_str)(err);
                s
            };
            (code, msg)
        }
    }

    pub fn set_active_profile(&self, name: &str) -> i32 {
        if let Ok(c) = CString::new(name) {
            unsafe { (self.f_set_active_profile)(self.handle, c.as_ptr()) }
        } else {
            -1
        }
    }

    /// 整份 TOML 写回（mac 侧 `setConfigToml` 同一入口）；返回 (code, err)
    pub fn set_config_toml(&self, toml: &str) -> (i32, String) {
        let c = std::ffi::CString::new(toml).unwrap_or_default();
        let mut err: *mut std::ffi::c_char = std::ptr::null_mut();
        let code = unsafe { (self.f_set_config_toml)(self.handle, c.as_ptr(), &mut err) };
        (code, self.take_err(err))
    }

    pub fn config_toml(&self) -> String {
        unsafe {
            let p = (self.f_config_toml)(self.handle);
            if p.is_null() {
                return String::new();
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f_free_str)(p);
            s
        }
    }

    pub fn config_json(&self) -> String {
        unsafe {
            let p = (self.f_get_config)(self.handle);
            if p.is_null() {
                return String::new();
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f_free_str)(p);
            s
        }
    }

    /// 写回配置（JSON 里只放要改的键），返回 (code, err)
    pub fn set_config(&self, json: &str) -> (i32, String) {
        let Ok(c) = CString::new(json) else {
            return (-1, "配置里有非法字符".into());
        };
        unsafe {
            let mut err: *mut c_char = std::ptr::null_mut();
            let code = (self.f_set_config)(self.handle, c.as_ptr(), &mut err);
            let msg = if err.is_null() {
                String::new()
            } else {
                let s = CStr::from_ptr(err).to_string_lossy().into_owned();
                (self.f_free_str)(err);
                s
            };
            (code, msg)
        }
    }

    pub fn diagnose(&self) -> String {
        unsafe {
            let p = (self.f_diagnose)(self.handle);
            if p.is_null() {
                return String::new();
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f_free_str)(p);
            s
        }
    }
    /// 返回 (code, 错误说明)
    pub fn decode_wav(&self, path: &str) -> (i32, String) {
        let c = CString::new(path).unwrap_or_default();
        let mut err: *mut c_char = std::ptr::null_mut();
        let code = unsafe { (self.f_decode_wav)(self.handle, c.as_ptr(), &mut err) };
        let msg = if err.is_null() { String::new() } else { unsafe {
            let s = CStr::from_ptr(err).to_string_lossy().into_owned();
            (self.f_free_str)(err);
            s } };
        (code, msg)
    }

    pub fn pull_model(&self, id: &str) -> (i32, String) {
        let c = CString::new(id).unwrap_or_default();
        let mut err: *mut c_char = std::ptr::null_mut();
        let code = unsafe { (self.f_pull)(self.handle, c.as_ptr(), &mut err) };
        let msg = if err.is_null() {
            String::new()
        } else {
            let s = unsafe { CStr::from_ptr(err).to_string_lossy().into_owned() };
            unsafe { (self.f_free_str)(err) };
            s
        };
        (code, msg)
    }

    pub fn debug_record(&self, secs: i32) {
        unsafe { (self.f_debug_record)(self.handle, secs); }
    }

    /// 测麦克风前清掉「上次识别结果」（I-156）：否则启动时恢复的旧文字会被当成刚听到的
    pub fn clear_last_result(&self) {
        unsafe {
            (self.f_clear_last)(self.handle);
        }
    }
    pub fn debug_submit_text(&self, text: &str) {
        if let Ok(c) = CString::new(text) {
            unsafe { (self.f_debug_text)(self.handle, c.as_ptr()); }
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        unsafe { (self.f_free)(self.handle); }
    }
}

unsafe extern "C" fn on_event(json: *const c_char, ctx: *mut c_void) {
    if json.is_null() {
        return;
    }
    let s = CStr::from_ptr(json).to_string_lossy().into_owned();
    let hwnd = HWND(ctx as _);
    // 事件字符串交给 UI 线程释放（原来 mem::forget 每条事件泄漏一块，长时间跑会涨内存）。
    let boxed: Box<[u16]> = s.encode_utf16().collect();
    let len = boxed.len();
    let ptr = Box::into_raw(boxed) as *const u16;
    let ok = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
        hwnd,
        crate::WM_ERYU_EVENT,
        windows::Win32::Foundation::WPARAM(len),
        windows::Win32::Foundation::LPARAM(ptr as isize),
    );
    if ok.is_err() {
        // 投递失败（队列满/窗口已销毁）就地释放，不能泄漏
        unsafe { drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u16, len))) };
    }
}

/// 给后台线程用：列服务商模型（同步 HTTP，别在 UI 线程调）
pub unsafe fn polish_models_raw(ptr: *mut c_void) -> (Vec<String>, String) {
    let f = { *crate::lock(&POLISH_MODELS_FN) };
    let Some(f) = f else {
        return (Vec::new(), "润色接口没初始化".to_string());
    };
    let free = { *crate::lock(&FREE_STR_FN) };
    let Some(free) = free else {
        return (Vec::new(), "内存释放接口没初始化".to_string());
    };
    let mut err: *mut std::ffi::c_char = std::ptr::null_mut();
    let p = f(ptr, &mut err);
    let e = if err.is_null() {
        String::new()
    } else {
        let s = CStr::from_ptr(err).to_string_lossy().into_owned();
        free(err);
        s
    };
    if p.is_null() {
        return (Vec::new(), e);
    }
    let raw = CStr::from_ptr(p).to_string_lossy().into_owned();
    free(p);
    (serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default(), e)
}

/// 给后台线程用：拿裸句柄跑一次润色连通性测试（同步 HTTP，别在 UI 线程调）
pub unsafe fn polish_test_raw(ptr: *mut c_void) -> (String, String) {
    let f = { *crate::lock(&POLISH_FN) };
    let Some(f) = f else {
        return (String::new(), "润色接口没初始化".to_string());
    };
    let free = { *crate::lock(&FREE_STR_FN) };
    let Some(free) = free else {
        return (String::new(), "内存释放接口没初始化".to_string());
    };
    let mut err: *mut std::ffi::c_char = std::ptr::null_mut();
    let p = f(ptr, &mut err);
    let text = if p.is_null() {
        String::new()
    } else {
        let s = CStr::from_ptr(p).to_string_lossy().into_owned();
        free(p);
        s
    };
    let e = if err.is_null() {
        String::new()
    } else {
        let s = CStr::from_ptr(err).to_string_lossy().into_owned();
        free(err);
        s
    };
    (text, e)
}
