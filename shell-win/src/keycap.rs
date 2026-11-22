//! 按键录制控件：点一下，然后直接按想用的键，自动识别并显示。
//!
//! 支持：单修饰键（右 Alt / 左 Ctrl / Shift / Win / Fn 类）、修饰键组合、单功能键、
//! 单字母数字键。识别结果就是要写进配置的 spec 字符串（与 core 的 hotkey 解析同构），
//! 例如 `ALT_R` / `CTRL+ALT+SPACE` / `F13` / `K`。

use std::sync::Mutex;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, InvalidateRect,
    RoundRect, SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    CreateFontW, DEFAULT_PITCH, DRAW_TEXT_FORMAT, FF_DONTCARE, HBRUSH, HFONT, OUT_DEFAULT_PRECIS,
    PAINTSTRUCT, DT_LEFT, DT_SINGLELINE, DT_VCENTER, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowLongPtrW, GetParent, GetDlgCtrlID, RegisterClassW,
    SendMessageW, SetWindowLongPtrW,
    SetWindowTextW, ShowWindow, WNDCLASSW, WS_CHILD, WS_VISIBLE, GWLP_USERDATA, SW_SHOW,
};

const C_BG: u32 = 0x001F1C1B;
const C_EDIT: u32 = 0x0038302E;
const C_CARD: u32 = 0x002C2624;
const C_TEXT: u32 = 0x00ECE9E9;
const C_MUTED: u32 = 0x00A6A09A;
const C_ACCENT: u32 = 0x00F6823B;
const C_LINE: u32 = 0x00453D3A;

static CLASS_REGISTERED: Mutex<bool> = Mutex::new(false);

struct State {
    spec: String,
    capturing: bool,
    font: HFONT,
    k: f32,
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn state(h: HWND) -> Option<&'static mut State> {
    unsafe {
        let p = GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State;
        p.as_mut()
    }
}

/// 把当前按键状态拼成 spec（规则与 `hotkey::parse` 一致：修饰键在前，主键在最后）
fn spec_from(vk: u16, extended: bool) -> Option<String> {
    let down = |k: i32| unsafe { (GetKeyState(k) as u16 & 0x8000) != 0 };
    // 主键本身不参与修饰键判断（否则单按 Ctrl 会被算成 CTRL+CTRL）
    let is_mod = matches!(
        vk as u32,
        0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5
    );

    // 组合键用不分左右的 CTRL / ALT / SHIFT / WIN（与 mac 的写法一致）：
    // 录下 `ALT_R+CTRL_R+SPACE` 只会让「必须按右键」成立，用户换一边按就没反应。
    let mut parts: Vec<String> = Vec::new();
    if !is_mod || vk == 0xA2 || vk == 0xA3 {
        if down(VK_CONTROL.0 as i32) {
            parts.push("CTRL".to_string());
        }
    }
    if !is_mod || vk == 0xA4 || vk == 0xA5 {
        if down(VK_MENU.0 as i32) {
            parts.push("ALT".to_string());
        }
    }
    if !is_mod || vk == 0xA0 || vk == 0xA1 {
        if down(VK_SHIFT.0 as i32) {
            parts.push("SHIFT".into());
        }
    }
    if down(VK_LWIN.0 as i32) || down(VK_RWIN.0 as i32) {
        parts.push("WIN".into());
    }

    let main = match vk as u32 {
        0x10 => Some("SHIFT".to_string()),
        0x11 => Some(if unsafe { down(0xA3) } { "CTRL_R" } else { "CTRL_L" }.to_string()),
        0x12 => Some(if extended { "ALT_R" } else { "ALT_L" }.to_string()),
        0x5B => Some("WIN".to_string()),
        0x5C => Some("WIN_R".to_string()),
        0x20 => Some("SPACE".to_string()),
        0x09 => Some("TAB".to_string()),
        0x0D => Some("ENTER".to_string()),
        0x1B => Some("ESC".to_string()),
        0x26 => Some("UP".to_string()),
        0x28 => Some("DOWN".to_string()),
        0x25 => Some("LEFT".to_string()),
        0x27 => Some("RIGHT".to_string()),
        0x70..=0x87 => Some(format!("F{}", vk as u32 - 0x6F)),
        0x30..=0x39 => Some(((vk as u8) as char).to_string()),
        0x41..=0x5A => Some(((vk as u8) as char).to_string()),
        _ => None,
    }?;
    // 单修饰键：只要它自己，不要把「按住它」算成组合
    if is_mod && parts.iter().all(|p| !p.starts_with(main.as_str())) {
        return Some(main);
    }
    parts.push(main);
    Some(parts.join("+"))
}

