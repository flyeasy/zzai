//! Eryu Windows 壳（W1 最小可用）
//!
//! 目标：把 macOS 版的核心交互先搬过来——按住右 Alt 说话 → 浮层 → 松手剪贴板上屏。
//! 设置/历史等桌面 UI 属于 W2（见 docs/WINDOWS.md），这里先保证「能用 + 可自测」。
//!
//! 设计约束（与 macOS 一致）：
//! - 热键走 Raw Input（能拿 keyUp、能分左右修饰键、不吞键）；
//! - 注入走「剪贴板 + SendInput Ctrl+V」，每轮只提交一次；
//! - core 是同一个 Rust 静态库编出的 `eryu_core.dll`，用 LoadLibrary 动态加载。

#![windows_subsystem = "windows"]

mod core;
mod diag;
mod inject;
mod keycap;
mod autostart;
mod caret;
mod media;
mod overlay;
mod selection;
mod mini;
mod observer;
mod tray;
mod ui;
mod settings;
mod hotkey;
mod hotkey_hook;
mod onboarding;
mod icon;

use std::io::Write;
use std::sync::Mutex;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIM_TYPEKEYBOARD};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const APP_TITLE: &str = "字在";
pub const APP_EN: &str = "zzai";

pub struct App {
    pub core: core::Core,
    pub overlay: overlay::Overlay,
    pub hwnd: HWND,
    /// 我们是否暂停过播放（只恢复自己暂停的那一次）
    pub media_paused: std::sync::atomic::AtomicBool,
    /// 当前阶段（idle/starting/listening/processing/inserting），托盘「状态」与「取消」用
    pub phase: String,
}

// 只在 UI 线程访问（静态 Mutex 保护）；HWND 本身不是 Send，这里明确由我们保证线程约束。
unsafe impl Send for App {}

static APP: Mutex<Option<App>> = Mutex::new(None);

/// 当前生效的热键（从 core 的 config 里读，默认右 Alt）
static HOTKEY: Mutex<Option<hotkey::Spec>> = Mutex::new(None);
/// 主键是否已经按下（忽略自动重复）
static HOTKEY_DOWN: Mutex<bool> = Mutex::new(false);
/// 这一段是谁开始的：0 = 没有，1 = 热键，2 = 迷你窗点击。
/// 迷你窗是「点一下开始、再点一下停止」，会话自己结束（静音/到上限）时没人会来调
/// `talk_up()` 清 `HOTKEY_DOWN`，下一次点击就会被去重逻辑吞掉（I-161）。
static TALK_SOURCE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
/// 润色档的快捷键槽：(档名, 图标, 组合键)。空槽不登记。
static SLOTS: Mutex<Vec<(String, String, hotkey::Spec)>> = Mutex::new(Vec::new());
/// 录音时暂停系统播放（配置 [audio] pause_media）
pub static PAUSE_MEDIA: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// 自定义消息
const WM_ERYU_TRAY: u32 = WM_APP + 1;
const WM_ERYU_EVENT: u32 = WM_APP + 2;
/// 低级键盘钩子投过来的键事件（wparam：vk | break<<8 | e0<<9）。
/// 钩子回调里只 PostMessage，真正的判定/派发都在这里（UI 线程）做，避免卡住全系统键盘。
pub(crate) const WM_ERYU_KEY: u32 = WM_APP + 3;
/// 二次启动时投给已有实例：把设置窗口叫出来（审计 P1：以前会开第二份，抢热键/托盘）
const WM_ERYU_REVEAL: u32 = WM_APP + 4;
/// 第二份实例告诉已运行的那份「又有人点了图标」：托盘气泡提示一下，不用弹模态框
const WM_ERYU_SECOND: u32 = WM_APP + 11;
/// 注入在后台线程做完后，用这条消息把结果交回 UI 线程（在事件分发里直接 SendInput 不生效，见 I-113）
const WM_ERYU_INJECTED: u32 = WM_APP + 6;

/// 按下热键时的前台窗口（上屏前若被自家窗口抢了前台，用它找回来）
/// 「上次没上屏的暂存文本」是否已提示过（启动兜底与核心广播各可能来一次，只提示一次）
pub(crate) static PENDING_SHOWN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) static LAST_TARGET: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// 后台注入的结果
struct InjectDone {
    task: String,
    text: String,
    status: &'static str,
    detail: String,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// CLI 输出：exe 是 windows 子系统（没有控制台），所以同时写日志文件

/// 诊断开关：ERYU_TRACE=1 时把关键路径打成一行，便于定位「卡在哪一步」
pub(crate) fn trace(msg: &str) {
    if std::env::var("ERYU_TRACE").is_ok() {
        eprintln!("[eryu][trace] {msg}");
        // exe 是 windows 子系统，没有控制台：同时写日志文件，远程/真机上才能事后看
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!("{}/eryu-cli.log", config_dir()))
        {
            let _ = writeln!(f, "[trace] {msg}");
        }
    }
}

pub(crate) fn report(line: &str) {
    println!("{line}");
    rotate_cli_log();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{}/eryu-cli.log", config_dir()))
    {
        let _ = writeln!(f, "{line}");
    }
}

/// 日志只保留最近一段：诊断包只取尾部 200 行，控制文件不会无限长。
fn rotate_cli_log() {
    const LIMIT: u64 = 512 * 1024;
    const KEEP: usize = 200 * 1024;
    let p = format!("{}/eryu-cli.log", config_dir());
    let Ok(md) = std::fs::metadata(&p) else { return };
    if md.len() <= LIMIT {
        return;
    }
    let Ok(text) = std::fs::read_to_string(&p) else { return };
    let cut = text.len().saturating_sub(KEEP);
    // 从换行处切，避免留下半行
    let cut = text[cut..].find('\n').map(|i| cut + i + 1).unwrap_or(cut);
    let _ = std::fs::write(&p, &text[cut..]);
}

/// 今日已节省（分）：口径与 mac 侧一致（chars ÷ 1000 × 40 分钟）
fn today_saved_minutes(core: &crate::core::Core) -> i32 {
    let js = core.stats_json();
    let chars = serde_json::from_str::<serde_json::Value>(&js)
        .ok()
        .and_then(|v| v.get("today").and_then(|t| t.get("chars")).and_then(|c| c.as_i64()))
        .unwrap_or(0);
    ((chars / 1000) * 40) as i32
}

/// 热键的展示名（给托盘提示和迷你窗用）
pub(crate) fn hotkey_hint() -> String {
    let raw = HOTKEY
        .lock()
        .ok()
        .and_then(|h| h.as_ref().map(|s| s.raw.clone()))
        .unwrap_or_else(|| "ALT_R".to_string());
    // 界面里别直接甩 `ALT_R`/`CTRL+ALT+SPACE` 这种配置写法（用户看到的是键名）
    match raw.as_str() {
        "ALT_R" | "RALT" | "RIGHT_ALT" => "右 Alt".to_string(),
        "ALT_L" | "LALT" | "LEFT_ALT" => "左 Alt".to_string(),
        "CTRL_R" => "右 Ctrl".to_string(),
        "CTRL_L" => "左 Ctrl".to_string(),
        "SHIFT_R" => "右 Shift".to_string(),
        "SHIFT_L" => "左 Shift".to_string(),
        _ => raw
            .split('+')
            .map(|t| match t {
                "CTRL" => "Ctrl",
                "ALT" => "Alt",
                "SHIFT" => "Shift",
                "WIN" => "Win",
                "SPACE" => "空格",
                "ESC" => "Esc",
                other => other,
            })
            .collect::<Vec<_>>()
            .join("+"),
    }
}

/// 迷你窗按钮：点一下开始，再点一下停止（只作用于按钮；全局模式留 R-110）
pub(crate) fn mini_toggle_talk() {
    let phase = APP
        .lock()
        .ok()
        .and_then(|a| a.as_ref().map(|a| a.phase.clone()))
        .unwrap_or_else(|| "idle".into());
    // 不伪造按键：热键是组合键（如 CTRL+ALT+SPACE）时，只发主键永远匹配不上。
    // 直接走「按住说话」的同一个入口，行为与按键完全一致。
    match phase.as_str() {
        // 迷你窗路径不走 HOTKEY_DOWN 去重：它没有「松开」这个事件，去重反而会
        // 让上一段自然结束后永远点不动（I-161）。
        "idle" | "done" => {
            let mut held = crate::lock(&HOTKEY_DOWN);
            if !*held {
                drop(held);
                talk_from_mini();
            }
        }
        "starting" | "listening" => talk_up(),
        _ => {}
    }
}

/// 取锁：某个线程 panic 后锁会被标记 poisoned，之后每一次 `lock().unwrap()` 都会 panic
/// （用户看到的就是「上一个错没报出来，后面连点都点不动」）。统一按「沿用里面最后的值」恢复，
/// 把偶发 panic 限制在那一次调用，不影响后续操作。
pub(crate) fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 隐私模式（不写历史、不学热词）
pub(crate) fn privacy_mode() -> bool {
    let app = crate::lock(&APP);
    app.as_ref()
        .and_then(|a| serde_json::from_str::<serde_json::Value>(&a.core.config_json()).ok())
        .and_then(|v| v.get("privacy_mode").and_then(|x| x.as_bool()))
        .unwrap_or(false)
}

/// 产品版本：与 mac 的 Info.plist / 安装包名保持一致（发版时一起改）。
pub(crate) const APP_VERSION: &str = "0.8.2";

fn config_dir() -> String {
    // 与 core 的优先级一致：显式环境变量 > 便携目录 > %APPDATA%\Eryu。
    // 自动化测试靠它把数据指到临时目录，不碰用户的真实配置。
    if let Ok(d) = std::env::var("ERYU_CONFIG_DIR") {
        if !d.trim().is_empty() {
            return d;
        }
    }
    // 绿色模式：exe 旁边有 `portable.txt`（或已有 EryuData 目录）就把数据放这儿，
    // 不碰 %APPDATA%，拷走整个文件夹就能带走配置、历史、模型。
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("portable.txt").is_file() || dir.join("EryuData").is_dir() {
                let data = dir.join("EryuData");
                let _ = std::fs::create_dir_all(&data);
                return data.to_string_lossy().into_owned();
            }
        }
    }
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    format!("{base}\\Eryu")
}

/// 开发模式：`--dev` 或 `ERYU_DEV=1`。发版包里不给测试口，但内部排障要用。
pub(crate) fn dev_mode() -> bool {
    std::env::args().any(|a| a == "--dev")
        || std::env::var("ERYU_DEV").ok().as_deref() == Some("1")
}

/// 产品版本（跟 README/CHANGELOG 走；Windows 壳不单独发版）
pub const PRODUCT_VERSION: &str = "0.8.2（预览）";

