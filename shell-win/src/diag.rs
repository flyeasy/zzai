//! 诊断包（DESIGN §12.8 / D-114）：一次点击导出、可选上传。
//!
//! 只收「定位问题要用到、但不含用户内容」的东西：版本、系统、脱敏后的配置、
//! 核心自检、最近错误码、日志尾部、历史条数统计。**不写任何转写正文。**

use std::fs;
use std::sync::Mutex;

use serde_json::json;

/// 最近 20 条错误码：`(code, 摘要)`。主线程写、导出线程读。
static ERR_RING: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

pub fn note_error(code: &str, msg: &str) {
    if code.is_empty() {
        return;
    }
    let brief: String = msg.lines().next().unwrap_or("").chars().take(120).collect();
    {
        let mut ring = crate::lock(&ERR_RING);
        ring.push((code.to_string(), brief.clone()));
        if ring.len() > 20 {
            let drop = ring.len() - 20;
            ring.drain(0..drop);
        }
    }
    // 落盘：错误往往发生在「上一次运行」，而诊断包是下一次运行里导出的，
    // 只在内存里留 20 条会丢证据。
    let p = errors_path();
    let line = format!("{}\t{}\t{}\n", now_stamp(), code, brief);
    let mut keep = String::new();
    if let Ok(old) = fs::read_to_string(&p) {
        let total = old.len() + line.len();
        if total > 64 * 1024 {
            let cut = old.len().saturating_sub(32 * 1024);
            let cut = old[cut..].find('\n').map(|i| cut + i + 1).unwrap_or(cut);
            keep.push_str(&old[cut..]);
        } else {
            keep = old;
        }
    }
    keep.push_str(&line);
    let _ = fs::write(&p, keep);
}

fn errors_path() -> String {
    format!("{}/eryu-errors.log", crate::config_dir())
}

/// 最近 20 条错误（内存 + 落盘合并，落盘的在前面）。
pub fn error_codes() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    if let Ok(text) = fs::read_to_string(errors_path()) {
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let start = lines.len().saturating_sub(20);
        for l in &lines[start..] {
            let mut it = l.splitn(3, '\t');
            let _at = it.next();
            let code = it.next().unwrap_or("");
            let msg = it.next().unwrap_or("");
            if !code.is_empty() {
                out.push((code.to_string(), msg.to_string()));
            }
        }
    }
    out.truncate(20);
    if out.is_empty() {
        return crate::lock(&ERR_RING).clone();
    }
    out
}

fn now_stamp() -> String {
    let t = chrono_like_now();
    format!("{t}")
}

