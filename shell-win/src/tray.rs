//! 托盘图标 + 右键菜单（W1：状态提示、复制上次结果、诊断、退出）。

use std::sync::Mutex;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const ICON_ID: u32 = 1;
/// 托盘自检定时器（见 `ensure`）
pub const TRAY_TIMER_ID: usize = 7;
const CMD_COPY: usize = 1001;
const CMD_SELECTION: usize = 1005;
const CMD_SETTINGS: usize = 1004;
const CMD_DIAG: usize = 1002;
const CMD_QUIT: usize = 1003;
const CMD_HISTORY: usize = 1006;
const CMD_CANCEL: usize = 1007;
const CMD_ABOUT: usize = 1008;
const CMD_MINI: usize = 1009;
const CMD_MICTEST: usize = 1010;
const CMD_PERM: usize = 1011;
const CMD_PERM_STATUS: usize = 1013;
const CMD_SELFTEST: usize = 1014;

static TIP: Mutex<String> = Mutex::new(String::new());
/// 托盘图标所在的窗口；set_tip 要用它发 NIM_MODIFY 才会真的刷新（只改字符串不通知系统没用）
static TRAY_HWND: Mutex<isize> = Mutex::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn icon_data() -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW::default();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.uID = ICON_ID;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = crate::WM_ERYU_TRAY;
    let own = crate::icon::app_icon();
    nid.hIcon = if own.is_invalid() {
        unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() }
    } else {
        own
    };
    let tip = crate::lock(&TIP);
    let text = if tip.is_empty() { "字在：按住右 Alt 说话" } else { tip.as_str() };
    let t = wide(text);
    let n = t.len().min(128);
    nid.szTip[..n].copy_from_slice(&t[..n]);
    nid
}

/// explorer.exe 重启（崩溃恢复、注销重登）后系统会把所有托盘图标清掉，
/// 并给每个顶层窗口广播 `TaskbarCreated`。收到它必须重新 NIM_ADD，
/// 否则用户会觉得「图标自己没了、右键点不到」。
static TASKBAR_CREATED: Mutex<u32> = Mutex::new(0);

pub fn taskbar_created_msg() -> u32 {
    let mut m = crate::lock(&TASKBAR_CREATED);
    if *m == 0 {
        let name = wide("TaskbarCreated");
        *m = unsafe { RegisterWindowMessageW(PCWSTR(name.as_ptr())) };
    }
    *m
}

/// 重新加一遍托盘图标（`TaskbarCreated` 之后调用）
pub fn reinstall() {
    let hwnd_raw = *crate::lock(&TRAY_HWND);
    if hwnd_raw == 0 {
        return;
    }
    unsafe {
        let mut nid = icon_data();
        nid.hWnd = HWND(hwnd_raw as *mut std::ffi::c_void);
        let ok = Shell_NotifyIconW(NIM_ADD, &nid).as_bool();
        crate::trace(&format!("taskbar-created re-add icon ok={ok}"));
    }
}

/// 兜底：explorer 重启后图标会丢。广播不一定收得到（VM 里实测 explorer 重启也不发），
/// 所以每 5 秒用 NIM_MODIFY 探一下：失败说明图标没了，补一次 NIM_ADD。
pub fn ensure() {
    let hwnd_raw = *crate::lock(&TRAY_HWND);
    if hwnd_raw == 0 {
        return;
    }
    unsafe {
        let mut nid = icon_data();
        nid.hWnd = HWND(hwnd_raw as *mut std::ffi::c_void);
        nid.uFlags = NIF_TIP;
        if !Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool() {
            crate::trace("tray-ensure：图标不在了（NIM_MODIFY 失败），重新 NIM_ADD");
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            if Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                crate::trace("tray-ensure re-added icon");
            }
        }
    }
}

/// 自测：主动删掉图标再走一次 `ensure()`，验证「图标没了会自动补回来」
pub fn debug_drop_and_ensure() -> (bool, bool) {
    let hwnd_raw = *crate::lock(&TRAY_HWND);
    if hwnd_raw == 0 {
        return (false, false);
    }
    unsafe {
        let mut nid = icon_data();
        nid.hWnd = HWND(hwnd_raw as *mut std::ffi::c_void);
        let dropped = Shell_NotifyIconW(NIM_DELETE, &nid).as_bool();
        nid.uFlags = NIF_TIP;
        let gone = !Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool();
        ensure();
        nid.uFlags = NIF_TIP;
        let back = Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool();
        (dropped && gone, back)
    }
}

pub fn install(hwnd: HWND) {
    *crate::lock(&TRAY_HWND) = hwnd.0 as isize;
    unsafe {
        let mut nid = icon_data();
        nid.hWnd = hwnd;
        Shell_NotifyIconW(NIM_ADD, &nid);
    }
}


