//! FFI 冒烟：走一遍真实 ABI 路径（事件有序 / 提交回报 / 取回结果 / panic 不穿 FFI）
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use eryu_core::*;

fn sink() -> &'static Mutex<Vec<String>> {
    static S: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Vec::new()))
}

extern "C" fn on_event(json: *const c_char, _ctx: *mut c_void) {
    let s = unsafe { CStr::from_ptr(json) }
        .to_string_lossy()
        .to_string();
    sink().lock().unwrap().push(s);
}

/// 从事件池里取走第一条匹配的事件，其余保留（事件可能同一时刻成对到达）
fn wait_for(kind: &str, timeout: Duration) -> Option<String> {
    let needle = format!("\"type\":\"{kind}\"");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        {
            let mut g = sink().lock().unwrap();
            if let Some(pos) = g.iter().position(|e| e.contains(&needle)) {
                return Some(g.remove(pos));
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

#[test]
fn full_cycle_over_ffi() {
    assert_eq!(eryu_abi_version() >> 16, 1, "ABI major 应为 1");

    let core = eryu_core_new(std::ptr::null());
    assert!(!core.is_null());
    assert_eq!(
        eryu_set_event_cb(core, Some(on_event), std::ptr::null_mut()),
        0
    );

    let text = CString::new("你好，Eryu。").unwrap();
    assert_eq!(eryu_debug_submit_text(core, text.as_ptr()), 0);

    let result = wait_for("result", Duration::from_secs(3)).expect("应收到 result 事件");
    assert!(result.contains("你好，Eryu。"), "result 应带文本: {result}");
    let submit = wait_for("submit_request", Duration::from_secs(3)).expect("应收到 submit_request");
    assert!(submit.contains("\"mode\":\"paste\""));

    // 提取 task_id 回报 ok
    let task_id = extract(&submit, "task_id").expect("task_id");
    let tid = CString::new(task_id.clone()).unwrap();
    let ok = CString::new("ok").unwrap();
    let empty = CString::new("").unwrap();
    assert_eq!(
        eryu_submit_result(core, tid.as_ptr(), ok.as_ptr(), empty.as_ptr()),
        0
    );
    wait_for("state", Duration::from_secs(2));

    // 同一任务号重复回报 → NoPending，不再产生 state
    assert_eq!(
        eryu_submit_result(core, tid.as_ptr(), ok.as_ptr(), empty.as_ptr()),
        0
    );

    let last = eryu_get_last_result(core);
    assert!(!last.is_null());
    let last_s = unsafe { CStr::from_ptr(last) }
        .to_string_lossy()
        .to_string();
    assert_eq!(last_s, "你好，Eryu。");
    eryu_string_free(last);

    eryu_core_free(core);
}

#[test]
fn panic_does_not_cross_ffi() {
    let core = eryu_core_new(std::ptr::null());
    assert!(!core.is_null());
    eryu_set_event_cb(core, Some(on_event), std::ptr::null_mut());
    assert_eq!(eryu_debug_panic(core), 0);
    let err = wait_for("error", Duration::from_secs(3)).expect("panic 应转成 error 事件");
    assert!(err.contains("worker_panic"), "{err}");
    // 实例已停用：后续命令返回 -2
    assert_eq!(eryu_hotkey_down(core), -2);
    eryu_core_free(core);
}

fn extract(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn profiles_over_ffi() {
    // ERYU_CONFIG_DIR 是进程级环境变量，两个测试并行跑会互相踩（实测偶发一个失败）
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 不碰真实 ~/.eryu：用临时目录
    let dir = std::env::temp_dir().join(format!("eryu-profiles-ffi-{}", std::process::id()));
    // 上次跑留下的 profiles.toml（active 可能已经是「邮件」）会让断言假失败：先清干净
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    std::env::set_var("ERYU_CONFIG_DIR", &dir);
    let dir_c = CString::new(dir.to_string_lossy().to_string()).unwrap();
    let core = eryu_core_new(dir_c.as_ptr());
    assert!(!core.is_null());

    // 默认三档，当前是「日常」
    let json = unsafe { eryu_get_profiles(core) };
    assert!(!json.is_null());
    let text = unsafe { std::ffi::CStr::from_ptr(json) }
        .to_string_lossy()
        .to_string();
    unsafe { eryu_string_free(json) };
    assert!(
        text.contains("\"active\":\"日常\""),
        "active 应为日常: {text}"
    );
    assert!(text.contains("邮件"), "应含内置「邮件」档: {text}");

    // 切到「邮件」
    let name = CString::new("邮件").unwrap();
    let mut err: *mut c_char = std::ptr::null_mut();
    assert_eq!(
        unsafe { eryu_set_active_profile(core, name.as_ptr(), &mut err) },
        0
    );

    // 切到不存在的档要被拒
    let bad = CString::new("不存在").unwrap();
    let mut err2: *mut c_char = std::ptr::null_mut();
    assert_eq!(
        unsafe { eryu_set_active_profile(core, bad.as_ptr(), &mut err2) },
        -3
    );
    if !err2.is_null() {
        unsafe { eryu_string_free(err2) };
    }

    // 整体替换成两档
    let payload = CString::new(r#"{"active":"乙","profiles":[{"name":"甲","level":"off"},{"name":"乙","level":"strong","prompt":"简短"}]}"#).unwrap();
    let mut err3: *mut c_char = std::ptr::null_mut();
    assert_eq!(
        unsafe { eryu_set_profiles(core, payload.as_ptr(), &mut err3) },
        0
    );

    let json2 = unsafe { eryu_get_profiles(core) };
    let text2 = unsafe { std::ffi::CStr::from_ptr(json2) }
        .to_string_lossy()
        .to_string();
    unsafe { eryu_string_free(json2) };
    assert!(text2.contains("\"active\":\"乙\""), "{text2}");
    assert!(text2.contains("简短"), "{text2}");

    unsafe { eryu_core_free(core) };
    let _ = std::fs::remove_dir_all(&dir);
}

/// v1.9/v1.10：声纹接口在「没注册、没模型」时也要给出确定答案，而不是崩或者假装成功
#[test]
fn voice_endpoints_degrade_cleanly() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let dir = std::env::temp_dir().join(format!("eryu-voice-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("ERYU_CONFIG_DIR", &dir);
    let cdir = CString::new(dir.to_string_lossy().to_string()).unwrap();

    unsafe {
        let core = eryu_core_new(cdir.as_ptr());
        assert!(!core.is_null());

        // 状态：应该是「没注册、模型没装」，但字段齐全
        let st = eryu_voice_state(core);
        let text = CStr::from_ptr(st).to_string_lossy().to_string();
        eryu_string_free(st);
        assert!(text.contains("\"enrolled\":false"), "{text}");
        assert!(text.contains("\"model_ready\":false"), "{text}");

        // 用不存在的 WAV 注册 → -3，并给中文原因
        let bad = CString::new("/tmp/不存在-voice.wav").unwrap();
        let mut err: *mut c_char = std::ptr::null_mut();
        assert_eq!(eryu_voice_enroll_wav(core, bad.as_ptr(), &mut err), -3);
        assert!(!err.is_null(), "失败要给原因");
        eryu_string_free(err);

        // 没注册也能「删除」（幂等）→ 0
        let mut err2: *mut c_char = std::ptr::null_mut();
        assert_eq!(eryu_voice_clear(core, &mut err2), 0);
        if !err2.is_null() {
            eryu_string_free(err2);
        }

        eryu_core_free(core);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
