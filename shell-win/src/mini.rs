//! 迷你主窗（DESIGN §12.3，对齐 mac 的 R-103）。
//!
//! 170×230 的小窗：一个大的麦克风圆钮 + 一行提示 + 今日节省 + 关闭。
//! 点按钮 = 开始/停止录音（只作用于按钮；全局「按一下开始/再按停止」是 R-110 的事）。
//! 关掉窗口会把 `mini_window` 写成 false，托盘「显示主窗」再打开。

use std::sync::Mutex;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    InvalidateRect, RoundRect, SelectObject, SetBkMode, SetTextColor, DT_CENTER, DT_SINGLELINE,
    DT_VCENTER, FW_BOLD, FW_NORMAL, HBRUSH, HDC, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos, IsWindow,
    IsWindowVisible, LoadCursorW, RegisterClassW, ShowWindow, CS_HREDRAW, CS_VREDRAW, HTCLIENT,
    IDC_ARROW, SW_HIDE, SW_SHOWNA, WM_DESTROY, WM_LBUTTONUP, WM_NCHITTEST, WM_PAINT,
    WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

const W: i32 = 170;
const H: i32 = 230;
/// 当前 DPI 百分比（100 = 96 DPI），建窗时写入，供命中测试使用
static SCALE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(100);

static MINI: Mutex<isize> = Mutex::new(0);

/// 界面状态：待机 / 在听 / 识别中 / 今日节省
struct Ui {
    phase: String,
    saved_min: i32,
}

static UI: Mutex<Ui> = Mutex::new(Ui {
    phase: String::new(),
    saved_min: 0,
});

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn hwnd() -> Option<HWND> {
    let h = *crate::lock(&MINI);
    if h != 0 && unsafe { IsWindow(HWND(h as *mut _)).as_bool() } {
        Some(HWND(h as *mut _))
    } else {
        None
    }
}

pub fn shown() -> bool {
    hwnd().map(|h| unsafe { IsWindowVisible(h).as_bool() }).unwrap_or(false)
}

/// 建窗（不显示）；由 `main` 在启动时调一次
pub fn install() {
    unsafe {
        let class = wide("EryuMini");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | windows::Win32::UI::WindowsAndMessaging::CS_DROPSHADOW,
            lpfnWndProc: Some(wndproc),
            hInstance: windows::Win32::Foundation::HINSTANCE(
                windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                    .unwrap_or_default()
                    .0,
            ),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);

        // 右下角（工作区，避开任务栏）——第一次显示时用
        let mut work = RECT::default();
        if windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            windows::Win32::UI::WindowsAndMessaging::SPI_GETWORKAREA,
            0,
            Some(&mut work as *mut _ as *mut _),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
        {
            let k = crate::ui::scale(HWND(std::ptr::null_mut()));
            let k = if k <= 0.0 { 1.0 } else { k };
            SCALE.store((k * 100.0).round() as i32, std::sync::atomic::Ordering::Relaxed);
            let ww = crate::ui::px(k, W as f32);
            let hh = crate::ui::px(k, H as f32);
            let x = work.right - ww - 18;
            // 往上让 60px：未激活 Windows 的水印窗口固定在右下角，别被它盖住
            let y = work.bottom - hh - 78;
            if let Ok(h) = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                PCWSTR(class.as_ptr()),
                PCWSTR(wide("字在").as_ptr()),
                // 不带 WS_VISIBLE：创建时就可见的置顶窗会抢前台（I-113 的根因之一），
                // 之后统一用 SW_SHOWNA 显示，绝不激活。
                WS_POPUP,
                x,
                y,
                ww,
                hh,
                None,
                None,
                windows::Win32::Foundation::HINSTANCE(
                    windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                        .unwrap_or_default()
                        .0,
                ),
                None,
            ) {
                *crate::lock(&MINI) = h.0 as isize;
                let rgn = windows::Win32::Graphics::Gdi::CreateRoundRectRgn(
                    0,
                    0,
                    ww + 1,
                    hh + 1,
                    crate::ui::px(k, 14.0),
                    crate::ui::px(k, 14.0),
                );
                let _ = windows::Win32::Graphics::Gdi::SetWindowRgn(h, rgn, true);
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
    }
}

pub fn show() {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = ShowWindow(h, SW_SHOWNA);
            let _ = InvalidateRect(h, None, true);
        }
    }
}

pub fn hide() {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
}

pub fn toggle() {
    // 托盘「显示主窗」/ 再次打开：把选择写回配置，下次启动按这个来（与 mac 的 `[ui] mini_window` 一致）
    if shown() {
        hide();
        crate::settings::set_config_quiet(serde_json::json!({ "mini_window": false }));
    } else {
        show();
        crate::settings::set_config_quiet(serde_json::json!({ "mini_window": true }));
    }
}