fn main() {
    // 启动耗时的起点：后面按「核心就绪 / 界面就绪」各打一行（ERYU_TRACE=1 才写）
    let t_boot = std::time::Instant::now();
    // 先声明 DPI 感知：否则在高分屏上 GDI 按逻辑坐标绘制，
    // 实测会让浮层文字被压缩到一半宽度、和量出来的宽度对不上。
    unsafe {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let args: Vec<String> = std::env::args().collect();
    // 发版包里不放「能改用户数据」的开关：看起来像功能，实际是测试口。
    // 需要时加 --dev（或 ERYU_DEV=1）——文档里只写 --dev，不解释内部细节。
    let dev = args.iter().any(|a| a == "--dev")
        || std::env::var("ERYU_DEV").ok().as_deref() == Some("1");
    const DEV_ONLY: &[&str] = &[
        "--set-config",
        "--set-hotkey",
        "--history-delete",
        "--assume-consent",
        "--decode-wav",
        "--inject-test",
        "--inject-test-hold",
        "--hotkey-sim",
        "--hotkey-sim-force",
        "--hotkey-sim-vk",
        "--hotkey-sim-repeat",
        "--hotkey-sim-secs",
        "--hotkey-sim-delay",
        "--onboarding-step",
        "--dump-config",
        "--selection-run",
        "--selection-demo",
    ];
    if !dev {
        if let Some(hit) = args.iter().find(|a| DEV_ONLY.contains(&a.as_str())) {
            eprintln!("{hit} 只在开发版可用（启动时加 --dev）");
            std::process::exit(2);
        }
    }
    // 任何自测/自动化开关都不该弹模态框（弹了就等人点，脚本会卡死）
    let quiet = args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--self-test"
                | "--settings-pages-smoke"
                | "--settings-input-selftest"
                | "--hotwords-suggest"
                | "--hotwords-suggest-selftest"
                | "--settings-perapp-selftest"
                | "--onboarding-probe-selftest"
                | "--voice-state"
                | "--voice-enroll-wav"
                | "--polish-test"
                | "--polish-prompt-selftest"
                | "--polish-prompt-reset"
                | "--polish-models"
                | "--perm-report"
                | "--history-delete"
                | "--tray-menu-dump"
                | "--tray-cmd"
                | "--mini-toggle"
                | "--settings-mic-test"
                | "--settings-cycle"
                | "--inject-test"
                | "--inject-test-hold"
                | "--dump-config"
                | "--tray-drop-test"
                | "--diag-bundle"
                | "--diag-upload"
        )
    });
    if quiet {
        QUIET.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if args.iter().any(|a| a == "--assume-consent") {
        // 只跳过「润色会发到服务端」那一个弹窗；其它模态框照旧弹（--assume-consent
        // 原来顺带把全进程的 message_box 降级成 stderr，等于悄悄改了行为）
        ASSUME_CONSENT.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);

    let core = match core::Core::load(&dir) {
        Ok(c) => c,
        Err(e) => {
            // 双开是最常见的失败原因（第二份会卡在数据目录锁上），先按「已经有一份在跑」处理，
            // 别让用户看到「dll 是不是放错了」这种误导信息。
            let other = unsafe {
                use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
                let class = wide("EryuMessageWindow");
                FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()).ok().filter(|h| !h.0.is_null())
            };
            if let Some(other) = other {
                let _ = unsafe { PostMessageW(other, WM_ERYU_REVEAL, WPARAM(0), LPARAM(0)) };
                let _ = unsafe { PostMessageW(other, WM_ERYU_SECOND, WPARAM(0), LPARAM(0)) };
                std::process::exit(0);
            } else {
                message_box(&format!("字在 启动失败：{e}"));
            }
            return;
        }
    };

    trace(&format!("启动：核心就绪 {}ms", t_boot.elapsed().as_millis()));

    // 拉模型：--pull-model <id>，等 model_done / model_failed 事件
    if let Some(pos) = args.iter().position(|a| a == "--pull-model") {
        let id = args.get(pos + 1).cloned().unwrap_or_else(|| "sense".into());
        core.set_event_cb_cli();
        let (code, err) = core.pull_model(&id);
        if code != 0 {
            report(&format!("PULLFAIL {id} code={code} {err}"));
            return;
        }
        report(&format!("PULLSTART {id}"));
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done2 = done.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(1800));
            done2.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        while !done.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if !core.pulling() {
                break;
            }
        }
        report(&format!("PULLEND {}", core.last_event()));
        report(&core.diagnose());
        return;
    }

    // 自测入口：不建窗口，跑完就退
    if args.iter().any(|a| a == "--hotkey-report") {
        let spec = load_hotkey(&core);
        report(&format!(
            "HOTKEY raw={} mods={:?} main={:?} single_mod={}",
            spec.raw, spec.mods, spec.main, spec.is_single_modifier()
        ));
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--inject-type") {
        let text = args.get(pos + 1).cloned().unwrap_or_default();
        let (status, detail) = inject::type_text(&text);
        report(&format!("TYPETEST {status} {detail}"));
        std::thread::sleep(std::time::Duration::from_millis(400));
        return;
    }

    // `--voice-enroll-wav <path>`：声纹登记（UI 上「录 8 秒登记」走 mic 版本；WAV 版给自测用）
    if let Some(pos) = args.iter().position(|a| a == "--voice-enroll-wav") {
        let path = args.get(pos + 1).cloned().unwrap_or_default();
        let (rc, err) = core.voice_enroll_wav(&path);
        report(&format!("VOICE wav rc={rc} {err}"));
        report(&format!("VOICE state={}", core.voice_state()));
        return;
    }
    if args.iter().any(|a| a == "--perm-report") {
        report(&perm_report_text().replace('\n', " | "));
        return;
    }
    if args.iter().any(|a| a == "--polish-models") {
        let (list, err) = core.polish_models();
        report(&format!("POLISHMODELS n={} err={} list={:?}", list.len(), err, list));
        return;
    }
    if args.iter().any(|a| a == "--polish-test") {
        let (text, err) = core.polish_test();
        report(&format!("POLISH text={text} err={err}"));
        return;
    }
    if args.iter().any(|a| a == "--voice-state") {
        report(&format!("VOICE state={}", core.voice_state()));
        return;
    }
    if let Some(pos) = args.iter().position(|a| a == "--decode-wav") {
        let path = args.get(pos + 1).cloned().unwrap_or_default();
        // `--as-app Notepad.exe`：假装前台是这个程序，用来验证「按应用覆盖上屏方式」
        if let Some(i) = args.iter().position(|a| a == "--as-app") {
            if let Some(name) = args.get(i + 1) {
                core.note_app(name);
                report(&format!("WAVAPP {}", name));
            }
        }
        core.set_event_cb_cli();
        let (code, err) = core.decode_wav(&path);
        report(&format!("WAVCODE {code} {err}"));
        std::thread::sleep(std::time::Duration::from_millis(4000));
        report(&format!("WAVEVENT {}", core.last_event()));
        report(&format!("WAVLAST {}", core.last_result()));
        return;
    }

    if args.iter().any(|a| a == "--net-check") {
        report("NETCHECK");
        let r = net_check();
        for line in r.lines() {
            report(&format!("NET {line}"));
        }
        return;
    }

    if args.iter().any(|a| a == "--caret-probe") {
        std::thread::sleep(std::time::Duration::from_millis(600));
        report(&format!("DPI scale={:.2}", caret::dpi_scale()));
        report(&caret::probe());
        match caret::caret_anchor() {
            Some(a) => report(&format!("CARET x={} y={} flip={}", a.x, a.y, a.flip)),
            None => report("CARET none"),
        }
        return;
    }

    if args.iter().any(|a| a == "--overlay-demo") {
        let ov = overlay::Overlay::new();
        // `--overlay-scale 150` 用来看非 100% 缩放下的样子（真实 DPI 仍是 0 = 自动）
        if let Some(i) = args.iter().position(|a| a == "--overlay-scale") {
            if let Some(v) = args.get(i + 1).and_then(|v| v.parse::<i32>().ok()) {
                ov.set_forced_scale(v);
            }
        }
        let text = args.iter().position(|a| a == "--overlay-text")
            .and_then(|i| args.get(i + 1).cloned())
            .unwrap_or_else(|| "明天下午三点会议室评审设计文档".to_string());
        let phase = args.iter().position(|a| a == "--overlay-phase")
            .and_then(|i| args.get(i + 1).cloned())
            .unwrap_or_else(|| "listening".to_string());
        ov.demo_phase(&text, &phase);
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--diagnose") {
        let _ = pos;
        report(&core.diagnose());
        return;
    }
    if args.iter().any(|a| a == "--diag-bundle") || args.iter().any(|a| a == "--diag-upload") {
        let want_upload = args.iter().any(|a| a == "--diag-upload");
        let snap = diag::snapshot_core(&core);
        let body = snap.bundle();
        match snap.save() {
            Ok(p) => report(&format!("DIAG 已保存 {}", p.display())),
            Err(e) => report(&format!("DIAG 保存失败：{e}")),
        }
        if want_upload {
            match core.uploader().upload(&snap.upload_url, &body) {
                (0, _) => report(&format!("DIAG 已上传 {}", snap.upload_url)),
                (rc, e) => report(&format!("DIAG 上传失败 rc={rc} {e}")),
            }
        }
        report(&format!("DIAGCODES {}", diag::error_codes().len()));
        return;
    }

    if args.iter().any(|a| a == "--dpi-report") {
        unsafe {
            use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetProcessDpiAwareness};
            let aw = GetProcessDpiAwareness(windows::Win32::Foundation::HANDLE::default())
                .map(|a| a.0)
                .unwrap_or(-1);
            report(&format!("DPI awareness={} system={}", aw, GetDpiForSystem()));
        }
        return;
    }

    if args.iter().any(|a| a == "--settings-tab-demo") {
        let n = args
            .iter()
            .position(|a| a == "--settings-tab-demo")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(6);
        *crate::lock(&APP) = Some(App {
            core,
            overlay: overlay::Overlay::new(),
            hwnd: create_message_window(),
            media_paused: std::sync::atomic::AtomicBool::new(false),
            phase: String::from("idle"),
        });
        settings::open_at(0);
        pump_messages(0.8);
        if let Some(h) = settings::hwnd() {
            unsafe {
                use windows::Win32::Foundation::{LPARAM as LP, WPARAM as WP};
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetDlgCtrlID as GId, GetNextDlgTabItem as GNext, PostMessageW as Post,
                    WM_KEYDOWN,
                };
                use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus as GFocus, SetFocus as SFocus};
                if let Ok(first) = GNext(h, windows::Win32::Foundation::HWND::default(), false) {
                    let _ = SFocus(first);
                }
                for i in 0..n {
                    let _ = Post(h, WM_KEYDOWN, WP(0x09), LP(0));
                    pump_messages(0.25);
                    let id = GId(GFocus());
                    report(&format!("TAB {i} focus id={id}"));
                }
                let _ = GId(GFocus());
            }
        }
        pump_messages(1.0);
        return;
    }
    if args.iter().any(|a| a == "--settings-ctrl-tab-demo") {
        // Ctrl+Tab 切设置页：keybd_event 让目标线程的 GetKeyState(VK_CONTROL) 看到真实按下
        let n = args
            .iter()
            .position(|a| a == "--settings-ctrl-tab-demo")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(3);
        *crate::lock(&APP) = Some(App {
            core,
            overlay: overlay::Overlay::new(),
            hwnd: create_message_window(),
            media_paused: std::sync::atomic::AtomicBool::new(false),
            phase: String::from("idle"),
        });
        settings::open_at(0);
        pump_messages(0.8);
        if let Some(h) = settings::hwnd() {
            unsafe {
                use windows::Win32::UI::Input::KeyboardAndMouse::{
                    keybd_event, KEYEVENTF_KEYUP, VK_CONTROL,
                };
                use windows::Win32::UI::WindowsAndMessaging::{
                    PostMessageW as Post, WM_KEYDOWN,
                };
                let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(h);
                for i in 0..n {
                    keybd_event(VK_CONTROL.0 as u8, 0, Default::default(), 0);
                    std::thread::sleep(std::time::Duration::from_millis(40));
                    let _ = Post(h, WM_KEYDOWN, windows::Win32::Foundation::WPARAM(0x09), windows::Win32::Foundation::LPARAM(0));
                    // 必须在 Ctrl 还按着的时候把消息处理掉：处理器读的是当前按键状态
                    pump_messages(0.2);
                    let page = settings::current_page();
                    keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
                    pump_messages(0.15);
                    report(&format!("CTRLTAB {i} page={page}"));
                }
            }
        }
        pump_messages(0.6);
        return;
    }
    if args.iter().any(|a| a == "--onboarding-key-demo") {
        let n = args
            .iter()
            .position(|a| a == "--onboarding-key-demo")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(4);
        *crate::lock(&APP) = Some(App {
            core,
            overlay: overlay::Overlay::new(),
            hwnd: create_message_window(),
            media_paused: std::sync::atomic::AtomicBool::new(false),
            phase: String::from("idle"),
        });
        {
            let p = {
                let a = crate::lock(&APP);
                a.as_ref().map(|a| &a.core as *const crate::core::Core)
            };
            // SAFETY: core 在 App 里活到进程结束
            if let Some(p) = p {
                onboarding::open_at(unsafe { &*p }, 0);
            }
        }
        pump_messages(0.6);
        if let Some(h) = onboarding::hwnd() {
            unsafe {
                use windows::Win32::Foundation::{LPARAM as LP, WPARAM as WP};
                use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus as GFocus, SetFocus as SFocus};
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetDlgCtrlID as GId, GetNextDlgTabItem as GNext, PostMessageW as Post, WM_KEYDOWN,
                };
                if let Ok(first) = GNext(h, windows::Win32::Foundation::HWND::default(), false) {
                    let _ = SFocus(first);
                }
                for i in 0..n {
                    let _ = Post(h, WM_KEYDOWN, WP(0x09), LP(0));
                    pump_messages(0.2);
                    let id = GId(GFocus());
                    report(&format!("ONBTAB {i} focus id={id}"));
                }
            }
        }
        // 停一会儿方便截图（焦点描边看得见）
        pump_messages(9.0);
        return;
    }
    if args.iter().any(|a| a == "--settings-key-demo") {
        let pos = args.iter().position(|a| a == "--settings-key-demo");
        let id = pos
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(204);
        let save_id = pos
            .and_then(|i| args.get(i + 2))
            .and_then(|s| s.parse::<usize>().ok());
        *crate::lock(&APP) = Some(App {
            core,
            overlay: overlay::Overlay::new(),
            hwnd: create_message_window(),
            media_paused: std::sync::atomic::AtomicBool::new(false),
            phase: String::from("idle"),
        });
        settings::open_at(0);
        pump_messages(0.8);
        settings::focus_demo(id);
        pump_messages(0.3);
        let win = settings::hwnd().unwrap_or_default();
        unsafe {
            use windows::Win32::Foundation::{LPARAM as LP, WPARAM as WP};
            use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus as GFocus;
            use windows::Win32::UI::WindowsAndMessaging::{PostMessageW as Post, WM_KEYDOWN, WM_KEYUP};
            let f = GFocus();
            // BM_CLICK 是按钮「被按下」的标准路径：真实空格键最终也是走到这里
            let _ = Post(f, 0x00F5u32, WP(0), LP(0)); // BM_CLICK
            pump_messages(0.25);
            if let Some(sid) = save_id {
                let via_enter = args.iter().any(|a| a == "enter");
                settings::focus_demo(sid);
                pump_messages(0.2);
                if via_enter {
                    // 走窗口的 Enter 处理（等价于键盘回车）；要投给窗口而不是按钮
                    let _ = Post(win, WM_KEYDOWN, WP(0x0D), LP(0));
                } else {
                    let f2 = GFocus();
                    let _ = Post(f2, 0x00F5u32, WP(0), LP(0));
                }
                pump_messages(0.4);
            }
        }
        pump_messages(0.8);
        let j = {
            let a = crate::lock(&APP);
            a.as_ref().map(|a| a.core.config_json()).unwrap_or_default()
        };
        let c: serde_json::Value = serde_json::from_str(&j).unwrap_or_default();
        report(&format!(
            "KEYDEMO id={id} privacy={} launch={}",
            c["privacy_mode"], c["launch_at_login"]
        ));
        pump_messages(0.5);
        return;
    }
    if args.iter().any(|a| a == "--settings-focus-demo") {
        let id = args
            .iter()
            .position(|a| a == "--settings-focus-demo")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(157);
        *crate::lock(&APP) = Some(App {
            core,
            overlay: overlay::Overlay::new(),
            hwnd: create_message_window(),
            media_paused: std::sync::atomic::AtomicBool::new(false),
            phase: String::from("idle"),
        });
        settings::open_at(0);
        pump_messages(0.6);
        settings::focus_demo(id);
        pump_messages(9.0);
        return;
    }
    if let Some(pos) = args.iter().position(|a| a == "--selection-run") {
        // 自测：不按键、直接走「选中一段示例文字 → 执行第 N 条指令 → 粘回」的完整链路。
        // 用来验证改写结果有没有真的落回原窗口（P1-8：焦点还错窗口就会粘到空气里）。
        let idx: usize = args.get(pos + 1).and_then(|v| v.parse().ok()).unwrap_or(0);
        let sample = "嗯那个我们明天下午三点在会议室评审一下设计文档然后你觉得有问题的话随时跟我说";
        *crate::lock(&APP) = Some(App {
            core,
            overlay: overlay::Overlay::new(),
            hwnd: create_message_window(),
            media_paused: std::sync::atomic::AtomicBool::new(false),
            phase: String::from("idle"),
        });
        selection::open_demo(sample);
        pump_messages(0.4);
        selection::run(idx);
        pump_messages(9.0);
        return;
    }
    if args.iter().any(|a| a == "--selection-demo") {
        let sample = "嗯那个我们明天下午三点在会议室评审一下设计文档然后你觉得有问题的话随时跟我说";
        *crate::lock(&APP) = Some(App {
            core,
            overlay: overlay::Overlay::new(),
            hwnd: create_message_window(),
            media_paused: std::sync::atomic::AtomicBool::new(false),
            phase: String::from("idle"),
        });
        selection::open_demo(sample);
        pump_messages(9.0);
        return;
    }
    if args.iter().any(|a| a == "--version") {
        // 与 mac 一致：`Eryu --version` 只打一行，脚本可直接读
        println!("字在（zzai） {APP_VERSION}");
        return;
    }
    if args.iter().any(|a| a == "--dump-config") {
        report(&core.config_toml());
        return;
    }
    if args.iter().any(|a| a == "--lan-chk") {
        let c: serde_json::Value = serde_json::from_str(&core.config_json()).unwrap_or_default();
        report(&format!(
            "LANCHK enabled={} port={} allow_lan={} has_token={}",
            c["lan_enabled"], c["lan_port"], c["lan_allow_lan"], c["lan_has_token"]
        ));
        return;
    }
    // `--history-delete <id>`：自测历史删除（UI 上「删除选中」走同一条 FFI 路径）
    if let Some(pos) = args.iter().position(|a| a == "--history-delete") {
        let id = args.get(pos + 1).cloned().unwrap_or_default();
        let before = core.history_json(200);
        let n0 = serde_json::from_str::<serde_json::Value>(&before)
            .ok()
            .and_then(|v| v.as_array().map(|a| a.len()))
            .unwrap_or(0);
        let (rc, err) = core.delete_history(&id);
        let after = core.history_json(200);
        let n1 = serde_json::from_str::<serde_json::Value>(&after)
            .ok()
            .and_then(|v| v.as_array().map(|a| a.len()))
            .unwrap_or(0);
        report(&format!("HISTDEL id={id} rc={rc} {err} before={n0} after={n1}"));
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--decode-wav") {
        let path = args.get(pos + 1).cloned().unwrap_or_default();
        let (rc, err) = core.decode_wav(&path);
        report(&format!("DECODE rc={rc} {err}"));
        std::thread::sleep(std::time::Duration::from_millis(2500));
        return;
    }
    if let Some(pos) = args.iter().position(|a| a == "--mic-test") {
        let secs: i32 = args.get(pos + 1).and_then(|s| s.parse().ok()).unwrap_or(3);
        report(&format!("MICTEST 录音 {secs}s…"));
        // 先清掉启动时恢复的上次结果，否则「什么都没听到」也会显示成刚听到的（I-156）
        core.clear_last_result();
        core.debug_record(secs);
        std::thread::sleep(std::time::Duration::from_secs(secs as u64 + 4));
        let got = core.last_result();
        report(&format!(
            "MICTEST OK 结果={}",
            if got.trim().is_empty() { "（没听到内容）" } else { got.as_str() }
        ));
        return;
    }
    if args.iter().any(|a| a == "--settings-input-selftest") {
        // R-137：设置页输入自测（键入 5 字 + 往自己窗口粘贴）
        report(&settings::input_selftest());
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--inject-test-hold") {
        // 先起完整进程（钩子/浮层/迷你窗都在），过几秒再注入，用来区分「注入实现坏了」和「事件处理里注入不行」
        let secs: u64 = args.get(i + 1).and_then(|x| x.parse().ok()).unwrap_or(8);
        let text = args.get(i + 2).cloned().unwrap_or_else(|| "延迟注入自测".to_string());
        std::thread::sleep(std::time::Duration::from_secs(secs));
        let (status, detail) = inject::paste(&text, "paste");
        report(&format!("INJECTHOLD status={status} detail={detail}"));
        std::thread::sleep(std::time::Duration::from_millis(400));
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--inject-test") {
        // 只测注入本身：把后面的参数当文本，直接走 inject::paste，不碰录音/识别
        let text = args.get(i + 1).cloned().unwrap_or_else(|| "注入自测文字".to_string());
        // `--mode keystroke`：直接验「逐字打字」这条路径（默认仍是粘贴）
        let mode = args
            .iter()
            .position(|a| a == "--mode")
            .and_then(|j| args.get(j + 1).cloned())
            .unwrap_or_else(|| "paste".to_string());
        let (status, detail) = inject::paste(&text, &mode);
        report(&format!("INJECT status={status} detail={detail}"));
        std::thread::sleep(std::time::Duration::from_millis(400));
        return;
    }
    if args.iter().any(|a| a == "--self-test") {
        // 自测不能污染用户历史（戴宗：托盘菜单「自测…」也走这里）
        core.set_test_mode(true);
        core.debug_submit_text("自测：你好，Windows。");
        std::thread::sleep(std::time::Duration::from_millis(800));
        report(&format!("SELFTEST OK 结果={}", core.last_result()));
        return;
    }

    // 单实例（审计 P1）：已经有 Eryu 在跑时，不要再装第二套键盘钩子和托盘图标——
    // 两份会同时抢热键，用户看到的却是「按了没反应」。把已有实例的设置窗口叫出来就退。
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
        let class = wide("EryuMessageWindow");
        if let Ok(other) = FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) {
            if !other.0.is_null() {
                let _ = PostMessageW(other, WM_ERYU_REVEAL, WPARAM(0), LPARAM(0));
                let _ = PostMessageW(other, WM_ERYU_SECOND, WPARAM(0), LPARAM(0));
                // 第二份立刻退出：只留一份钩子、一份托盘图标，用户不会看到「两个 Eryu」
                std::process::exit(0);
            }
        }
    }

    let hwnd = create_message_window();
    let overlay = overlay::Overlay::new();
    let spec = load_hotkey(&core);
    // 配置里的热键写法如果解析不了，load_hotkey 会静默退回 ALT_R；GUI 进程看不到
    // stderr，用户在设置里明明写着别的键、实际按的是 Alt。启动时用托盘提示说一句。
    {
        let raw = serde_json::from_str::<serde_json::Value>(&core.config_json())
            .ok()
            .and_then(|v| v["hotkey"].as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        if !raw.trim().is_empty() && hotkey::parse(&raw).is_none() {
            tray::set_tip(&format!("字在：热键「{raw}」看不懂，先用 Alt（右键）说话；到设置里改一下"));
            trace(&format!("hotkey-fallback raw={raw}"));
        }
    }
    *crate::lock(&HOTKEY) = Some(spec);
    reload_slots(&core);
    reload_pause_media(&core);
    *crate::lock(&APP) = Some(App {
        core,
        overlay,
        hwnd,
        media_paused: std::sync::atomic::AtomicBool::new(false),
        phase: String::from("idle"),
    });

    // `--hotwords-suggest`：自测热词建议（走 APP 已初始化后的同一条路径）
    // 设置页冒烟：逐页打开+泵消息，抓崩溃/漏画（UI 改动后的回归用）
    if args.iter().any(|a| a == "--settings-pages-smoke") {
        for p in 0..settings::page_count() {
            settings::open_at(p);
            pump_messages(0.35);
            report(&format!("PAGES page={p} ok"));
        }
        report("PAGES done");
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--settings-cycle") {
        let n = args
            .get(i + 1)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(20);
        settings::open_at(0);
        crate::pump_messages(0.4);
        report(&settings::self_test_gdi_cycles(n));
        return;
    }
    if args.iter().any(|a| a == "--hotwords-suggest-selftest") {
        let v = settings::debug_suggest_selftest();
        for (w, n) in &v {
            report(&format!("SUGSEL {w} x{n}"));
        }
        report(&format!("SUGSEL count={} ok={}", v.len(), !v.is_empty()));
        return;
    }
    if args.iter().any(|a| a == "--hotwords-suggest") {
        for (w, n) in settings::debug_suggested_words() {
            report(&format!("SUG {w} x{n}"));
        }
        report("SUG done");
        return;
    }

    hotkey_hook::set_target(hwnd.0 as isize);
    unsafe {
        register_raw_input(hwnd);
    if !hotkey_hook::install() {
        eprintln!("[eryu] 热键钩子不可用：按住说话不会触发");
    }
        tray::install(hwnd);
        if args.iter().any(|a| a == "--tray-drop-test") {
            let (dropped, back) = tray::debug_drop_and_ensure();
            report(&format!("TRAYDROP dropped={dropped} back={back}"));
        }
        crate::trace(&format!(
            "tray-installed taskbar-created msg={:#x}",
            tray::taskbar_created_msg()
        ));
        let timer = SetTimer(hwnd, tray::TRAY_TIMER_ID, 5000, None);
        trace(&format!("tray-timer set id={} token={}", tray::TRAY_TIMER_ID, timer));
        recover_pending_once();
        warn_bad_config_once();
        mini::install();
        let mini_on = {
            let app = crate::lock(&APP);
            app.as_ref()
                .map(|a| {
                    let cfg = a.core.config_json();
                    serde_json::from_str::<serde_json::Value>(&cfg)
                        .ok()
                        .and_then(|v| v.get("mini_window").and_then(|x| x.as_bool()))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        };
        if mini_on {
            mini::show();
        }
    }

    // 事件回调：把 JSON 原样投递给 UI 线程（回调可能在任意线程）
    {
        // 不 unwrap：万一 APP 还没放进全局，这里 panic 会让整个进程消失（用户眼里
        // 就是「启动后什么都没有」）。没有就跳过回调注册，事件只在 CLI 可见。
        if let Some(a) = crate::lock(&APP).as_ref() {
            a.core.set_event_cb(hwnd);
        }
    }

    // 开机自启：把配置里的开关和 Run 注册表对齐（容错，不阻塞启动）
    {
        let launch = {
            let app = crate::lock(&APP);
            app.as_ref()
                .and_then(|a| serde_json::from_str::<serde_json::Value>(&a.core.config_json()).ok())
                .and_then(|v| v["launch_at_login"].as_bool())
                .unwrap_or(false)
        };
        autostart::reconcile(launch);
    }

    // 首次启动：模型没装就在后台拉（断点续传，重复调用无害）
    {
        let diag = { crate::lock(&APP).as_ref().map(|a| a.core.diagnose()).unwrap_or_default() };
        if diag.contains("SenseVoice 就绪=false") {
            tray::set_tip("字在：正在下载语音模型…");
            std::thread::spawn(|| {
                let app = crate::lock(&APP);
                if let Some(a) = app.as_ref() {
                    let _ = a.core.pull_model("sense");
                }
            });
        }
    }

    if args.iter().any(|a| a == "--polish-prompt-reset") {
        settings::open_at(5);
        pump_messages(0.6);
        settings::self_test_prompt("");
        return;
    }
    if args.iter().any(|a| a == "--polish-prompt-selftest") {
        // 打开润色页 → 选中第一档 → 写入一句话 → 保存 → 读回确认
        settings::open_at(5);
        pump_messages(0.7);
        settings::self_test_prompt("SELFTEST 指令：把口语整理成书面语。");
        std::thread::sleep(std::time::Duration::from_millis(300));
        return;
    }

    // `--settings-perapp-selftest`：走真实的「按应用覆盖」保存路径
    if args.iter().any(|a| a == "--settings-perapp-selftest") {
        let r = settings::self_test_per_app("notepad.exe=打字\r\nchrome.exe=粘贴\r\n# 注释行忽略");
        report(&r);
        std::thread::sleep(std::time::Duration::from_millis(300));
        return;
    }

    // 联调用：模拟按住右 Alt 说话（真机/VM 上没法远程注键时用它跑通整条链路）
    if args.iter().any(|a| a == "--hotkey-sim") {
        let secs: u64 = args
            .iter()
            .position(|a| a == "--hotkey-sim-secs")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(4);
        // 默认模拟右 Alt（0xA5）；配置里换成别的键时用 --hotkey-sim-vk 指定，
        // 否则 handle_key 会被快捷键匹配过滤掉，什么也不会发生。
        let sim_vk = args
            .iter()
            .position(|a| a == "--hotkey-sim-vk")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| {
                let t = v.trim().trim_start_matches("0x").trim_start_matches("0X");
                u32::from_str_radix(t, 16).ok()
            })
            .unwrap_or(0xA5);
        let force = args.iter().any(|a| a == "--hotkey-sim-force");
        // 连续多轮（每轮：按下 → 采 secs 秒 → 松开 → 等 2.5 秒识别）
        let repeat: u32 = args
            .iter()
            .position(|a| a == "--hotkey-sim-repeat")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        // 测试钩子：把「按下热键」推迟到界面准备好、测试脚本把目标窗口点成前台之后再发生。
        let delay_ms: u64 = args
            .iter()
            .position(|a| a == "--hotkey-sim-delay")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(1500);
        std::thread::spawn(move || {
            if force {
                // 自测用：把内存里的快捷键临时改成右 Alt，免得配置里换过键（如 CTRL+ALT+SPACE）
                // 时模拟按键被匹配过滤掉，看起来像「录音链路坏了」。
                if let Ok(mut g) = HOTKEY.lock() {
                    *g = crate::hotkey::parse("ALT_R");
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            for round in 1..=repeat {
                report(&format!("SIM hotkey_down vk={sim_vk:#x} round={round}"));
                handle_key(sim_vk, false, true);
                std::thread::sleep(std::time::Duration::from_secs(secs));
                report("SIM hotkey_up");
                handle_key(sim_vk, true, true);
                if round < repeat {
                    std::thread::sleep(std::time::Duration::from_millis(2500));
                }
            }
        });
    }

    // 设置窗口自测：写入 500ms 并读回（验证「点保存」的真实代码路径）
    if args.iter().any(|a| a == "--polish-save-test") {
        let dir = config_dir();
        let c = match crate::core::Core::load(&dir) {
            Ok(c) => c,
            Err(e) => {
                report(&format!("POLISHSAVE load_failed={e}"));
                return;
            }
        };
        let (code, err) = c.set_config(
            r#"{"polish_enabled":true,"polish_endpoint":"https://api.deepseek.com","polish_model":"deepseek-chat","polish_api_key":"sk-test-123"}"#,
        );
        let v: serde_json::Value = serde_json::from_str(&c.config_json()).unwrap_or_default();
        report(&format!(
            "POLISHSAVE code={code} err={err} enabled={} has_key={} model={}",
            v["polish_enabled"], v["polish_has_key"], v["polish_model"]
        ));
        let (code2, err2) = c.set_config(r#"{"polish_enabled":false,"polish_api_key":""}"#);
        let v2: serde_json::Value = serde_json::from_str(&c.config_json()).unwrap_or_default();
        report(&format!(
            "POLISHRESET code={code2} err={err2} enabled={} has_key={}",
            v2["polish_enabled"], v2["polish_has_key"]
        ));
        return;
    }

    if let Some(i) = args.iter().position(|a| a == "--set-config") {
        if let Some(json) = args.get(i + 1) {
            let (code, err) = {
                let app = crate::lock(&APP);
                match app.as_ref() {
                    Some(a) => a.core.set_config(json),
                    None => (-1, "core 没起来".to_string()),
                }
            };
            // set_config 只是投给 worker 的异步命令；进程立刻退出的话配置还没写盘。
            // 自动化实测多次「code=0 但 config.toml 没变」，就是这里少等了这一下。
            std::thread::sleep(std::time::Duration::from_millis(1200));
            report(&format!("SETCONFIG code={code} err={err}"));
            return;
        }
    }
    // 自测/自动化用：直接设快捷键，绕开 JSON 引号在 cmd 里被吞的问题
    if let Some(i) = args.iter().position(|a| a == "--set-hotkey") {
        if let Some(spec) = args.get(i + 1) {
            let payload = serde_json::json!({ "hotkey": spec }).to_string();
            let (code, err) = {
                let app = crate::lock(&APP);
                match app.as_ref() {
                    Some(a) => a.core.set_config(&payload),
                    None => (-1, "core 没起来".to_string()),
                }
            };
            // set_config 是投给 worker 的异步命令：进程立刻退出的话配置还没落盘，
            // 这里等一下让它 apply_config + save 完成。
            std::thread::sleep(std::time::Duration::from_millis(1200));
            report(&format!("SETHOTKEY spec={spec} code={code} err={err}"));
            return;
        }
    }
    if args.iter().any(|a| a == "--settings-live-test") {
        let r = settings::self_test_all();
        report(&r);
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--obs-diff") {
        if let Some(pair) = args.get(i + 1) {
            if let Some((a, b)) = pair.split_once('|') {
                match observer::diff(a, b) {
                    Some(s) => report(&format!("OBSDIFF from={} to={}", s.from, s.to)),
                    None => report("OBSDIFF none"),
                }
                return;
            }
        }
        report("OBSDIFF usage: --obs-diff <expected>|<actual>");
        return;
    }
    if args.iter().any(|a| a == "--obs-list") {
        for s in observer::load() {
            report(&format!("OBS {} -> {}", s.from, s.to));
        }
        report("OBS done");
        return;
    }
    if args.iter().any(|a| a == "--autostart-on") || args.iter().any(|a| a == "--autostart-off") {
        let on = args.iter().any(|a| a == "--autostart-on");
        let r = autostart::sync(on);
        // 配置里也要记一份，否则下次启动 reconcile 会把注册表改回来
        let (code, err) = {
            let app = crate::lock(&APP);
            match app.as_ref() {
                Some(a) => a
                    .core
                    .set_config(&serde_json::json!({ "launch_at_login": on }).to_string()),
                None => (-1, "core 没起来".to_string()),
            }
        };
        std::thread::sleep(std::time::Duration::from_millis(1200));
        let cur = autostart::current();
        report(&format!(
            "AUTOSTART {} result={r:?} config={code}/{err} current={cur:?}",
            if on { "ON" } else { "OFF" }
        ));
        return;
    }
    if args.iter().any(|a| a == "--autostart-test") {
        let before = autostart::current();
        let on = autostart::sync(true);
        let after_on = autostart::current();
        let off = autostart::sync(false);
        let after_off = autostart::current();
        report(&format!(
            "AUTOSTARTTEST before={before:?} on={on:?} after_on={after_on:?} off={off:?} after_off={after_off:?}"
        ));
        return;
    }
    // 设置页保存前的热键校验，做成 CLI 才能在 VM 里验（单元测试要 link.exe，宿主跑不了）
    if args.iter().any(|a| a == "--hotwords-json") {
        // 打印热词表 JSON（含 error 字段）：VM 里验「文件坏了但规则还在用」
        let json = match crate::lock(&APP).as_ref() {
            Some(a) => a.core.hotwords_json(),
            None => "{}".to_string(),
        };
        report(&format!("HOTWORDS {json}"));
        return;
    }
    if let Some(pos) = args.iter().position(|a| a == "--hotkey-check") {
        let spec = args.get(pos + 1).cloned().unwrap_or_default();
        let ok = hotkey::parse(&spec).is_some();
        report(&format!("HOTKEYCHECK spec={spec:?} ok={ok}"));
        return;
    }
    if args.iter().any(|a| a == "--settings-perf-test") {
        let r = settings::self_test_perf();
        report(&r);
        return;
    }
    if args.iter().any(|a| a == "--settings-save-test") {
        let r = settings::self_test_save(500);
        report(&r);
        return;
    }

    // 首次启动引导（用户要求：新用户先做最基本的设置）。
    // `--onboarding` 强制再走一遍（截图/自测），`--no-onboarding` 跳过。
    let force_onb = args.iter().any(|a| a == "--onboarding")
        || args.iter().any(|a| a == "--onboarding-step");
    // 只跑一条 CLI 就退出的调用（--version/--tray-menu-dump/--diag-*…）不该弹引导窗口：
    // 真机上一条 `--tray-menu-dump` 会把引导窗弹出来，看着像程序自己乱开窗。
    let cli_only = args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--version"
                | "--tray-menu-dump"
                | "--tray-drop-test"
                | "--diag-bundle"
                | "--diag-upload"
                | "--hotkey-check"
                | "--self-test"
                | "--settings-pages-smoke"
                | "--settings-tab-demo"
                | "--settings-key-demo"
                | "--settings-focus-demo"
                | "--settings-cycle"
                | "--dump-config"
                | "--set-config"
                | "--set-hotkey"
                | "--history-delete"
                | "--decode-wav"
                | "--lang"
                | "--perm-report"
                | "--hotwords-json"
                | "--polish-test"
                | "--polish-models"
                | "--mic-test"
        )
    });
    let skip_onb = args.iter().any(|a| a == "--no-onboarding") || cli_only;
    if args.iter().any(|a| a == "--onboarding-step") {
        let _ = std::fs::remove_file(onboarding::done_marker());
    }
    if force_onb && !skip_onb {
        let _ = std::fs::remove_file(onboarding::done_marker());
    }
    // 打开设置窗口/做别的自测时不要再弹引导，否则两个窗口叠在一起
    let opening_settings = args.iter().any(|a| a == "--open-settings")
        || args.iter().any(|a| a == "--open-settings-page");
    if !skip_onb && (!opening_settings || force_onb) {
        {
            let app = crate::lock(&APP);
            let core = app.as_ref().map(|a| &a.core as *const crate::core::Core);
            drop(app);
            if let Some(p) = core {
                // SAFETY: core 在 App 里活到进程结束，这里只借它的地址给引导窗口
                if force_onb {
                    // 显式要求再走一遍：尊重 --onboarding-step 指定的起始步
                    onboarding::open(unsafe { &*p });
                } else {
                    onboarding::open_if_needed(unsafe { &*p });
                }
            }
        }
    }

    // `--open-settings`：启动后直接把设置窗口打开（自测/截图用，不改变正常运行）
    if args.iter().any(|a| a == "--open-settings") {
        settings::open();
    }
    // `--settings-mic-test <秒>`：打开音频页并自动开电平测试，N 秒后退出（自测用）
    if let Some(pos) = args.iter().position(|a| a == "--settings-mic-test") {
        let secs: u64 = args.get(pos + 1).and_then(|s| s.parse().ok()).unwrap_or(5);
        let page: usize = args
            .iter()
            .position(|a| a == "--open-settings-page")
            .and_then(|p| args.get(p + 1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(2);
        {
            settings::open_page(page);
            settings::set_meter_on(true);
            let app = crate::lock(&APP);
            if let Some(a) = app.as_ref() {
                let rc = a.core.meter_start();
                eprintln!("[eryu] settings mic test meter_start rc={rc}");
            }
        }
        // 主线程在这段里要继续泵消息，否则 meter 事件送不到设置页（自测卡住时发现的坑）
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        while std::time::Instant::now() < deadline {
            unsafe {
                let mut msg: windows::Win32::UI::WindowsAndMessaging::MSG =
                    std::mem::zeroed();
                while windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                    &mut msg,
                    None,
                    0,
                    0,
                    windows::Win32::UI::WindowsAndMessaging::PM_REMOVE,
                )
                .as_bool()
                {
                    let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                    windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        std::process::exit(0);
    }

    if let Some(pos) = args.iter().position(|a| a == "--open-settings-page") {
        let page = args.get(pos + 1).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
        settings::open_page(page);
    }

    // `--onboarding-probe-selftest`：只验判定函数（不录音）
    if args.iter().any(|a| a == "--onboarding-probe-selftest") {
        let cases: [(&str, bool); 8] = [
            ("今天天气不错，我们开始吧", true),
            ("今天天气不错", true),
            ("我们开始吧", false),
            ("今天", false),
            ("", false),
            ("今天天气不错！！", true),
            ("嗯那个今天天气不错我们开始吧然后呢", true),
            ("hello world", false),
        ];
        let mut ok = true;
        for (text, want) in cases {
            let got = onboarding::probe_passes(text);
            if got != want {
                ok = false;
            }
            println!(
                "PROBE {} {:?} 期望={} 实际={} 重合率={:.2}",
                if got == want { "OK" } else { "FAIL" },
                text,
                want,
                got,
                onboarding::probe_ratio(text)
            );
        }
        println!(
            "PROBE 例句去标点 {} 字，阈值 0.5",
            onboarding::PROBE_EXAMPLE.chars().filter(|c| c.is_alphanumeric()).count()
        );
        std::process::exit(if ok { 0 } else { 1 });
    }

    // `--mini-toggle`：走一遍托盘「显示主窗」的开关路径（自测）
    if args.iter().any(|a| a == "--mini-toggle") {
        mini::toggle();
        std::thread::sleep(std::time::Duration::from_millis(400));
        report(&format!("MINI shown={}", mini::shown()));
        std::process::exit(0);
    }

    // `--tray-cmd <id>`：直接执行一个托盘菜单命令（自测用；id 见 tray.rs 的 CMD_*）
    if let Some(pos) = args.iter().position(|a| a == "--tray-cmd") {
        let id: usize = args.get(pos + 1).and_then(|v| v.parse().ok()).unwrap_or(0);
        if id == 0 {
            report("USAGE --tray-cmd <id>：1001 复制 1002 诊断 1003 退出 1004 设置 1006 历史 1009 主窗");
        } else {
            let before = crate::mini::shown();
            report(&format!("TRAYCMD start id={id}"));
            // 注意：不要把 `crate::lock(&APP)` 的守卫留在 if-let 条件里——那样整个 if-let 语句
            // 期间都持着 APP 锁，而 run_cmd 里的动作（mini/settings/quit）可能再取同一把锁（I-127 同款自锁）。
            let hwnd = { crate::lock(&APP).as_ref().map(|a| a.hwnd) };
            if let Some(hwnd) = hwnd {
                tray::run_cmd(hwnd, id);
                report(&format!("TRAYCMD ran id={id}"));
            } else {
                report("TRAYCMD 没有运行中的应用实例");
            }
            report(&format!("TRAYCMD id={id} mini_before={before} mini_after={}", crate::mini::shown()));
        }
        std::thread::sleep(std::time::Duration::from_millis(600));
        std::process::exit(0);
    }

    // `--tray-menu-dump`：把托盘菜单建一遍并打印各项（自测菜单结构）
    if args.iter().any(|a| a == "--tray-menu-dump") {
        report(&tray::dump_menu());
        return;
    }

    // `--tray-menu-demo`：把托盘右键菜单在鼠标位置弹出来（自测/截图用）
    if args.iter().any(|a| a == "--tray-menu-demo") {
        let hwnd = crate::lock(&APP).as_ref().map(|a| a.hwnd);
        if let Some(hwnd) = hwnd {
            unsafe {
                let _ = SetCursorPos(700, 400);
                let _ = PostMessageW(hwnd, WM_ERYU_TRAY, WPARAM(0), LPARAM(WM_RBUTTONUP as isize));
            }
        }
    }

    // `--settings-mic-test N`：打开音频页并自动开电平测试 N 秒后退出（自测/截图用）
    if let Some(pos) = args.iter().position(|a| a == "--settings-mic-test") {
        let secs: u64 = args.get(pos + 1).and_then(|v| v.parse().ok()).unwrap_or(6);
        settings::set_meter_on(true);
        if let Some(app) = crate::lock(&APP).as_ref() {
            app.core.meter_start();
        }
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(secs));
            std::process::exit(0);
        });
    }

    trace(&format!("启动：界面就绪 {}ms", t_boot.elapsed().as_millis()));

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe fn register_raw_input(hwnd: HWND) {
    let dev = RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x06, // keyboard
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };
    if RegisterRawInputDevices(&[dev], std::mem::size_of::<RAWINPUTDEVICE>() as u32).is_err() {
        eprintln!("[eryu] Raw Input 注册失败，热键不可用");
    }
}

fn create_message_window() -> HWND {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let class = wide("EryuMessageWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            hIcon: icon::app_icon(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class.as_ptr()),
            PCWSTR(wide("字在").as_ptr()),
            // 顶层（不可见）窗口而不是 message-only：TaskbarCreated 这类系统广播
            // 只发给顶层窗口，message-only 窗口收不到，explorer 重启后托盘图标就回不来了。
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            hinst,
            None,
        )
        .unwrap_or_default()
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_INPUT => {
            let mut size = 0u32;
            GetRawInputData(
                HRAWINPUT(lparam.0 as _),
                RID_INPUT,
                None,
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as u32,
            );
            if size > 0 {
                let mut buf = vec![0u8; size as usize];
                let got = GetRawInputData(
                    HRAWINPUT(lparam.0 as _),
                    RID_INPUT,
                    Some(buf.as_mut_ptr() as *mut _),
                    &mut size,
                    std::mem::size_of::<RAWINPUTHEADER>() as u32,
                );
                if got != u32::MAX {
                    let raw = &*(buf.as_ptr() as *const RAWINPUT);
                    if raw.header.dwType == RIM_TYPEKEYBOARD.0 {
                        let kb = raw.data.keyboard;
                        let is_break = kb.Flags & 0x01 != 0; // RI_KEY_BREAK
                        let is_e0 = kb.Flags & 0x02 != 0; // RI_KEY_E0
                        if std::env::var("ERYU_RAW_DEBUG").is_ok() {
                            eprintln!(
                                "[eryu][raw] vk={:#x} break={} e0={}",
                                kb.VKey, is_break, is_e0
                            );
                        }
                        handle_key(kb.VKey as u32, is_break, is_e0);
                    }
                }
            }
            LRESULT(0)
        }
        WM_ERYU_EVENT => {
            let len = wparam.0;
            let ptr = lparam.0 as *mut u16;
            let json = if len > 0 && !ptr.is_null() {
                unsafe {
                    let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
                    // 释放 post_event/on_event 交接过来的 Box<[u16]>
                    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
                    s
                }
            } else {
                String::new()
            };
            if !json.is_empty() {
                // 窗口过程是 extern "system"，一旦 panic 就不能 unwind → 整个进程直接 abort
                //（I-108 就是这么炸的）。这里兜一层：单条事件处理出错只记日志，界面继续活。
                let kind_brief: String = json.chars().take(40).collect();
                trace(&format!("event-in {kind_brief}"));
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle_event(&json)));
                trace(&format!("event-out {kind_brief}"));
                if r.is_err() {
                    eprintln!("[eryu] 处理事件时出错（已忽略这一条，程序继续）：{}", &json.chars().take(120).collect::<String>());
                }
            }
            LRESULT(0)
        }
        WM_ERYU_KEY => {
            let packed = wparam.0;
            let vk = (packed & 0xFF) as u32;
            let is_break = (packed >> 8) & 1 == 1;
            let is_e0 = (packed >> 9) & 1 == 1;
            handle_key(vk, is_break, is_e0);
            LRESULT(0)
        }
        WM_ERYU_TRAY => {
            tray::on_message(hwnd, lparam);
            LRESULT(0)
        }
        // explorer 重启后任务栏会把所有托盘图标丢掉：重新 NIM_ADD（I-128 / I-160）。
        // 这个值来自 RegisterWindowMessageW，不是编译期常量，只能这样判。
        m if m == tray::taskbar_created_msg() => {
            trace("taskbar-created：重加托盘图标");
            tray::ensure();
            LRESULT(0)
        }
        // 兜底：每 5 秒看一眼图标还在不在（explorer 没广播或广播丢失时也能补回来）
        WM_TIMER if wparam.0 == tray::TRAY_TIMER_ID => {
            trace("tray-timer：复检图标");
            tray::ensure();
            LRESULT(0)
        }
        WM_ERYU_INJECTED => {
            let ptr = lparam.0 as *mut InjectDone;
            if !ptr.is_null() {
                trace("injected-in");
                let r = unsafe { Box::from_raw(ptr) };
                trace("injected-lock-wait");
                if let Ok(mut g) = APP.lock() {
                    trace("injected-locked");
                    if let Some(app) = g.as_mut() {
                        trace("injected-submit");
                        app.core.submit(r.task.as_str(), r.status, &r.detail);
                        trace("injected-submitted");
                        if r.status != "ok" {
                            // 没上屏就把文字留在剪贴板，用户能手动粘（与 mac 的降级一致）
                            let _ = inject::set_text_only(&r.text);
                            app.overlay.error(&r.detail);
                        }
                    }
                }
                // 注意：observer::watch 里会调 `privacy_mode()`，它自己会 `APP.lock()`。
                // 以前这行写在上面那个锁作用域里 → 同线程二次加锁、整个程序卡死
                //（会话结束后 Responding=False，2026-11-21 实测定位）。
                // 所以观察必须在释放 APP 锁之后再做。
                if r.status == "ok" {
                    trace("injected-watch");
                    observer::watch(r.text.clone());
                    trace("injected-watched");
                }
            }
            trace("injected-out");
            LRESULT(0)
        }
        WM_ERYU_SECOND => {
            crate::tray::set_tip("Eryu 已经在运行了：这是同一个实例，托盘图标也在右下角");
            LRESULT(0)
        }
        WM_ERYU_REVEAL => {
            settings::open();
            LRESULT(0)
        }
        // 睡眠唤醒：钩子可能在休眠期间被系统摘掉，重新装一遍，免得醒来后热键没反应
        WM_POWERBROADCAST => {
            let ev = wparam.0 as u32;
            if ev == PBT_APMRESUMEAUTOMATIC || ev == PBT_APMRESUMESUSPEND {
                hotkey_hook::set_target(hwnd.0 as isize);
                if !hotkey_hook::install() {
                    eprintln!("[eryu] 唤醒后重新装热键钩子失败（按热键可能没反应）");
                } else {
                    eprintln!("[eryu] 唤醒：热键钩子已重装");
                }
                unsafe {
                    register_raw_input(hwnd);
                }
            }
            LRESULT(1)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 右 Alt（VK_MENU + E0）= 默认按住说话键；Esc 取消。
/// 从 core 的配置里读热键写法（`hotkey = "ALT_R"`），解析失败退回右 Alt
/// 跑一会儿消息循环（演示/自测用）：窗口要有人泵消息才会画出来。
pub fn pump_messages(secs: f32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f32(secs);
    unsafe {
        while std::time::Instant::now() < deadline {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    }
}

pub fn reload_hotkey() {
    let app = crate::lock(&APP);
    if let Some(app) = app.as_ref() {
        let spec = load_hotkey(&app.core);
        *crate::lock(&HOTKEY) = Some(spec);
        reload_slots(&app.core);
        reload_pause_media(&app.core);
    }
}

/// 从 core 的 profiles JSON 里读「档名 + 图标 + 快捷键槽」。
/// 只收组合键：单键会和「按住说话」抢输入。
fn reload_slots(core: &core::Core) {
    let mut out: Vec<(String, String, hotkey::Spec)> = Vec::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&core.profiles_json()) {
        if let Some(list) = v["profiles"].as_array() {
            for p in list {
                let name = p["name"].as_str().unwrap_or("").to_string();
                let icon = p["icon"].as_str().unwrap_or("").to_string();
                let slot = p["slot"].as_str().unwrap_or("").trim().to_string();
                if name.is_empty() || slot.is_empty() {
                    continue;
                }
                match hotkey::parse(&slot) {
                    Some(sp) if !sp.mods.is_empty() && sp.mods.iter().any(|m| *m >= 0xA0) => {
                        out.push((name, icon, sp));
                    }
                    _ => eprintln!("[eryu] 档「{name}」的快捷键槽 {slot} 不是组合键，已忽略"),
                }
            }
        }
    }
    if let Ok(mut g) = SLOTS.lock() {
        *g = out;
    }
}

pub fn reload_pause_media(core: &core::Core) {
    let on = serde_json::from_str::<serde_json::Value>(&core.config_json())
        .ok()
        .and_then(|v| v["pause_media"].as_bool())
        .unwrap_or(false);
    PAUSE_MEDIA.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn load_hotkey(core: &core::Core) -> hotkey::Spec {
    let raw = serde_json::from_str::<serde_json::Value>(&core.config_json())
        .ok()
        .and_then(|v| v["hotkey"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "ALT_R".to_string());
    hotkey::parse(&raw).unwrap_or_else(|| {
        eprintln!("[eryu] 热键写法无法解析：{raw}，退回 ALT_R");
        hotkey::parse("ALT_R").unwrap()
    })
}

pub(crate) fn handle_key(vkey: u32, is_break: bool, is_e0: bool) {
    let vk = hotkey::normalize(vkey, is_e0);
    // Esc 永远取消
    if vk == 0x1B && !is_break {
        if let Some(app) = crate::lock(&APP).as_ref() {
            app.core.cancel();
        }
        return;
    }

    // Ctrl+Alt+E：选区改写（对齐 macOS 的 ⌘E）
    if !is_break
        && vk == 0x45
        && hotkey::debug_down(0xA2)
        && hotkey::debug_down(0xA4)
    {
        crate::selection::open();
        return;
    }

    // 先看润色档的快捷键槽（只认按下沿；按住说话是主键，优先级更高）
    if !is_break {
        let hit = SLOTS.lock().ok().and_then(|slots| {
            slots.iter().find(|(_, _, sp)| sp.matches(vk, false)).cloned()
        });
        if let Some((name, icon, _)) = hit {
            let app = crate::lock(&APP);
            if let Some(app) = app.as_ref() {
                let rc = app.core.set_active_profile(&name);
                app.overlay.set_profile(&icon, &name);
                crate::tray::set_tip(&format!("字在 · 润色档：{name}"));
                crate::report(&format!("SLOT 切换到 {name} rc={rc}"));
            }
            return;
        }
    }

    let spec = crate::lock(&HOTKEY).clone();
    let Some(spec) = spec else { return };
    let matched = spec.matches(vk, is_break);
    if std::env::var("ERYU_RAW_DEBUG").is_ok() {
        use std::io::Write;
        let path = format!("{}/raw.log", config_dir());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(
                f,
                "handle vk={:#x} break={} spec={} matched={} mods_down={:?}",
                vk, is_break, spec.raw, matched,
                spec.mods.iter().map(|m| hotkey::debug_down(*m)).collect::<Vec<_>>()
            );
        }
    }
    if !matched {
        return;
    }

    if is_break {
        talk_up();
    } else {
        talk_down();
    }
}

/// 开始一段「按住说话」（热键匹配通过时调用）。迷你窗点击走 `talk_from_mini()`。
fn talk_down() {
    trace("talk_down-in");
    let mut held = crate::lock(&HOTKEY_DOWN);
    if *held {
        return; // 钩子会为按住状态反复发按下事件，必须自己去重
    }
    *held = true;
    TALK_SOURCE.store(1, std::sync::atomic::Ordering::Relaxed);
    drop(held);
    talk_start();
}

/// 迷你窗点击开始（没有「松开」事件，所以不参与 HOTKEY_DOWN 去重）
fn talk_from_mini() {
    trace("talk_from_mini");
    {
        let mut held = crate::lock(&HOTKEY_DOWN);
        if *held {
            return;
        }
        *held = true;
        TALK_SOURCE.store(2, std::sync::atomic::Ordering::Relaxed);
    }
    talk_start();
}

/// 真正去让核心开始录一段（调用方已经设好 HOTKEY_DOWN / TALK_SOURCE）
fn talk_start() {
    let fg = foreground_app_name();
    // 记住按下热键那一刻的前台窗口：录音期间浮层/迷你窗可能把前台抢走，
    // 上屏时要能把它找回来（I-113）。
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let h = GetForegroundWindow();
        // 自家浮层/迷你窗当上前台时不覆盖之前的真实目标
        let mut pid = 0u32;
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(h, Some(&mut pid));
        if pid != std::process::id() {
            LAST_TARGET.store(h.0 as isize, std::sync::atomic::Ordering::Relaxed);
        }
    }
    if let Some(app) = crate::lock(&APP).as_ref() {
        let (pname, picon) = app.core.active_profile_pair();
        app.overlay.set_profile(&picon, &pname);
    }
    if std::env::var("ERYU_RAW_DEBUG").is_ok() {
        use std::io::Write;
        let path = format!("{}/raw.log", config_dir());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "hotkey_down app={fg}");
        }
    }
    if let Some(app) = crate::lock(&APP).as_ref() {
        app.core.note_app(&fg);
        app.core.hotkey_down();
    }
    trace("talk_down-out");
}