fn chrono_like_now() -> String {
    // 不引 chrono：用 SystemTime 走本地时间戳（UTC+8 由系统 local time 决定）
    let now = std::time::SystemTime::now();
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = civil_from_unix(secs + 8 * 3600);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

fn civil_from_unix(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    let h = (rem / 3600) as u32;
    let mi = ((rem % 3600) / 60) as u32;
    let s = (rem % 60) as u32;
    (y, m, d, h, mi, s)
}

/// 配置脱敏：key/token/password 的值一律换 `***`（只留「有没有设」的信息）。
pub fn redact_config(toml: &str) -> String {
    let mut out = String::new();
    for line in toml.lines() {
        let lower = line.to_ascii_lowercase();
        let key_part = lower.split('=').next().unwrap_or("").trim_start_matches('#').trim();
        let is_secret = key_part.ends_with("api_key")
            || key_part == "token"
            || key_part.ends_with("_token")
            || key_part.contains("password")
            || key_part.contains("secret");
        if is_secret {
            if let Some(eq) = line.find('=') {
                let key = &line[..eq];
                let val: String = line[eq + 1..].trim().trim_matches('"').chars().collect();
                if val.is_empty() {
                    out.push_str(&format!("{key}= \"\""));
                } else {
                    out.push_str(&format!("{key}= \"***\""));
                }
                out.push('\n');
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn log_tail(lines: usize) -> String {
    let p = format!("{}/eryu-cli.log", crate::config_dir());
    let Ok(text) = fs::read_to_string(&p) else {
        return String::from("（还没有写日志）");
    };
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    all[start..].join("\n")
}

/// 导出线程要用的数据：在 UI 线程一次取齐，导出时不再碰 APP 锁。
pub struct Snapshot {
    pub version: &'static str,
    pub diagnose: String,
    pub config_toml: String,
    pub upload_url: String,
    pub history_json: String,
    pub host: serde_json::Value,
}

/// 从 UI 线程的 APP 取快照（App 还没建好时退化为空）。
pub fn snapshot() -> Snapshot {
    let lock = crate::lock(&crate::APP);
    build(lock.as_ref().map(|a| &a.core))
}

/// CLI 路径用：核心已经加载、但还没有 App。
pub fn snapshot_core(core: &crate::core::Core) -> Snapshot {
    build(Some(core))
}

fn build(core: Option<&crate::core::Core>) -> Snapshot {
    let (diagnose, config_toml, upload_url, history_json) = match core {
        Some(c) => {
            let cfg: serde_json::Value =
                serde_json::from_str(&c.config_json()).unwrap_or(json!({}));
            (
                c.diagnose(),
                c.config_toml(),
                cfg.get("diag_upload_url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                c.history_json(40),
            )
        }
        None => (
            "核心没有起来（看日志尾部）".to_string(),
            String::new(),
            String::new(),
            "[]".to_string(),
        ),
    };
    Snapshot {
        version: crate::APP_VERSION,
        diagnose,
        config_toml,
        upload_url,
        history_json,
        host: host_info(),
    }
}

impl Snapshot {
    pub fn bundle(&self) -> String {
        let mut errors = Vec::new();
        for (code, msg) in error_codes() {
            errors.push(json!({"code": code, "msg": msg}));
        }
        let doc = json!({
            "format": 1,
            "app": {
                "name": "字在",
                "version": self.version,
                "platform": "windows",
                "arch": std::env::consts::ARCH,
                "generated_at": now_stamp(),
            },
            "host": self.host,
            "config": redact_config(&self.config_toml),
            "diagnose": self.diagnose,
            "errors": errors,
            "history": history_summary(&self.history_json),
            "log_tail": log_tail(200),
        });
        serde_json::to_string_pretty(&doc)
            .unwrap_or_else(|e| format!("{{\"format\":1,\"error\":\"{e}\"}}"))
    }

    /// 写出诊断包，返回路径。
    pub fn save(&self) -> Result<std::path::PathBuf, String> {
        let dir = format!("{}/diag", crate::config_dir());
        fs::create_dir_all(&dir).map_err(|e| format!("建不了诊断包目录 {dir}：{e}"))?;
        let p = std::path::PathBuf::from(format!("{dir}/eryu-diag-{}.json", now_stamp()));
        fs::write(&p, self.bundle()).map_err(|e| format!("诊断包写不进去 {}：{e}", p.display()))?;
        Ok(p)
    }
}

fn host_info() -> serde_json::Value {
    json!({
        "os": windows_version(),
        "dpi_scale": dpi_scale(),
        "screen": screen_size(),
        "locale": "zh-CN",
        "exe": std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default(),
    })
}

fn dpi_scale() -> String {
    use windows::Win32::UI::HiDpi::GetDpiForSystem;
    let d = unsafe { GetDpiForSystem() };
    format!("{}%", (d as f32 / 96.0 * 100.0).round() as i32)
}

fn screen_size() -> String {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe {
        format!(
            "{}x{}",
            GetSystemMetrics(SM_CXSCREEN),
            GetSystemMetrics(SM_CYSCREEN)
        )
    }
}

fn windows_version() -> String {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };
    unsafe fn read_sz(path: &str, name: &str) -> String {
        let mut buf = [0u16; 256];
        let mut len = (buf.len() * 2) as u32;
        let sub = HSTRING::from(path);
        let key = HSTRING::from(name);
        let rc = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &sub,
            &key,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len),
        );
        if rc.is_err() {
            return String::new();
        }
        let n = (len as usize / 2).saturating_sub(1);
        String::from_utf16_lossy(&buf[..n.min(buf.len())])
    }
    unsafe fn read_u32(path: &str, name: &str) -> u32 {
        let mut v: u32 = 0;
        let mut len = 4u32;
        let sub = HSTRING::from(path);
        let key = HSTRING::from(name);
        let rc = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &sub,
            &key,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut len),
        );
        if rc.is_err() {
            0
        } else {
            v
        }
    }
    const P: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let (name, build, disp, ubr) = unsafe {
        (
            read_sz(P, "ProductName"),
            read_sz(P, "CurrentBuildNumber"),
            read_sz(P, "DisplayVersion"),
            read_u32(P, "UBR"),
        )
    };
    if name.is_empty() {
        "Windows（读不到版本号）".to_string()
    } else {
        // 注册表里 ProductName 常年写着「Windows 10」（Win11 也是），按 build 号纠正，
        // 否则诊断包里会看到自相矛盾的版本。
        let build_no: u32 = build.split('.').next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let name = if build_no >= 22000 && name.contains("Windows 10") {
            name.replace("Windows 10", "Windows 11")
        } else {
            name
        };
        format!("{name} {disp}（build {build}.{ubr}）")
    }
}

fn history_summary(raw: &str) -> serde_json::Value {
    // 只统计条数与状态分布，不带正文（隐私：诊断包不该包含用户说的话）
    let v: serde_json::Value = serde_json::from_str(raw).unwrap_or(json!([]));
    let mut by_status = std::collections::BTreeMap::new();
    let mut total = 0usize;
    if let Some(arr) = v.as_array() {
        total = arr.len();
        for it in arr {
            let st = it.get("status").and_then(|x| x.as_str()).unwrap_or("?");
            *by_status.entry(st.to_string()).or_insert(0) += 1;
        }
    }
    json!({"recent": total, "by_status": by_status})
}

/// 在后台线程上传：`url` 来自 `[diag] upload_url`，`body` 是 `Snapshot::bundle()`。
/// 返回给用户看的一句话（成功/失败都直接显示）。
pub fn upload_body(up: crate::core::Uploader, url: String, body: String) -> Result<String, String> {
    if url.trim().is_empty() {
        return Err(
            "没设上传地址（[diag] upload_url 留空了）：诊断包已保存到本地 diag 目录，直接发给我即可"
                .into(),
        );
    }
    let (rc, detail) = up.upload(&url, &body);
    if rc == 0 {
        Ok(format!("已上传到 {url}"))
    } else if detail.is_empty() {
        Err(format!("上传失败（{rc}）"))
    } else {
        Err(detail)
    }
}
