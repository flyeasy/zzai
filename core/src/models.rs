//! 模型注册表与下载（DESIGN §9.2）
//!
//! 只做三件事：内置清单 → 下载到 `models/.download/` → 解包到 `models/`。
//! 解包用系统 `tar`（macOS/Windows 10+ 都自带 bsdtar，支持 bz2），避免为一个 tar.bz2 引入两个 crate。
//! 下载带断点续传（已有 `.part` 文件时用 HTTP Range 续），进度通过 `model_progress` 事件回报。

use std::path::{Path, PathBuf};

/// 清单里的一项
pub struct Entry {
    pub id: &'static str,
    /// 下载地址
    pub url: &'static str,
    /// 落盘文件名（放在 models/.download/ 下）
    pub file: &'static str,
    /// 解包后应该存在的标志文件（相对 models/ 或模型目录）
    pub marker: &'static str,
    /// 大致体积，仅用于 UI 文案
    pub approx_mb: u32,
    /// true = tar.bz2 解包；false = 单文件直接落到 models/
    pub archive: bool,
}

pub const ENTRIES: &[Entry] = &[
    Entry {
        id: "sense",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09.tar.bz2",
        file: "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09.tar.bz2",
        marker: "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09/model.int8.onnx",
        approx_mb: 233,
        archive: true,
    },
    Entry {
        id: "qwen3",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2",
        file: "sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2",
        marker: "sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25/encoder.int8.onnx",
        approx_mb: 879,
        archive: true,
    },
    Entry {
        id: "speaker",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx",
        file: "3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx",
        marker: "3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx",
        approx_mb: 39,
        archive: false,
    },
    Entry {
        id: "punct",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/punctuation-models/sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8.tar.bz2",
        file: "sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8.tar.bz2",
        marker: "sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8/model.int8.onnx",
        approx_mb: 65,
        archive: true,
    },
    Entry {
        id: "gtcrn",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speech-enhancement-models/gtcrn_simple.onnx",
        file: "gtcrn_simple.onnx",
        marker: "gtcrn_simple.onnx",
        approx_mb: 1,
        archive: false,
    },
    Entry {
        id: "vad",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx",
        file: "silero_vad.onnx",
        marker: "silero_vad.onnx",
        approx_mb: 1,
        archive: false,
    },
];

pub fn find(id: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.id == id)
}

pub fn installed(models_dir: &Path, entry: &Entry) -> bool {
    models_dir.join(entry.marker).exists()
}

fn download_dir(models_dir: &Path) -> PathBuf {
    models_dir.join(".download")
}

/// 下载一个 entry。`progress` 每 ~400ms 被调用一次 `(received, total)`。
/// 实际下载地址：配置了镜像就用「镜像 + 文件名」，否则走官方 GitHub release。
/// 镜像约定：把同一批发布文件平铺在根目录（如 `http://host/eryu-models/`）。
pub fn source_url(entry: &Entry, mirror: &str) -> String {
    let m = mirror.trim().trim_end_matches('/');
    if m.is_empty() {
        entry.url.to_string()
    } else {
        format!("{m}/{}", entry.file)
    }
}

/// 返回 Ok(()) 表示文件已就绪（可能来自断点续传），失败返回中文原因。

/// 系统代理：先看环境变量，Windows 上再读 Internet Settings（curl 自己不会读注册表）。
pub fn system_proxy() -> Option<String> {
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(v) = std::env::var(key) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    #[cfg(windows)]
    {
        if let Some(p) = windows_proxy_from_registry() {
            return Some(p);
        }
    }
    None
}