/// 结束一段「按住说话」
fn talk_up() {
    trace("talk_up-in");
    let mut held = crate::lock(&HOTKEY_DOWN);
    if !*held {
        return;
    }
    *held = false;
    TALK_SOURCE.store(0, std::sync::atomic::Ordering::Relaxed);
    if let Some(app) = crate::lock(&APP).as_ref() {
        app.core.hotkey_up();
    }
    trace("talk_up-out");
}

/// 前台应用的进程名（`notepad.exe`），给 core 做「按应用覆盖上屏方式」用

/// 下载环境的自检：系统代理、DNS/连通性、镜像是否可达（内网排查用）
fn net_check() -> String {
    use std::process::{Command, Stdio};
    let mut out = String::new();
    for key in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
        if let Ok(v) = std::env::var(key) {
            if !v.trim().is_empty() {
                out.push_str(&format!("env {key}={v}\n"));
            }
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut q = |name: &str| {
            let o = Command::new("reg")
                .args([
                    "query",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                    "/v",
                    name,
                ])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x0800_0000)
                .output();
            o.ok().map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        };
        let pick = |t: &str| {
            t.lines()
                .find(|l| l.contains("REG_"))
                .map(|l| l.split("REG_").nth(1).unwrap_or("").trim().to_string())
                .unwrap_or_else(|| "（未设置）".to_string())
        };
        if let Some(t) = q("ProxyEnable") {
            out.push_str(&format!("reg ProxyEnable: {}\n", pick(&t)));
        }
        if let Some(t) = q("ProxyServer") {
            out.push_str(&format!("reg ProxyServer: {}\n", pick(&t)));
        }
        if let Some(t) = q("AutoConfigURL") {
            out.push_str(&format!("reg AutoConfigURL: {}\n", pick(&t)));
        }
    }
    // 用配置文件里的镜像 + 代理做一次 HEAD
    let (mirror, proxy) = {
        let dir = config_dir();
        match crate::core::Core::load(&dir) {
            Ok(c) => {
                let v: serde_json::Value = serde_json::from_str(&c.config_json()).unwrap_or_default();
                (
                    v["model_mirror"].as_str().unwrap_or("").to_string(),
                    v["model_proxy"].as_str().unwrap_or("").to_string(),
                )
            }
            Err(e) => {
                out.push_str(&format!("core 加载失败：{e}\n"));
                (String::new(), String::new())
            }
        }
    };
    out.push_str(&format!("config mirror={mirror}\nconfig proxy={proxy}\n"));
    let url = format!("{}/silero_vad.onnx", mirror.trim_end_matches('/'));
    let mut cmd = Command::new("curl");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    if !proxy.trim().is_empty() {
        cmd.arg("--proxy").arg(proxy.trim());
    }
    let o = cmd
        .args(["-sIL", "--max-time", "20", "-w", "code=%{http_code} time=%{time_total}s", &url])
        .output();
    match o {
        Ok(o) => {
            let so = String::from_utf8_lossy(&o.stdout);
            let se = String::from_utf8_lossy(&o.stderr);
            out.push_str(&format!(
                "curl {url} -> status={} {}\nstderr={}\n",
                o.status,
                so.lines().rev().take(3).collect::<Vec<_>>().join(" | "),
                se.lines().rev().take(2).collect::<Vec<_>>().join(" | ")
            ));
        }
        Err(e) => out.push_str(&format!("curl 启动失败：{e}\n")),
    }
    out
}