/// 菜单项统一在这里建（右键弹菜单与自测 dump 共用一份，避免两处漂移）
unsafe fn fill_menu(menu: HMENU, phase_text: &str, last: &str, failed: &[usize]) {
    let _ = AppendMenuW(
        menu,
        MF_STRING | MF_DISABLED | MF_GRAYED,
        0,
        PCWSTR(wide(&format!("字在 · {phase_text}")).as_ptr()),
    );
    if !last.is_empty() {
        let total = last.chars().count();
        let mut short: String = last.chars().take(24).collect();
        if total > 24 {
            short.push('…');
        }
        let _ = AppendMenuW(
            menu,
            MF_STRING | MF_DISABLED | MF_GRAYED,
            0,
            PCWSTR(wide(&format!("上次：{short}（{total} 字）")).as_ptr()),
        );
    }
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let listening = matches!(phase_text, "准备中…" | "在听…（松开结束）");
    let _ = AppendMenuW(
        menu,
        if listening { MF_STRING } else { MF_STRING | MF_GRAYED },
        CMD_CANCEL,
        PCWSTR(wide("取消当前录音（Esc）").as_ptr()),
    );
    let _ = AppendMenuW(menu, MF_STRING, CMD_COPY, PCWSTR(wide("复制上次结果").as_ptr()));
    let _ = AppendMenuW(menu, MF_STRING, CMD_HISTORY, PCWSTR(wide("统计与历史…").as_ptr()));
    let _ = AppendMenuW(menu, MF_STRING, CMD_SETTINGS, PCWSTR(wide("设置…").as_ptr()));
    let _ = AppendMenuW(menu, MF_STRING, CMD_SELECTION, PCWSTR(wide("选区改写…（Ctrl+Alt+E）").as_ptr()));
    let _ = AppendMenuW(menu, MF_STRING, CMD_DIAG, PCWSTR(wide("诊断…").as_ptr()));
    if !failed.is_empty() {
        let sub = CreatePopupMenu().unwrap_or_default();
        for idx in failed {
            let label = format!("第 {} 段", idx + 1);
            let _ = AppendMenuW(sub, MF_STRING, 1200 + *idx, PCWSTR(wide(&label).as_ptr()));
        }
        let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(wide("重识别失败的段").as_ptr()));
    }
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, CMD_ABOUT, PCWSTR(wide("关于 Eryu").as_ptr()));
    let _ = AppendMenuW(menu, MF_STRING, CMD_MINI, PCWSTR(wide("显示主窗").as_ptr()));
    if crate::dev_mode() {
        // 「自测…」是排障口，不是功能：发版包里不露，需要时用 --dev 启动
        let _ = AppendMenuW(menu, MF_STRING, CMD_SELFTEST, PCWSTR(wide("自测…").as_ptr()));
    }
    // 测麦 / 权限这些在「设置 → 音频」「设置 → 诊断」里都有，菜单里再放一份只会让用户问「这是干嘛的」
    let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, PCWSTR(wide("退出").as_ptr()));
}

/// 自测：建一遍菜单并把每一项的文字读回来（`--tray-menu-dump`），验证菜单结构与文案
pub fn dump_menu() -> String {
    unsafe {
        let menu = CreatePopupMenu().unwrap_or_default();
        fill_menu(menu, "就绪", "", &[]);
        let n = GetMenuItemCount(menu);
        let mut out = String::new();
        for i in 0..n.max(0) {
            let mut buf = [0u16; 256];
            let len = GetMenuStringW(menu, i as u32, Some(&mut buf), MF_BYPOSITION);
            if len > 0 {
                let used = len as usize;
                out.push_str(&String::from_utf16_lossy(&buf[..used]));
                out.push('\n');
            } else {
                out.push_str("---\n");
            }
        }
        let _ = DestroyMenu(menu);
        out
    }
}