#[cfg(windows)]
fn windows_proxy_from_registry() -> Option<String> {
    use std::process::{Command, Stdio};
    let mut q = |name: &str| -> Option<String> {
        let mut cmd = Command::new("reg");
        hide_console(&mut cmd);
        let out = cmd
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                "/v",
                name,
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        for line in text.lines() {
            if let Some(idx) = line.find(name) {
                let rest = &line[idx + name.len()..];
                let val = rest.rsplit("REG_").next().unwrap_or("").trim().to_string();
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }
        None
    };
    let enabled = q("ProxyEnable").unwrap_or_default();
    if enabled.trim() != "0x1" && enabled.trim() != "1" {
        return None;
    }
    let server = q("ProxyServer")?;
    // 形如 "10.0.0.1:8080" 或 "http=1.2.3.4:80;https=1.2.3.4:443"
    if server.contains('=') {
        for part in server.split(';') {
            if let Some(rest) = part.strip_prefix("https=") {
                return Some(rest.trim().to_string());
            }
        }
        for part in server.split(';') {
            if let Some(rest) = part.strip_prefix("http=") {
                return Some(rest.trim().to_string());
            }
        }
    }
    Some(server.trim().to_string())
}

/// curl 退出码 → 一句人话（内网里最常见的是 6/7/28/35）
fn curl_hint(status: i32) -> &'static str {
    match status {
        5 => "代理地址解析不出来，检查系统代理设置",
        6 => "域名解析失败：内网可能要走代理，或 DNS 没放行",
        7 => "连不上服务器：防火墙/代理挡了，或镜像地址不可达",
        18 | 56 => "传输中断：网络不稳定或中间设备重置了连接",
        22 => "服务器返回错误（404/403）：镜像路径或权限不对",
        28 => "超时：网速太慢或代理不通",
        35 => "TLS 握手失败：可能有中间设备拦截 HTTPS",
        _ => "",
    }
}