fn foreground_app_name() -> String {
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return String::new();
        }
        let mut pid = 0u32;
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_FORMAT(0), windows::core::PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = windows::Win32::Foundation::CloseHandle(h);
        if ok.is_err() {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().unwrap_or("").to_string()
    }
}

/// 上次崩溃/强杀前没上屏的那段文字：核心在 worker 启动那一刻广播 `pending_result`，
/// 但那时事件回调还没挂上，可能丢掉；这里直接读盘兜底，读完就删（避免每次启动都提示）。
fn recover_pending_once() {
    let path = std::path::Path::new(&config_dir())
        .join("pending")
        .join("last_result.txt");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let text = text.trim_start_matches('\u{feff}').trim().to_string();
    if text.is_empty() {
        let _ = std::fs::remove_file(&path);
        return;
    }
    let chars = text.chars().count();
    let ok = inject::set_text_only(&text);
    let _ = std::fs::remove_file(&path);
    PENDING_SHOWN.store(true, std::sync::atomic::Ordering::SeqCst);
    tray::set_tip(&format!(
        "字在：上次有 {chars} 个字没上屏，{}",
        if ok { "已放剪贴板" } else { "复制失败，去托盘取回" }
    ));
    if let Some(app) = crate::lock(&APP).as_ref() {
        app.overlay.error(if ok {
            "上次有一段没上屏的文字，已放剪贴板（Ctrl+V 取回）"
        } else {
            "上次有一段没上屏的文字，但剪贴板写不进去（托盘「复制上次结果」可再试）"
        });
    }
    eprintln!("[eryu] 恢复上次没上屏的文字 {chars} 字（剪贴板={ok}）");
}