pub fn close() {
    hide();
    crate::report("MINI closed（托盘「显示主窗」可以再打开）");
    // 记一笔，下次启动不再自动弹出（"稍后再开"的语义）
    crate::settings::set_config_quiet(serde_json::json!({ "mini_window": false }));
}

pub fn set_phase(p: &str) {
    crate::lock(&UI).phase = p.to_string();
    if let Some(h) = hwnd() {
        unsafe {
            let _ = InvalidateRect(h, None, true);
        }
    }
}

pub fn set_saved_minutes(m: i32) {
    crate::lock(&UI).saved_min = m;
    if let Some(h) = hwnd() {
        unsafe {
            let _ = InvalidateRect(h, None, true);
        }
    }
}

fn listening(phase: &str) -> bool {
    phase == "starting" || phase == "listening"
}

fn busy(phase: &str) -> bool {
    matches!(phase, "decoding" | "processing" | "inserting" | "polishing")
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let (phase, saved) = {
        let ui = crate::lock(&UI);
        (ui.phase.clone(), ui.saved_min)
    };
    let on = listening(&phase);
    let k = crate::ui::scale(hwnd);
    let s = |v: f32| crate::ui::px(k, v);

    // 背景：圆角卡片（窗口本身也做了圆角 region，这里再画一层底，四角不会出现直角）
    let bg = CreateSolidBrush(COLORREF(0x001C1C1F));
    let pen = windows::Win32::Graphics::Gdi::CreatePen(
        windows::Win32::Graphics::Gdi::PS_SOLID,
        1,
        COLORREF(0x002A2A2E),
    );
    let old_pen = SelectObject(hdc, pen);
    let old_brush = SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(bg.0));
    let _ = RoundRect(hdc, 0, 0, rc.right, rc.bottom, s(14.0), s(14.0));
    let _ = SelectObject(hdc, old_brush);
    let _ = SelectObject(hdc, old_pen);
    let _ = DeleteObject(pen);
    let _ = DeleteObject(bg);

    // 圆钮
    let r = s(38.0);
    let cx = rc.right / 2;
    let cy = s(92.0);
    let brush = CreateSolidBrush(if on { COLORREF(0x002B2BD6) } else { COLORREF(0x003A3A46) });
    let old_brush = SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(brush.0));
    let old_pen = SelectObject(
        hdc,
        windows::Win32::Graphics::Gdi::GetStockObject(windows::Win32::Graphics::Gdi::NULL_PEN),
    );
    let _ = RoundRect(hdc, cx - r, cy - r, cx + r, cy + r, 2 * r, 2 * r);
    let _ = SelectObject(hdc, old_brush);
    let _ = SelectObject(hdc, old_pen);
    let _ = DeleteObject(brush);

    SetBkMode(hdc, TRANSPARENT);

    // 图标：用 GDI 画，别用 emoji/符号字体（GDI 里代理对和缺字会变成方框）
    let white = CreateSolidBrush(COLORREF(0x00F0F0F0));
    let old_b = SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(white.0));
    let old_p = SelectObject(
        hdc,
        windows::Win32::Graphics::Gdi::GetStockObject(windows::Win32::Graphics::Gdi::NULL_PEN),
    );
    if on {
        let half = s(12.0);
        let _ = windows::Win32::Graphics::Gdi::Rectangle(hdc, cx - half, cy - half, cx + half, cy + half);
    } else {
        // Segoe MDL2 Assets 是 Windows 自带的图标字体（不是 emoji），话筒字形稳定
        let icon_font = CreateFontW(
            -s(34.0),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            PCWSTR(wide("Segoe MDL2 Assets").as_ptr()),
        );
        let mut icon_rc = RECT {
            left: cx - s(24.0),
            top: cy - s(24.0),
            right: cx + s(24.0),
            bottom: cy + s(24.0),
        };
        let old_icon = SelectObject(hdc, icon_font);
        SetTextColor(hdc, COLORREF(0x00F0F0F0));
        DrawTextW(hdc, &mut wide("\u{E720}"), &mut icon_rc, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
        SelectObject(hdc, old_icon);
        let _ = DeleteObject(icon_font);
    }
    let _ = SelectObject(hdc, old_b);
    let _ = SelectObject(hdc, old_p);
    let _ = DeleteObject(white);

    // 提示行
    let f_hint = crate::ui::font(k, 12.0, 400);
    let old_f = SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(f_hint.0));
    SetTextColor(hdc, COLORREF(0x00C8C8CC));
    if on {
        let mut rc_hint = RECT { left: s(8.0), top: s(142.0), right: rc.right - s(8.0), bottom: s(166.0) };
        DrawTextW(hdc, &mut wide("正在听，点击停止"), &mut rc_hint, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
    } else if busy(&phase) {
        let mut rc_hint = RECT { left: s(8.0), top: s(142.0), right: rc.right - s(8.0), bottom: s(166.0) };
        DrawTextW(hdc, &mut wide("识别中…"), &mut rc_hint, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
    } else {
        // 组合键名称可能很长，拆成两行，避免被窗口裁掉
        let line1 = format!("按 {}", crate::hotkey_hint());
        let mut rc1 = RECT { left: s(6.0), top: s(138.0), right: rc.right - s(6.0), bottom: s(160.0) };
        DrawTextW(hdc, &mut wide(&line1), &mut rc1, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
        let mut rc2 = RECT { left: s(6.0), top: s(158.0), right: rc.right - s(6.0), bottom: s(180.0) };
        DrawTextW(hdc, &mut wide("或点击开始"), &mut rc2, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
    }

    // 今日节省（数字稍突出，说明文字更小，形成层次）
    let f_saved = crate::ui::font(k, crate::ui::CAPTION, 400);
    SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(f_saved.0));
    SetTextColor(hdc, COLORREF(0x009A9AA0));
    let mut rc_saved = RECT { left: s(8.0), top: s(174.0), right: rc.right - s(8.0), bottom: s(194.0) };
    let saved_line = format!("今日已节省 {saved} 分");
    DrawTextW(hdc, &mut wide(&saved_line), &mut rc_saved, DT_CENTER | DT_SINGLELINE | DT_VCENTER);

    // 设置（齿轮）：底部中间，点开设置窗（和 mac 迷你窗的齿轮一致）
    // 用 Segoe MDL2 Assets（图标字体），雅黑里没有这个字形，会画成方框
    let f_icon = CreateFontW(
        -s(15.0),
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        PCWSTR(wide("Segoe MDL2 Assets").as_ptr()),
    );
    let old_gear = SelectObject(hdc, f_icon);
    SetTextColor(hdc, COLORREF(0x009A9AA0));
    let mut rc_gear = RECT { left: s(0.0), top: s(198.0), right: rc.right, bottom: s(222.0) };
    DrawTextW(hdc, &mut wide("\u{E713}"), &mut rc_gear, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
    let _ = SelectObject(hdc, old_gear);
    let _ = DeleteObject(f_icon);

    // 关闭按钮
    SetTextColor(hdc, COLORREF(0x008A8A90));
    let mut rc_x = RECT { left: rc.right - s(30.0), top: s(6.0), right: rc.right - s(8.0), bottom: s(28.0) };
    DrawTextW(hdc, &mut wide("✕"), &mut rc_x, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
    let _ = SelectObject(hdc, old_f);
    let _ = DeleteObject(f_hint);
    let _ = DeleteObject(f_saved);

    EndPaint(hwnd, &ps);
}

fn in_circle(x: i32, y: i32) -> bool {
    let k = SCALE.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.0;
    let (cx, cy, r) = ((W as f32 * k) as i32 / 2, (92.0 * k) as i32, (44.0 * k) as i32); // 命中区比画出来的圆稍大，好点
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r * r
}

fn in_gear(x: i32, y: i32) -> bool {
    let k = SCALE.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.0;
    let w = (W as f32 * k) as i32;
    let cx = w / 2;
    let cy = (210.0 * k) as i32;
    let r = (18.0 * k) as i32;
    (x - cx).abs() <= r && (y - cy).abs() <= r
}

fn in_close(x: i32, y: i32) -> bool {
    let k = SCALE.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.0;
    let w = (W as f32 * k) as i32;
    (w - (34.0 * k) as i32..=w).contains(&x) && (0..=(30.0 * k) as i32).contains(&y)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            if in_close(x, y) {
                close();
            } else if in_gear(x, y) {
                crate::settings::open();
            } else if in_circle(x, y) {
                crate::mini_toggle_talk();
            }
            LRESULT(0)
        }
        // 空白处可以拖动窗口
        WM_NCHITTEST => {
            let mut pt = windows::Win32::Foundation::POINT::default();
            let _ = GetCursorPos(&mut pt);
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let mut origin = windows::Win32::Foundation::POINT::default();
            windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut origin);
            let x = pt.x - origin.x;
            let y = pt.y - origin.y;
            if in_circle(x, y) || in_close(x, y) || in_gear(x, y) {
                return LRESULT(HTCLIENT as isize);
            }
            LRESULT(windows::Win32::UI::WindowsAndMessaging::HTCAPTION as isize)
        }
        WM_DESTROY => {
            *crate::lock(&MINI) = 0;
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[allow(dead_code)]
pub fn destroy() {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = DestroyWindow(h);
        }
    }
}

#[allow(dead_code)]
fn unused(_: HBRUSH, _: HDC) {}