unsafe fn paint(h: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(h, &mut ps);
    let mut rc = RECT::default();
    windows::Win32::UI::WindowsAndMessaging::GetClientRect(h, &mut rc).ok();
    let k = state(h).map(|s| s.k).unwrap_or(1.0);
    let focused = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() } == h;

    let bg = CreateSolidBrush(COLORREF(C_EDIT));
    FillRect(hdc, &rc, bg);
    DeleteObject(bg);
    let pen = windows::Win32::Graphics::Gdi::CreatePen(
        windows::Win32::Graphics::Gdi::PS_SOLID,
        if focused { 2 } else { 1 },
        COLORREF(if focused { C_ACCENT } else { C_LINE }),
    );
    let old = SelectObject(hdc, pen);
    let br = windows::Win32::Graphics::Gdi::GetStockObject(
        windows::Win32::Graphics::Gdi::NULL_BRUSH,
    );
    let oldb = SelectObject(hdc, br);
    let r = (k * 6.0) as i32;
    RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, r, r);
    SelectObject(hdc, oldb);
    SelectObject(hdc, old);
    DeleteObject(pen);

    let st = state(h);
    let (text, color) = match st {
        None => (String::new(), C_TEXT),
        Some(s) => {
            if s.capturing {
                ("按下要用的键…".to_string(), C_MUTED)
            } else if s.spec.is_empty() {
                ("点这里，然后按键".to_string(), C_MUTED)
            } else {
                (pretty(&s.spec), C_TEXT)
            }
        }
    };
    if let Some(s) = state(h) {
        SelectObject(hdc, s.font);
    }
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, COLORREF(color));
    let mut buf = w(&text);
    let n = buf.len().saturating_sub(1);
    let mut tr = rc;
    tr.left += (k * 10.0) as i32;
    tr.right -= (k * 8.0) as i32;
    DrawTextW(hdc, &mut buf[..n], &mut tr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    EndPaint(h, &ps);
}

/// 显示成用户看得懂的样子：ALT_R → 右 Alt，CTRL+ALT+SPACE → Ctrl+Alt+空格
fn pretty(spec: &str) -> String {
    fn key_name(k: &str) -> String {
        match k {
            "SPACE" => "空格".into(),
            "ENTER" => "Enter".into(),
            "TAB" => "Tab".into(),
            "ESC" => "Esc".into(),
            "UP" => "↑".into(),
            "DOWN" => "↓".into(),
            "LEFT" => "←".into(),
            "RIGHT" => "→".into(),
            other => other.to_string(),
        }
    }
    let parts: Vec<&str> = spec.split('+').collect();
    if parts.len() == 1 {
        return match spec {
            "ALT_R" => "右 Alt".into(),
            "ALT_L" => "左 Alt".into(),
            "CTRL_R" => "右 Ctrl".into(),
            "CTRL_L" => "左 Ctrl".into(),
            "SHIFT" => "Shift".into(),
            "WIN" => "Win".into(),
            other => key_name(other),
        };
    }
    let mut out = String::new();
    for p in &parts[..parts.len() - 1] {
        out.push_str(match *p {
            "CTRL" | "CTRL_L" | "CTRL_R" => "Ctrl+",
            "ALT" | "ALT_L" | "ALT_R" => "Alt+",
            "SHIFT" => "Shift+",
            "WIN" => "Win+",
            _ => "",
        });
    }
    out.push_str(&key_name(parts[parts.len() - 1]));
    out
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_GETDLGCODE, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_NCDESTROY, WM_PAINT,
        WM_SETFOCUS, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };
    const DLGC_WANTALLKEYS: usize = 0x0004;
    match msg {
        WM_PAINT => {
            unsafe { paint(h) };
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            unsafe {
                SetFocus(h);
                if let Some(s) = state(h) {
                    s.capturing = true;
                }
                InvalidateRect(h, None, false);
            }
            LRESULT(0)
        }
        WM_SETFOCUS => {
            unsafe {
                if let Some(s) = state(h) {
                    s.capturing = true;
                }
                InvalidateRect(h, None, false);
            }
            LRESULT(0)
        }
        WM_KILLFOCUS => {
            unsafe {
                if let Some(s) = state(h) {
                    s.capturing = false;
                }
                InvalidateRect(h, None, false);
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            // 每次 settings::rebuild_page 都会重建控件；不在这里回收就是每次泄漏一个
            // State + HFONT（I-104 审计 P0-5）。
            unsafe {
                let p = GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State;
                if !p.is_null() {
                    SetWindowLongPtrW(h, GWLP_USERDATA, 0);
                    let st = Box::from_raw(p);
                    let _ = DeleteObject(st.font);
                }
            }
            LRESULT(0)
        }
        WM_GETDLGCODE => LRESULT(DLGC_WANTALLKEYS as isize),
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
            let vk = (wp.0 & 0xFFFF) as u16;
            let extended = (lp.0 as usize & (1 << 24)) != 0;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let is_mod = matches!(vk as u32, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5);
            unsafe {
                let Some(s) = state(h) else { return LRESULT(0) };
                if !s.capturing {
                    return LRESULT(0);
                }
                // 单修饰键：等松开再定（按下时其它修饰键还没就位，但也允许 shift+alt 组合）
                if down {
                    if let Some(spec) = spec_from(vk, extended) {
                        if !is_mod {
                            s.spec = spec;
                            s.capturing = false;
                            InvalidateRect(h, None, false);
                            let parent = unsafe { GetParent(h) }.unwrap_or_default();
                            unsafe {
                                SendMessageW(
                                    parent,
                                    windows::Win32::UI::WindowsAndMessaging::WM_COMMAND,
                                    WPARAM((GetDlgCtrlID(h) & 0xFFFF) as usize),
                                    LPARAM(h.0 as isize),
                                );
                            }
                        }
                    }
                } else if is_mod {
                    if let Some(spec) = spec_from(vk, extended) {
                        s.spec = spec;
                        s.capturing = false;
                        InvalidateRect(h, None, false);
                        let parent = unsafe { GetParent(h) }.unwrap_or_default();
                        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                            parent,
                            windows::Win32::UI::WindowsAndMessaging::WM_COMMAND,
                            WPARAM((windows::Win32::UI::WindowsAndMessaging::GetDlgCtrlID(h) & 0xFFFF) as usize),
                            LPARAM(h.0 as isize),
                        );
                    }
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(h, msg, wp, lp) },
    }
}

