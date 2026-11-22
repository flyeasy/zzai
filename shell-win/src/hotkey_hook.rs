//! 全局热键：低级键盘钩子（`WH_KEYBOARD_LL`）。
//!
//! 为什么不用 Raw Input：实测（2026-11-19，Windows 11 26200）在 session 1 里
//! `RegisterRawInputDevices(RIDEV_INPUTSINK)` 明明返回成功，但合成键（`keybd_event`/`SendInput`）
//! 一条 `WM_INPUT` 都不来，热键链路无法自测；低级钩子对合成键同样回调，且带
//! `LLKHF_EXTENDED`（分左右 Alt/Ctrl）和 `LLKHF_UP`（松开），正好满足「按住说话」。
//! 代价：钩子回调必须极快（只做判定 + PostMessage 回 UI 线程），不能阻塞。

use std::sync::atomic::{AtomicIsize, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, KBDLLHOOKSTRUCT, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    WH_KEYBOARD_LL,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

const LLKHF_EXTENDED: u32 = 0x01;
const LLKHF_UP: u32 = 0x80;
/// 合成输入的标记。自己的上屏注入（Ctrl+V）也会走这个钩子；
/// 如果钩子回调还落在我们被阻塞的 UI 线程上，超过 LowLevelHooksTimeout
/// 系统会把这段输入丢掉，表现为「粘贴了但什么都没进去」（I-113）。
/// 所以合成键一律放过：热键只认人手按下的。
const LLKHF_INJECTED: u32 = 0x10;


static mut HOOK: isize = 0;

/// 钩子回调只往这个窗口投一条消息（UI 线程再慢慢处理）。
/// 回调里绝不能做任何阻塞操作：超 `LowLevelHooksTimeout`（约 1s）Windows 会静默把钩子摘掉，
/// 之后热键就「按了没反应」了（I-104 审计 P0-1）。
static HOOK_TARGET: AtomicIsize = AtomicIsize::new(0);

/// 主窗口建好后调一次：把键事件的目标窗口告诉钩子
pub fn set_target(hwnd: isize) {
    HOOK_TARGET.store(hwnd, Ordering::Relaxed);
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let flags = kb.flags.0;
        // 合成键（我们自己的注入、别的工具模拟）不当作热键
        if flags & LLKHF_INJECTED != 0 {
            return CallNextHookEx(None, code, wparam, lparam);
        }
        let is_break = flags & LLKHF_UP != 0;
        let is_e0 = flags & LLKHF_EXTENDED != 0;
        // 只打包 + 投递，不做任何逻辑（见 HOOK_TARGET 注释）
        if std::env::var("ERYU_RAW_DEBUG").is_ok() {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(format!("{}/raw.log", crate::config_dir()))
            {
                let _ = writeln!(f, "hook vk={:#x} break={} target={:#x}", kb.vkCode, is_break, HOOK_TARGET.load(Ordering::Relaxed));
            }
        }
        let target = HOOK_TARGET.load(Ordering::Relaxed);
        if target != 0 {
            let packed = (kb.vkCode as usize & 0xFF)
                | ((is_break as usize) << 8)
                | ((is_e0 as usize) << 9);
            unsafe {
                let _ = PostMessageW(
                    HWND(target as *mut _),
                    crate::WM_ERYU_KEY,
                    WPARAM(packed),
                    LPARAM(0),
                );
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// 装钩子；返回是否成功
pub fn install() -> bool {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let h = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), hinst, 0);
        match h {
            Ok(h) => {
                HOOK = h.0 as isize;
                true
            }
            Err(e) => {
                eprintln!("[eryu] 键盘钩子安装失败：{e}（热键不可用）");
                false
            }
        }
    }
}

/// 退出前把钩子摘掉：进程被结束虽然系统会回收，但显式摘更干净，
/// 也避免「退出后键盘仍被拦一下」这类怪现象。
pub fn uninstall() {
    unsafe {
        if HOOK != 0 {
            let _ = UnhookWindowsHookEx(windows::Win32::UI::WindowsAndMessaging::HHOOK(HOOK as *mut core::ffi::c_void));
            HOOK = 0;
        }
    }
}