/// 模型 id → 用户看得懂的名字（提示语里别出现 gtcrn/punct 这种内部代号）。
pub(crate) fn model_label(id: &str) -> &'static str {
    match id {
        "sense" | "qwen3" => "语音模型",
        "gtcrn" => "降噪模型",
        "punct" => "标点模型",
        "speaker" => "声纹模型",
        "vad" => "静音检测模型",
        _ => "模型",
    }
}

/// 配置解析失败时核心会用默认值起来（并备份成 config.toml.broken），
/// 这里把原因告诉用户——不然用户只会觉得「我的设置全没了」。
fn warn_bad_config_once() {
    let warning = {
        let app = crate::lock(&APP);
        app.as_ref().and_then(|a| {
            serde_json::from_str::<serde_json::Value>(&a.core.config_json())
                .ok()
                .and_then(|v| v["load_warning"].as_str().map(|s| s.to_string()))
        })
    };
    let Some(w) = warning else { return };
    eprintln!("[eryu] {w}");
    let brief: String = w.chars().take(90).collect();
    tray::set_tip(&format!("字在：{brief}"));
    if let Some(app) = crate::lock(&APP).as_ref() {
        app.overlay.error(&brief);
    }
}

fn handle_event(json: &str) {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return,
    };
    let kind = v["type"].as_str().unwrap_or("");
    if std::env::var("ERYU_RAW_DEBUG").is_ok() {
        use std::io::Write;
        let path = format!("{}/raw.log", config_dir());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            // 按字符截断：按字节切会在中文事件里切在 UTF-8 字符中间 → 直接 panic（I-108）
            let brief: String = json.chars().take(160).collect();
            let _ = writeln!(f, "event {brief}");
        }
    }
    let payload = &v["payload"];
    let text = payload["text"].as_str().unwrap_or("").to_string();
    let mut app_lock = crate::lock(&APP);
    let Some(app) = app_lock.as_mut() else { return };
    // 「result」那一支要给历史页加一条：不能在持有 APP 锁时重建页面（build 会再取 APP），
    // 先记个标记，等这把锁放开再刷。
    let mut hist_dirty = false;
    match kind {
        "state" => {
            let phase = payload["phase"].as_str().unwrap_or("idle");
            app.phase = phase.to_string();
            mini::set_phase(phase);
            if phase == "idle" || phase == "done" {
                let mins = today_saved_minutes(&app.core);
                mini::set_saved_minutes(mins);
                // 迷你窗点起来的一段自己结束了（静音/到上限/取消）：没有「松开」事件
                // 来清去重标记，必须在这里放掉，否则下一次点击会被吞（I-161）。
                if TALK_SOURCE.load(std::sync::atomic::Ordering::Relaxed) == 2 {
                    *crate::lock(&HOTKEY_DOWN) = false;
                    TALK_SOURCE.store(0, std::sync::atomic::Ordering::Relaxed);
                    crate::trace("迷你窗一段结束：放掉去重标记");
                }
            }
            // 引导窗开着的时候不弹浮层：用户正在看引导，突然冒个胶囊只会让人分心
            if !onboarding::is_open() {
                app.overlay.show(phase);
            }
            onboarding::note_state(phase);
            // 录音时暂停系统播放（只在设置里开了 pause_media 时）
            let pause_media = PAUSE_MEDIA.load(std::sync::atomic::Ordering::Relaxed);
            if pause_media {
                match phase {
                    "starting" | "listening" => {
                        if app.media_paused.swap(media::pause(), std::sync::atomic::Ordering::SeqCst) {
                            // 已经是「我们暂停的」状态，不重复
                        }
                    }
                    "idle" | "processing" | "inserting" => {
                        if app.media_paused.swap(false, std::sync::atomic::Ordering::SeqCst) {
                            media::resume();
                        }
                    }
                    _ => {}
                }
            }
            tray::set_tip(match phase {
                "starting" | "listening" => "字在：在听…",
                "processing" => "字在：识别中…",
                "inserting" => "字在：上屏中…",
                _ => "字在：就绪（按住右 Alt 说话）",
            });
        }
        "partial_text" => {
            if onboarding::probe_active() {
                onboarding::note_partial(&text);
            } else {
                app.overlay.set_partial(&text);
            }
        }
        "result" => {
            hist_dirty = true;
            app.overlay.set_final(&text);
            tray::set_tip("字在：已上屏");
        }
        "pending_result" => {
            // 上次崩溃/强杀前没来得及上屏的那段文字。核心只是把它读回来并广播，
            // 这里绝不自行注入（可能已经换了前台程序），放剪贴板 + 提示让用户自己取回。
            // 启动兜底 `recover_pending_once()` 可能已经提示过同一条，别重复打扰。
            if PENDING_SHOWN.swap(true, std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            let chars = text.chars().count();
            let ok = inject::set_text_only(&text);
            app.overlay.error(if ok {
                "上次有一段没上屏的文字，已放剪贴板（Ctrl+V 取回）"
            } else {
                "上次有一段没上屏的文字，但剪贴板写不进去（托盘「复制上次结果」可再试）"
            });
            tray::set_tip(&format!(
                "字在：上次有 {chars} 个字没上屏，{}",
                if ok { "已放剪贴板" } else { "复制失败，去托盘取回" }
            ));
        }
        "submit_request" => {
            let task = payload["task_id"].as_str().unwrap_or("").to_string();
            let mode = payload["mode"].as_str().unwrap_or("paste").to_string();
            let text = payload["text"].as_str().unwrap_or("").to_string();
            // 这里**不能**再 `APP.lock()`：上面 `let mut app = APP.lock()` 的
            // MutexGuard 还被 `let Some(app) = app.as_mut()` 的借用压着，
            // 同一线程二次加锁会在第一次上屏后直接死锁（I-104）。
            if onboarding::offer_result(&text) {
                // 引导「试说一句」：只回显给引导窗口，绝不往用户当前程序里注入（D-110）
                eprintln!("[eryu] onboarding probe ok={} len={}", onboarding::probe_passes(&text), text.chars().count());
                app.core.submit(task.as_str(), "ok", "");
                return;
            }
            // 注入既不能在本条事件分发里同步做（I-113：SendInput 被自家钩子阻塞吞掉），
            // 也不能留在 UI 线程上做——UIA SetValue / SetForegroundWindow 在窗口过程里
            // 会等消息泵，前台又是自家窗口时直接互等（本轮实测：会话结束后 Responding=False）。
            // 放到后台线程注入，再把结果 PostMessage 回 UI 线程收尾。
            let hwnd_raw = app.hwnd.0 as isize;
            let task_done = task.clone();
            let text_done = text.clone();
            let mode_done = mode.clone();
            std::thread::spawn(move || {
                trace("inject-thread-in");
                let target = LAST_TARGET.load(std::sync::atomic::Ordering::Relaxed);
                let (status, detail) = inject::paste_to(&text_done, &mode_done, target);
                eprintln!("[eryu] 上屏 status={status} detail={detail}");
                trace("inject-thread-posting");
                let payload = Box::new(InjectDone {
                    task: task_done,
                    text: text_done,
                    status,
                    detail,
                });
                unsafe {
                    let _ = PostMessageW(
                        HWND(hwnd_raw as *mut std::ffi::c_void),
                        WM_ERYU_INJECTED,
                        WPARAM(0),
                        LPARAM(Box::into_raw(payload) as isize),
                    );
                }
            });
        }
        "error" => {
            let code = payload["code"].as_str().unwrap_or("");
            diag::note_error(code, payload["message"].as_str().unwrap_or(""));
            // 润色失败不吞：文字照样上屏（原话），但要让用户知道这次没润色（audit 2026-11-21）
            if code == "polish_failed" {
                let msg = payload["message"].as_str().unwrap_or("润色没成功");
                app.overlay.error("润色没成功，这次贴的是原话");
                tray::set_tip(&format!("字在：{msg}"));
            } else {
                let msg = payload["message"].as_str().unwrap_or("出错了");
                // 浮层上只放一句能照着做的话；完整文案（含可用设备列表）进托盘提示。
                // 核心的完整文案要留着：scripts/matrix.sh 会断言它列出了可用设备（mac 同样显示全文）。
                let brief = if code == "mic_unavailable" || code == "mic_unplugged" {
                    // 设备名太长会把这句顶出胶囊，名字留给托盘提示（那里有全文）
                    if msg.contains("找不到输入设备") {
                        "麦克风打不开：在「设置 → 音频」里改回系统默认设备".to_string()
                    } else {
                        "麦克风中断了，已保留中断前的内容".to_string()
                    }
                } else {
                    msg.to_string()
                };
                app.overlay.error(&brief);
                tray::set_tip(&format!("字在：{msg}"));
            }
        }
        "model_progress" => {
            let recv = payload["received"].as_u64().unwrap_or(0);
            let total = payload["total"].as_u64().unwrap_or(0);
            if total > 0 {
                tray::set_tip(&format!("字在：正在下载模型 {}%", recv * 100 / total));
            }
            settings::on_model_progress(recv, total);
            onboarding::on_model_progress(recv, total, payload["id"].as_str().unwrap_or(""), payload["phase"].as_str().unwrap_or(""));
        }
        "model_done" => {
            let id = payload["id"].as_str().unwrap_or("");
            let what = model_label(id);
            // 引导停在模型页时：模型装好了要立刻解除「下一步」的灰态
            if onboarding::is_open() {
                onboarding::refresh_asr();
            }
            tray::set_tip(&format!("字在：{what}已就绪"));
            if id == "sense" || id == "qwen3" {
                app.overlay
                    .error(&format!("{what}下载完成，现在可以按住 {} 说话了", hotkey_hint()));
            } else {
                app.overlay.error(&format!("{what}下载完成"));
            }
        }
        "model_failed" => {
            let msg = payload["message"].as_str().unwrap_or("下载失败");
            tray::set_tip(&format!("字在：模型下载失败（{msg}）"));
            app.overlay.error(&format!("模型下载失败：{msg}"));
            if settings::is_open() {
                let hwnd = app.hwnd;
                defer_message_box(hwnd, format!("模型下载失败：{msg}"));
            }
        }
        "meter" => {
            let rms = payload["rms"].as_f64().unwrap_or(0.0) as f32;
            // 电平事件也会驱动浮层重画；引导开着时别让胶囊跟着冒出来（引导自己画电平条）
            if !onboarding::is_open() {
                let secs = payload["secs"].as_f64().unwrap_or(0.0);
                app.overlay.set_level_secs(rms, secs);
            }
            // 设置页的「输入电平」测试（对齐 macOS 的电平条）
            settings::on_meter(rms);
            // 引导的麦克风自检页（R-102）
            onboarding::on_meter(rms);
        }
        _ => {}
    }
    drop(app_lock);
    if hist_dirty {
        settings::refresh_history_if_open();
    }
}

