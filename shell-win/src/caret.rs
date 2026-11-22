//! 找「文字会插到哪里」：读前台窗口的插入光标位置（W2：浮层跟着光标走）。
//!
//! 用 `GetGUIThreadInfo` 读**前台线程**的 caret——它跨进程可用，
//! 比 UIA 便宜得多，也不需要额外的 COM 初始化。

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetGUIThreadInfo, GetWindowRect, GetWindowThreadProcessId,
    GUITHREADINFO,
};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::Foundation::GetLastError;

/// 光标在屏幕下半部分时，浮层应翻到上方（与 overlay::place 的判定同口径）
fn near_bottom(y: i32) -> bool {
    unsafe {
        let h = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN,
        );
        h > 0 && y > h * 2 / 3
    }
}

#[derive(Debug)]
pub struct Anchor {
    /// 光标右下角（屏幕坐标，物理像素）
    pub x: i32,
    pub y: i32,
    /// true = 这行文字在屏幕下半部分，浮层应该翻到上面
    pub flip: bool,
}

/// 读前台窗口的插入光标。
/// 读不到（游戏、终端、全屏应用、无焦点）就回退到前台窗口底部中心，再不行给 None。
pub fn caret_anchor() -> Option<Anchor> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        let tid = GetWindowThreadProcessId(fg, Some(&mut pid));
        // 跨进程读 caret 需要先 attach 到目标线程的输入队列
        if tid != 0 {
            let me = GetCurrentThreadId();
            let _ = AttachThreadInput(me, tid, true);
        }
        if tid != 0 {
            let mut gti = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            if GetGUIThreadInfo(tid, &mut gti).is_ok() && !gti.hwndCaret.0.is_null() {
                // caret rect 是 hwndCaret 的客户区坐标
                let mut pt = POINT { x: gti.rcCaret.left, y: gti.rcCaret.bottom };
                if ClientToScreen(gti.hwndCaret, &mut pt).as_bool() {
                    let _ = AttachThreadInput(GetCurrentThreadId(), tid, false);
                    return Some(Anchor { x: pt.x, y: pt.y, flip: near_bottom(pt.y) }); // via=caret
                }
            }
        }

        let _ = AttachThreadInput(GetCurrentThreadId(), tid, false);
        // 回退 1：前台窗口底部中心（输入法候选、自绘控件常见）
        let mut wr = RECT::default();
        if GetWindowRect(fg, &mut wr).is_ok() {
            let cx = (wr.left + wr.right) / 2;
            let y = wr.bottom;
            if y > 0 && cx > 0 {
                return Some(Anchor { x: cx, y, flip: near_bottom(y) });
            }
        }

        // 回退 2：鼠标位置（用户刚点过的地方）
        let mut mp = POINT::default();
        if GetCursorPos(&mut mp).is_ok() {
            return Some(Anchor { x: mp.x, y: mp.y, flip: near_bottom(mp.y) });
        }
        let _ = GetActiveWindow();
        None
    }
}

/// 诊断：分别报告三条路各拿到了什么（--caret-probe 用）
pub fn probe() -> String {
    unsafe {
        let fg = GetForegroundWindow();
        let mut pid = 0u32;
        let tid = GetWindowThreadProcessId(fg, Some(&mut pid));
        let mut gti = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let me = GetCurrentThreadId();
        if tid != 0 { let _ = AttachThreadInput(me, tid, true); }
        let ok = if tid != 0 { GetGUIThreadInfo(tid, &mut gti).is_ok() } else { false };
        let err = GetLastError().0;
        if tid != 0 { let _ = AttachThreadInput(me, tid, false); }
        let caret_hwnd = gti.hwndCaret.0 as isize;
        let mut pt = POINT { x: gti.rcCaret.left, y: gti.rcCaret.bottom };
        let converted = if !gti.hwndCaret.0.is_null() {
            ClientToScreen(gti.hwndCaret, &mut pt).as_bool()
        } else { false };
        let mut wr = RECT::default();
        let _wrok = GetWindowRect(fg, &mut wr).is_ok();
        format!(
            "PROBE fg_thread={tid} gti_ok={ok} err={err} caret_hwnd={caret_hwnd} rc=({},{},{},{}) screen=({},{}) conv={converted} win=({},{},{},{})",
            gti.rcCaret.left, gti.rcCaret.top, gti.rcCaret.right, gti.rcCaret.bottom,
            pt.x, pt.y, wr.left, wr.top, wr.right, wr.bottom
        )
    }
}

/// 当前系统的 DPI 缩放（100% = 96）
pub fn dpi_scale() -> f32 {
    unsafe {
        use windows::Win32::UI::HiDpi::GetDpiForSystem;
        let d = GetDpiForSystem();
        if d == 0 { 1.0 } else { d as f32 / 96.0 }
    }
}