pub fn fetch(
    models_dir: &Path,
    entry: &Entry,
    mirror: &str,
    proxy: &str,
    mut progress: impl FnMut(u64, u64),
    cancelled: impl Fn() -> bool,
) -> Result<PathBuf, String> {
    let dir = download_dir(models_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("建下载目录失败：{e}"))?;
    let part = dir.join(format!("{}.part", entry.file));
    let dest = dir.join(entry.file);

    if part.exists() && dest.exists() {
        let _ = std::fs::remove_file(&part);
    }
    if !dest.exists() {
        // 用系统 curl 下载：GitHub 的 TLS 握手对 rustls 客户端会被中断（实测 rustls → unexpected end of file，
        // 而 curl / 浏览器正常），所以这里走 curl，顺便拿到断点续传。
        let eff_proxy = if proxy.trim().is_empty() {
            system_proxy()
        } else {
            Some(proxy.trim().to_string())
        };
        // 镜像缺文件（404/403）时自动退回官方源：内置镜像只同步了部分模型，
        // 用户在设置页点「下载」不该因为镜像没同步而彻底下不了。
        let mut candidates: Vec<String> = Vec::new();
        let mirrored = source_url(entry, mirror);
        if !mirror.trim().is_empty() {
            candidates.push(mirrored);
        }
        if !candidates.iter().any(|u| u == entry.url) {
            candidates.push(entry.url.to_string());
        }
        let mut last_err = String::new();
        let mut ok = false;
        for (i, url) in candidates.iter().enumerate() {
            match download_with_curl(url, &part, eff_proxy.as_deref(), &mut progress, &cancelled) {
                Ok(()) => {
                    ok = true;
                    break;
                }
                Err(e) => {
                    let missing = e.contains("404") || e.contains("403");
                    let has_fallback = i + 1 < candidates.len();
                    if missing && has_fallback {
                        let _ = std::fs::remove_file(&part);
                        eprintln!("[eryu] 镜像没有这个文件，改走官方源：{url}（{e}）");
                        last_err = e;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        if !ok {
            return Err(last_err);
        }
        let received = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        progress(received, received);
        std::fs::rename(&part, &dest).map_err(|e| format!("改名前失败：{e}"))?;
    }

    if entry.archive {
        extract(&dest, models_dir)?;
        let _ = std::fs::remove_file(&dest);
    } else {
        let target = models_dir.join(entry.marker);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::copy(&dest, &target).map_err(|e| format!("复制失败：{e}"))?;
        let _ = std::fs::remove_file(&dest);
    }
    Ok(models_dir.join(entry.marker))
}

/// 真正跑一次 curl（带断点续传 + 进度回报）。失败时把 curl 最后一行错误拼进返回串。
fn download_with_curl(
    url: &str,
    part: &Path,
    proxy: Option<&str>,
    progress: &mut impl FnMut(u64, u64),
    cancelled: &impl Fn() -> bool,
) -> Result<(), String> {
    let existing = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    let total = head_length(url, proxy)
        .map(|t| t.max(existing))
        .unwrap_or(0);
    let errfile = part.with_extension("err");
    let errsink = std::fs::File::create(&errfile).ok();
    let mut cmd = std::process::Command::new("curl");
    hide_console(&mut cmd);
    if let Some(p) = proxy {
        cmd.arg("--proxy").arg(p);
    }
    let mut child = cmd
        .arg("-fL")
        .arg("--retry")
        .arg("2")
        .arg("--connect-timeout")
        .arg("20")
        .arg("-C")
        .arg("-")
        .arg("-o")
        .arg(part)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(match errsink {
            Some(f) => std::process::Stdio::from(f),
            None => std::process::Stdio::null(),
        })
        .spawn()
        .map_err(|e| format!("启动 curl 失败：{e}"))?;
    let mut last = std::time::Instant::now();
    loop {
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("已取消".into());
        }
        match child
            .try_wait()
            .map_err(|e| format!("等待 curl 失败：{e}"))?
        {
            Some(status) => {
                if !status.success() {
                    let detail = std::fs::read_to_string(&errfile)
                        .ok()
                        .map(|t| {
                            t.lines()
                                .filter(|l| !l.trim().is_empty())
                                .last()
                                .unwrap_or("")
                                .trim()
                                .to_string()
                        })
                        .unwrap_or_default();
                    let hint = curl_hint(status.code().unwrap_or(-1));
                    let pxy = proxy
                        .map(|p| format!("；代理 {p}"))
                        .unwrap_or_else(|| "；没有检测到系统代理".to_string());
                    return Err(if detail.is_empty() {
                        format!("下载失败（curl {status}）{hint}{pxy}")
                    } else {
                        format!("下载失败（curl {status}）：{detail}{hint}{pxy}")
                    });
                }
                break;
            }
            None => {
                if last.elapsed().as_millis() >= 400 {
                    let received = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
                    progress(received, total);
                    last = std::time::Instant::now();
                }
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
        }
    }
    let _ = std::fs::remove_file(&errfile);
    Ok(())
}

/// Windows 上别弹出黑色控制台窗口（这版是 windows 子系统，spawn 子进程默认会闪一个 cmd）
fn hide_console(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// 用 curl 取 Content-Length（失败就返回 None，UI 显示不确定进度）
fn head_length(url: &str, proxy: Option<&str>) -> Option<u64> {
    let mut cmd = std::process::Command::new("curl");
    hide_console(&mut cmd);
    if let Some(p) = proxy {
        cmd.arg("--proxy").arg(p);
    }
    let out = cmd.args(["-sIL", "--max-time", "20", url]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
    // 取最后一个 content-length（跟随重定向后是最终地址的）
    text.lines()
        .filter_map(|l| l.strip_prefix("content-length:"))
        .filter_map(|v| v.trim().parse::<u64>().ok())
        .max()
}

fn extract(archive: &Path, models_dir: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new("tar");
    hide_console(&mut cmd);
    let out = cmd
        .arg("-xjf")
        .arg(archive)
        .arg("-C")
        .arg(models_dir)
        .output()
        .map_err(|e| format!("调用 tar 失败：{e}"))?;
    if !out.status.success() {
        return Err(format!(
            "解包失败（{}）：{}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_have_unique_ids_and_urls() {
        let mut ids = std::collections::BTreeSet::new();
        for e in ENTRIES {
            assert!(ids.insert(e.id), "重复 id：{}", e.id);
            assert!(e.url.starts_with("https://"), "{} 必须是 https", e.id);
            assert!(!e.marker.is_empty());
        }
    }

    #[test]
    fn find_works() {
        assert!(find("sense").is_some());
        assert!(find("qwen3").is_some());
        assert!(find("nope").is_none());
    }
}