/// 给 onboarding 用：收起浮层（它没有 overlay 的引用）
pub(crate) fn overlay_hide() {
    if let Some(app) = crate::lock(&APP).as_ref() {
        app.overlay.hide_now();
    }
}

pub fn post_event(json: &str) {
    let app = crate::lock(&APP);
    let Some(app) = app.as_ref() else { return };
    // 字符串所有权交给 UI 线程（WM_ERYU_EVENT 分支里 Box::from_raw 释放）
    let boxed: Box<[u16]> = json.encode_utf16().collect();
    let len = boxed.len();
    let ptr = Box::into_raw(boxed) as *const u16;
    let ok = unsafe {
        PostMessageW(app.hwnd, WM_ERYU_EVENT, WPARAM(len), LPARAM(ptr as isize))
    };
    if ok.is_err() {
        unsafe { drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u16, len))) };
    }
}

pub fn hotkey_down() {
    let app = crate::lock(&APP);
    if let Some(app) = app.as_ref() {
        app.core.hotkey_down();
    }
}

pub fn hotkey_up() {
    let app = crate::lock(&APP);
    if let Some(app) = app.as_ref() {
        app.core.hotkey_up();
    }
}

/// 托盘菜单里的「复制」：只写剪贴板，不注入
pub fn inject_copy(text: &str) -> bool {
    inject::set_text_only(text)
}