pub fn set_tip(text: &str) {
    *crate::lock(&TIP) = text.to_string();
    // 之前这里只记字符串、从不通知系统 → 托盘提示/下载进度/失败原因永远不刷新（I-104 审计 P1）。
    let h = *crate::lock(&TRAY_HWND);
    if h != 0 {
        unsafe {
            let mut nid = icon_data();
            nid.hWnd = HWND(h as *mut core::ffi::c_void);
            nid.uFlags = NIF_TIP;
            Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }
}

/// 退出时把图标删掉，否则鼠标划过残留图标会一直有幽灵项
pub fn remove() {
    let h = *crate::lock(&TRAY_HWND);
    if h != 0 {
        unsafe {
            let mut nid = NOTIFYICONDATAW::default();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.uID = ICON_ID;
            nid.hWnd = HWND(h as *mut core::ffi::c_void);
            Shell_NotifyIconW(NIM_DELETE, &nid);
        }
        *crate::lock(&TRAY_HWND) = 0;
    }
}

pub fn on_message(hwnd: HWND, lparam: windows::Win32::Foundation::LPARAM) {
    let event = lparam.0 as u32;
    // 双击托盘 = 打开设置（Windows 习惯）；单击仍然弹菜单
    if event == WM_LBUTTONDBLCLK {
        crate::settings::open();
        return;
    }
    if event != WM_RBUTTONUP && event != WM_LBUTTONUP {
        return;
    }
    // 状态与最近结果先取出来，弹菜单期间不持锁（P0-2 的教训）
    let (phase, last) = {
        let g = crate::lock(&crate::APP);
        match g.as_ref() {
            Some(app) => (app.phase.clone(), app.core.last_result()),
            None => (String::from("idle"), String::new()),
        }
    };
    let phase_text = match phase.as_str() {
        "starting" => "准备中…",
        "listening" => "在听…（松开结束）",
        "processing" | "decoding" => "识别中…",
        "inserting" => "上屏中…",
        "polishing" => "润色中…",
        _ => "就绪",
    };
    let listening = matches!(phase.as_str(), "starting" | "listening");
    let failed = crate::APP
        .lock()
        .ok()
        .and_then(|a| a.as_ref().map(|a| a.core.failed_segments()))
        .unwrap_or_default();
    unsafe {
        let menu = CreatePopupMenu().unwrap_or_default();
        fill_menu(menu, phase_text, &last, &failed);
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, None);
        let _ = DestroyMenu(menu);
        run_cmd(hwnd, cmd.0 as usize);
    }
}

/// 托盘菜单命令的执行体。菜单点击和 `--tray-cmd <id>` 自测入口都走这里，
/// 这样命令行能验到「显示主窗 / 设置 / 退出」这些只在菜单里出现过的动作。
pub fn run_cmd(hwnd: HWND, cmd: usize) {
    {
        match cmd {
            CMD_COPY => {
                let text = crate::copy_last();
                if text.is_empty() {
                    set_tip("字在：还没有上次结果");
                } else if !crate::inject_copy(&text) {
                    set_tip("字在：剪贴板被别的程序占着，没复制上");
                } else {
                    set_tip("字在：上次结果已复制");
                }
            }
            CMD_SETTINGS => crate::settings::open(),
            CMD_HISTORY => crate::settings::open_page(4),
            CMD_CANCEL => {
                if let Some(app) = crate::lock(&crate::APP).as_ref() {
                    app.core.cancel();
                }
            }
            CMD_MINI => {
                crate::mini::toggle();
            }
            CMD_MICTEST => {
                crate::mic_test_3s(hwnd.0 as isize);
            }
            CMD_SELFTEST => {
                crate::selftest_async(hwnd.0 as isize);
            }
            CMD_PERM => {
                // 与诊断页同一个入口：系统设置里的麦克风隐私页
                let _ = std::process::Command::new("cmd")
                    .args(["/c", "start", "", "ms-settings:privacy-microphone"])
                    .spawn();
            }
            CMD_PERM_STATUS => {
                crate::defer_message_box(hwnd, crate::perm_report_text());
            }
            CMD_ABOUT => {
                crate::defer_message_box(
                    hwnd,
                    format!(
                        "字在（zzai） {}（Windows 预览版）\n本机识别，不联网。\n\n按住热键说话，松开后文字打进当前光标位置；按 Esc 或再按一下热键可取消。\n托盘图标右键可以设置、看历史、复制上次结果。\n\n问题与日志：%APPDATA%\\Eryu\\eryu-shell.log",
                        crate::PRODUCT_VERSION
                    ),
                );
            }
            CMD_DIAG => {
                // diagnose() 可能在网络/子进程里花时间，弹框不能持 APP 锁（P0-2）
                let (owner, d) = {
                    let g = crate::lock(&crate::APP);
                    match g.as_ref() {
                        Some(app) => (app.hwnd, app.core.diagnose()),
                        None => (hwnd, String::new()),
                    }
                };
                if !d.is_empty() {
                    crate::defer_message_box(owner, d);
                }
            }
            CMD_SELECTION => crate::selection::open(),
            CMD_QUIT => crate::quit(),
            c if c >= 1200 => {
                let idx = (c - 1200) as i32;
                let (owner, rc, err) = {
                    let g = crate::lock(&crate::APP);
                    match g.as_ref() {
                        Some(app) => {
                            let (rc, err) = app.core.retry_segment(idx);
                            (app.hwnd, rc, err)
                        }
                        None => (hwnd, -1, String::from("没有运行中的实例")),
                    }
                };
                if rc == 0 {
                    set_tip(&format!("字在：正在重识别第 {} 段", idx + 1));
                } else {
                    crate::defer_message_box(owner, format!("重识别第 {} 段失败：{}", idx + 1, err));
                }
            }
            _ => {}
        }
    }
}