pub fn ensure_class() {
    let mut done = crate::lock(&CLASS_REGISTERED);
    if *done {
        return;
    }
    unsafe {
        let hi = GetModuleHandleW(None).unwrap_or_default();
        let name = w("EryuKeyCap");
        let cls = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: HINSTANCE(hi.0),
            lpszClassName: PCWSTR(name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&cls);
    }
    *done = true;
}

pub fn font_px(k: f32, pt: f32) -> HFONT {
    let face = w("Segoe UI");
    unsafe {
        CreateFontW(
            -((pt * k).round() as i32),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            1,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            PCWSTR(face.as_ptr()),
        )
    }
}

/// 建一个按键录制框（id 用来在 WM_COMMAND 里识别）
pub fn create(parent: HWND, x: i32, y: i32, cw: i32, ch: i32, id: i32, initial: &str, k: f32) -> HWND {
    ensure_class();
    let hi = unsafe { GetModuleHandleW(None).unwrap_or_default() };
    let cls = w("EryuKeyCap");
    let h = unsafe {
        CreateWindowExW(
            Default::default(),
            PCWSTR(cls.as_ptr()),
            PCWSTR(w("").as_ptr()),
            WS_CHILD | WS_VISIBLE,
            x,
            y,
            cw,
            ch,
            parent,
            windows::Win32::UI::WindowsAndMessaging::HMENU(id as *mut core::ffi::c_void),
            HINSTANCE(hi.0),
            None,
        )
    }
    .unwrap_or_default();
    let st = Box::new(State {
        spec: initial.to_string(),
        capturing: false,
        font: font_px(k, 14.0),
        k,
    });
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(st) as isize);
        ShowWindow(h, SW_SHOW);
    }
    h
}

pub fn text(h: HWND) -> String {
    state(h).map(|s| s.spec.clone()).unwrap_or_default()
}

pub fn set(h: HWND, spec: &str) {
    if let Some(s) = state(h) {
        s.spec = spec.to_string();
    }
    unsafe {
        let _ = SetWindowTextW(h, PCWSTR(w(spec).as_ptr()));
        InvalidateRect(h, None, false);
    }
}

#[allow(dead_code)]
fn _unused(_: u32, _: HBRUSH, _: u32) {
    let _ = (C_BG, C_CARD);
}