/// 托盘「测试麦克风 3 秒」：录一句、给结果（mac 侧托盘也有这一项）
/// 权限状态：麦克风隐私（注册表 ConsentStore）+ 另外两项在 Windows 上的实际口径
pub fn perm_report_text() -> String {
    let mic = crate::settings::mic_privacy_state();
    let mut out = String::from("权限状态\n\n");
    out.push_str(&format!("麦克风：{mic}\n"));
    out.push_str("  要改：设置 → 隐私和安全性 → 麦克风 → 允许桌面应用访问\n\n");
    out.push_str("键盘监听：不需要单独授权（低级键盘钩子随程序运行）\n");
    out.push_str("模拟按键：往以管理员身份运行的程序里输入会被系统拦下（UIPI）\n");
    out.push_str("  如果目标程序是管理员窗口，Eryu 也要以管理员身份运行。\n");
    out
}

/// 托盘「自测…」：另起一个 --self-test 进程跑一遍再弹结果。
/// 不在这里直接调 core：debug_submit_text 是异步的，持 APP 锁 sleep 会卡 UI。
pub fn selftest_async(hwnd_raw: isize) {
    let _ = std::thread::spawn(move || {
        let exe = std::env::current_exe().ok();
        let mut text = String::from("自测没跑起来（找不到自己的 exe）");
        if let Some(exe) = exe {
            let out = std::process::Command::new(exe).arg("--self-test").output();
            text = match out {
                Ok(o) => {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    let line = stdout
                        .lines()
                        .find(|l| l.contains("SELFTEST"))
                        .unwrap_or("（没有输出结果）");
                    format!("{}（退出码 {}）", line.trim(), o.status.code().unwrap_or(-1))
                }
                Err(e) => format!("自测进程起不来：{e}"),
            };
        }
        crate::defer_message_box(
            windows::Win32::Foundation::HWND(hwnd_raw as *mut std::ffi::c_void),
            text,
        );
    });
}

