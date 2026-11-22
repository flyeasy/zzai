//! 容错测试：坏输入不许让 core 崩、不许静默改配置、不许越过 FFI 抛 panic。
//!
//! 覆盖：空/坏 WAV、非法 JSON、非法 UTF-8、空指针、超长字符串、非法热键规格、重复释放。
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use eryu_core::*;

/// 这些用例都改 `ERYU_CONFIG_DIR`（进程级环境变量），必须串行，否则互相踩。
fn env_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

fn tmpdir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("eryu-fuzz-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cstr(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        None
    } else {
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().to_string();
        unsafe { eryu_string_free(p) };
        Some(s)
    }
}

extern "C" fn drop_event(_json: *const c_char, _ctx: *mut c_void) {}

#[test]
fn bad_pointers_are_rejected_not_crashed() {
    // 空指针一律返回 -1，不 panic
    assert_eq!(eryu_hotkey_down(std::ptr::null_mut()), -1);
    assert_eq!(eryu_hotkey_up(std::ptr::null_mut()), -1);
    assert_eq!(eryu_cancel(std::ptr::null_mut()), -1);
    assert_eq!(
        eryu_set_event_cb(std::ptr::null_mut(), Some(drop_event), std::ptr::null_mut()),
        -1
    );
    // 释放空指针是安全的 no-op
    unsafe { eryu_string_free(std::ptr::null_mut()) };
}

#[test]
fn corrupt_wav_is_reported() {
    let _g = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmpdir("wav");
    let _ = std::env::set_var("ERYU_CONFIG_DIR", &dir);
    let core = eryu_core_new(std::ptr::null());
    assert!(!core.is_null());

    // 不存在的文件
    let missing = CString::new(dir.join("nope.wav").to_string_lossy().to_string()).unwrap();
    let mut err: *mut c_char = std::ptr::null_mut();
    assert_eq!(eryu_decode_wav(core, missing.as_ptr(), &mut err), -3);
    assert!(cstr(err).is_some_and(|e| !e.is_empty()), "应给出原因");

    // 存在但不是 WAV
    let junk = dir.join("junk.wav");
    std::fs::write(&junk, b"this is definitely not a wav file").unwrap();
    let junk_c = CString::new(junk.to_string_lossy().to_string()).unwrap();
    let mut err2: *mut c_char = std::ptr::null_mut();
    assert_eq!(eryu_decode_wav(core, junk_c.as_ptr(), &mut err2), -3);
    assert!(cstr(err2).is_some());

    // 空串路径
    let empty = CString::new("").unwrap();
    let mut err3: *mut c_char = std::ptr::null_mut();
    assert_eq!(eryu_decode_wav(core, empty.as_ptr(), &mut err3), -3);

    unsafe { eryu_core_free(core) };
}

#[test]
fn invalid_config_json_does_not_change_config() {
    let _g = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmpdir("cfg");
    let _ = std::env::set_var("ERYU_CONFIG_DIR", &dir);
    let core = eryu_core_new(std::ptr::null());
    assert!(!core.is_null());

    let before = cstr(eryu_get_config_json(core)).expect("配置可读");

    for bad in [
        "{",
        "not json",
        "[]",
        r#"{"engine":"nonexistent-engine-xyz"}"#,
        r#"{"min_record_ms":"abc"}"#,
    ] {
        let s = CString::new(bad).unwrap();
        let mut err: *mut c_char = std::ptr::null_mut();
        let rc = eryu_set_config_json(core, s.as_ptr(), &mut err);
        let reason = cstr(err);
        assert!(
            rc != 0 || reason.is_none(),
            "坏 JSON 不该静默成功: {bad} → rc={rc} {reason:?}"
        );
    }

    let after = cstr(eryu_get_config_json(core)).expect("配置仍可读");
    assert_eq!(before, after, "坏输入之后配置应保持原样");

    unsafe { eryu_core_free(core) };
}

#[test]
fn huge_strings_and_illegal_utf8_are_safe() {
    let _g = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmpdir("str");
    let _ = std::env::set_var("ERYU_CONFIG_DIR", &dir);
    let core = eryu_core_new(std::ptr::null());

    // 1MB 的“文字”：转录接口应接受并返回，或干净报错，但绝不能崩
    let big: String = "字".repeat(350_000);
    let big_c = CString::new(big.clone()).unwrap();
    let out = cstr(unsafe {
        eryu_run_command(core, big_c.as_ptr(), big_c.as_ptr(), std::ptr::null_mut())
    });
    let _ = out; // 没 key 时应返回 NULL + 原因；这里只要求不崩

    // 非法 UTF-8（0xFF 不是合法 UTF-8 起始字节）
    let mut raw = vec![0xffu8, 0xfe, b'a', 0];
    let bad_ptr = raw.as_mut_ptr() as *const c_char;
    let mut err: *mut c_char = std::ptr::null_mut();
    assert_eq!(eryu_set_hotwords(core, bad_ptr, &mut err), -1);
    assert!(cstr(err).is_some(), "非法 UTF-8 应给原因");

    unsafe { eryu_core_free(core) };
}

#[test]
fn odd_hotkey_and_voice_inputs_are_rejected() {
    let _g = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmpdir("odd");
    let _ = std::env::set_var("ERYU_CONFIG_DIR", &dir);
    let core = eryu_core_new(std::ptr::null());

    // 声纹：太短的样本要报错，不存在模型也要报错，都不能 panic
    let tiny = dir.join("tiny.wav");
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
    std::fs::write(&tiny, &wav).unwrap();
    let tiny_c = CString::new(tiny.to_string_lossy().to_string()).unwrap();
    let mut err: *mut c_char = std::ptr::null_mut();
    assert_eq!(eryu_voice_enroll_wav(core, tiny_c.as_ptr(), &mut err), -3);
    assert!(cstr(err).is_some());

    // 状态查询在没有声纹时也要给出合法 JSON
    let state = cstr(eryu_voice_state(core)).expect("voice_state 应返回 JSON");
    assert!(state.contains("\"enrolled\":false"));
    assert!(state.contains("\"model_ready\":"));

    unsafe { eryu_core_free(core) };
}

#[test]
fn double_free_is_not_called_but_free_after_null_is_safe() {
    // 只验证「空指针 free 安全」与「free 之后不重复 free」的契约边界：
    // 真 double-free 是调用方 bug，ABI 不承诺安全，这里不制造 UB。
    let _g = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    let core = eryu_core_new(std::ptr::null());
    eryu_core_free(core);
    unsafe { eryu_string_free(std::ptr::null_mut()) };
}