pub fn mic_test_3s(hwnd_raw: isize) {
    // HWND 不是 Send：跨线程只传 isize，进线程再拼回 HWND
    std::thread::spawn(move || {
        let hwnd = windows::Win32::Foundation::HWND(hwnd_raw as *mut std::ffi::c_void);
        {
            let app = crate::lock(&APP);
            match app.as_ref() {
                Some(a) => {
                    // 同 I-156：清掉恢复的旧结果，测麦只认这次录到的
                    a.core.clear_last_result();
                    a.core.debug_record(3);
                }
                None => return,
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(6));
        let text = {
            let app = crate::lock(&APP);
            match app.as_ref() {
                Some(a) => a.core.last_result(),
                None => String::new(),
            }
        };
        crate::tray::set_tip("字在：麦克风测试完成");
        if text.trim().is_empty() {
            crate::defer_message_box(
                hwnd,
                "麦克风测试：没听到明显的说话声或识别为空。\n\n换一个输入设备，或到「设置 → 音频」看电平条有没有起伏。".to_string(),
            );
        } else {
            crate::defer_message_box(hwnd, format!("麦克风测试结果：{text}"));
        }
    });
}

pub fn copy_last() -> String {
    let app = crate::lock(&APP);
    app.as_ref().map(|a| a.core.last_result()).unwrap_or_default()
}

pub fn quit() {
    hotkey_hook::uninstall();
    tray::remove();
    unsafe {
        PostQuitMessage(0);
    }
}

pub fn message_box_yesno(text: &str) -> bool {
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(wide(text).as_ptr()),
            PCWSTR(wide(APP_TITLE).as_ptr()),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES
    }
}

/// 自测/自动化模式：不弹模态框（模态框会等人点，脚本就卡死了）。
pub static QUIET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 自动化勾选「我明白」：只跳过润色同意弹窗，不影响其它对话框
pub static ASSUME_CONSENT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn message_box(text: &str) {
    if QUIET.load(std::sync::atomic::Ordering::Relaxed) {
        eprintln!("[eryu][quiet] {text}");
        return;
    }
    unsafe {
        MessageBoxW(None, PCWSTR(wide(text).as_ptr()), PCWSTR(wide(APP_TITLE).as_ptr()), MB_OK | MB_ICONWARNING);
    }
}

/// 会自己关掉的提示框：用户再点一次图标想进来，不该被一个必须点「确定」的框挡住。
/// 关法：到点后枚举本进程的 `#32770` 对话框，发 WM_CLOSE（模态框收到就当「确定」返回）。
pub fn message_box_auto_close(text: &str, after: std::time::Duration) {
    if QUIET.load(std::sync::atomic::Ordering::Relaxed) {
        eprintln!("[eryu][quiet] {text}");
        return;
    }
    OUR_PID.store(unsafe { windows::Win32::System::Threading::GetCurrentProcessId() }, std::sync::atomic::Ordering::Relaxed);
    std::thread::spawn(move || {
        std::thread::sleep(after);
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(
                Some(close_our_dialogs),
                LPARAM(0),
            );
        }
    });
    unsafe {
        MessageBoxW(None, PCWSTR(wide(text).as_ptr()), PCWSTR(wide(APP_TITLE).as_ptr()), MB_OK | MB_ICONINFORMATION);
    }
}

static OUR_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

unsafe extern "system" fn close_our_dialogs(hwnd: windows::Win32::Foundation::HWND, _l: LPARAM) -> windows::Win32::Foundation::BOOL {
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId, IsWindowVisible};
    let mut cls = [0u16; 32];
    let n = GetClassNameW(hwnd, &mut cls);
    if n > 0 && String::from_utf16_lossy(&cls[..n as usize]) == "#32770" {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == OUR_PID.load(std::sync::atomic::Ordering::Relaxed) && IsWindowVisible(hwnd).as_bool() {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
    windows::Win32::Foundation::BOOL(1)
}

/// 在**不持 APP 锁**的地方弹框：模态框自己会转消息循环，若调用方还攥着 APP 锁，
/// 模态期间来的 WM_ERYU_EVENT 会在同一线程二次 `APP.lock()` → 界面永久卡死（I-104 审计 P0-2）。
/// 所以从事件处理里弹框一律走这个：丢给别的线程，等 UI 线程把锁放掉再弹。
pub fn defer_message_box(owner: HWND, text: String) {
    let owner = owner.0 as isize; // HWND 不是 Send，转成整数跨线程
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        unsafe {
            MessageBoxW(
                HWND(owner as *mut _),
                PCWSTR(wide(&text).as_ptr()),
                PCWSTR(wide(APP_TITLE).as_ptr()),
                MB_OK | MB_ICONWARNING,
            );
        }
    });
}
