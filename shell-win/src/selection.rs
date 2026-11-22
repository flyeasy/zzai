//! 选区改写（对齐 macOS 的 ⌘E 指令面板）
//!
//! 流程：Ctrl+Alt+E → 抓当前选区（剪贴板哨兵法）→ 弹出指令面板
//! → 选中一条指令 → core.run_command（走网络）→ 把结果粘回去。
//!
//! 为什么用剪贴板抓选区：Windows 没有跨进程读选区的通用 API（UIA 的 TextPattern
//! 在浏览器/Electron/自绘控件里经常读不到），而 Ctrl+C 是所有输入框都认的。
//! 代价是要小心翼翼还回用户原来的剪贴板（只在自己内容还在时恢复）。

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetStockObject,
    InvalidateRect, RoundRect, SelectObject, SetBkColor, SetBkMode, SetTextColor, CLEARTYPE_QUALITY,
    DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE, HBRUSH, HDC, HFONT, NULL_BRUSH, PAINTSTRUCT, PEN_STYLE,
    TRANSPARENT,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL,
    VK_RETURN, VK_ESCAPE, VK_DOWN, VK_UP, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetSystemMetrics,
    RegisterClassW, SetForegroundWindow, SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW,
    CS_DROPSHADOW, HWND_TOPMOST, SW_HIDE, SW_SHOW, SM_CXSCREEN, SM_CYSCREEN, WNDCLASSW,
    WM_ACTIVATE, WM_CLOSE, WM_KEYDOWN, WM_PAINT, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::Win32::Graphics::Gdi::DrawTextW;
use windows::Win32::Foundation::POINT;

use crate::report;

const C_BG: u32 = 0x001F1C1B;
const C_CARD: u32 = 0x002C2624;
const C_ACCENT: u32 = 0x00F6823B;
const C_TEXT: u32 = 0x00ECE9E9;
const C_MUTED: u32 = 0x00A6A09A;

pub const CMD_CAPTURE: u32 = 910;

/// 打开面板之前用户所在的那个窗口（粘回时要把焦点还给它）。
/// 面板一显示就会自己抢前台，所以这个值必须在 show_palette **之前**记下来——
/// 在 run() 里再取 GetForegroundWindow() 拿到的是面板自己，粘贴会落进一个已经收起的窗口（P1-8 复发）。
static PREV_FG: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
const SELECTION_SENTINEL: &str = "ERYU-SELECTION-SENTINEL-7F3A";

/// 五条指令（和 macOS 侧一致）
const COMMANDS: [(&str, &str); 5] = [
    (
        "润色",
        "把下面这段文字润色一下：去掉口头禅和重复，保持原意和语气，只输出改写后的正文。",
    ),
    (
        "更正式",
        "把下面这段文字改成更正式、书面化的表达，保留全部信息量和专有名词，只输出改写后的正文。",
    ),
    (
        "精简",
        "把下面这段文字精简到原来的一半左右，只保留关键信息，不改意思，只输出改写后的正文。",
    ),
    (
        "译成英文",
        "把下面这段文字翻译成自然的英文，专有名词保留原文，只输出译文。",
    ),
    (
        "总结要点",
        "把下面这段文字总结成 3 条以内的要点，用短句，只输出要点。",
    ),
];

struct Palette {
    hwnd: HWND,
    font: HFONT,
    font_title: HFONT,
    brush_bg: HBRUSH,
    brush_card: HBRUSH,
    sel: usize,
    text: String,
    k: f32,
}

static PALETTE: Mutex<Option<Palette>> = Mutex::new(None);
unsafe impl Send for Palette {}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn px(k: f32, v: f32) -> i32 {
    (v * k).round() as i32
}

fn dpi_k() -> f32 {
    unsafe {
        let dc = windows::Win32::Graphics::Gdi::GetDC(None);
        let dpi = windows::Win32::Graphics::Gdi::GetDeviceCaps(
            dc,
            windows::Win32::Graphics::Gdi::LOGPIXELSY,
        ) as f32;
        let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, dc);
        if dpi > 0.0 {
            dpi / 96.0
        } else {
            1.0
        }
    }
}

fn font(px_: i32, weight: i32) -> HFONT {
    let face = wide("Microsoft YaHei UI");
    unsafe {
        CreateFontW(
            -px_,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            0,
            0,
            CLEARTYPE_QUALITY.0 as u32,
            DEFAULT_PITCH.0 as u32,
            PCWSTR(face.as_ptr()),
        )
    }
}

// ---------- 剪贴板 ----------

fn clipboard_text() -> Option<String> {
    unsafe {
        if OpenClipboard(None).is_err() {
            return None;
        }
        let h = GetClipboardData(13).ok(); // CF_UNICODETEXT
        let out = h.and_then(|h| {
            let ptr = windows::Win32::System::Memory::GlobalLock(windows::Win32::Foundation::HGLOBAL(
                h.0,
            ));
            if ptr.is_null() {
                return None;
            }
            let s = {
                let p = ptr as *const u16;
                // 畸形剪贴板可能没有结尾 0：按 GlobalSize 封顶，别越界读崩进程
                let size = windows::Win32::System::Memory::GlobalSize(
                    windows::Win32::Foundation::HGLOBAL(h.0),
                ) as usize;
                let cap = (size / 2).max(1);
                let mut n = 0usize;
                while n < cap && *p.add(n) != 0 {
                    n += 1;
                }
                String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
            };
            let _ = GlobalUnlock(windows::Win32::Foundation::HGLOBAL(h.0));
            Some(s)
        });
        let _ = CloseClipboard();
        out
    }
}

fn put_clipboard(text: &str) -> bool {
    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = units.len() * 2;
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes).unwrap_or_default();
        if h.0.is_null() {
            let _ = CloseClipboard();
            return false;
        }
        let ptr = GlobalLock(h);
        if ptr.is_null() {
            let _ = CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(units.as_ptr(), ptr as *mut u16, units.len());
        let _ = GlobalUnlock(h);
        let ok = SetClipboardData(13, windows::Win32::Foundation::HANDLE(h.0)).is_ok();
        let _ = CloseClipboard();
        ok
    }
}

fn key(vk: u16, up: bool) {
    unsafe {
        let mut inp = INPUT::default();
        inp.r#type = INPUT_KEYBOARD;
        inp.Anonymous.ki = KEYBDINPUT {
            wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(vk),
            wScan: 0,
            dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
            time: 0,
            dwExtraInfo: 0,
        };
        let _ = SendInput(&[inp], std::mem::size_of::<INPUT>() as i32);
    }
}

fn send_copy() {
    key(VK_CONTROL.0, false);
    key(0x43 /* C */, false);
    key(0x43, true);
    key(VK_CONTROL.0, true);
}

fn send_paste() {
    key(VK_CONTROL.0, false);
    key(VK_V.0, false);
    key(VK_V.0, true);
    key(VK_CONTROL.0, true);
}

/// 抓当前选区。返回 (文字, 原剪贴板内容)
fn grab_selection() -> Result<(String, Option<String>), String> {
    let original = clipboard_text();
    if !put_clipboard(SELECTION_SENTINEL) {
        return Err("剪贴板被别的程序占着，读不到选区".into());
    }
    send_copy();
    std::thread::sleep(std::time::Duration::from_millis(140));
    let now = clipboard_text().unwrap_or_default();
    // 还原用户原来的剪贴板（只有哨兵还在时才还原，避免覆盖用户新复制的东西）
    if now == SELECTION_SENTINEL {
        if let Some(o) = &original {
            let _ = put_clipboard(o);
        }
        return Err("没读到选中的文字（先选中一段文字再按 Ctrl+Alt+E）".into());
    }
    if let Some(o) = &original {
        let _ = put_clipboard(o);
    }
    if now.trim().is_empty() {
        return Err("选中的是空的".into());
    }
    Ok((now, original))
}

fn fg_now() -> isize {
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow().0 as isize }
}

// ---------- 面板 ----------

unsafe extern "system" fn palette_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_ACTIVATE => {
            // 点到别的地方就收起来，别常驻挡视线（审计 P1-9）
            if (wparam.0 & 0xFFFF) == 0 {
                hide();
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            destroy_palette();
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            if vk == VK_ESCAPE.0 {
                hide();
            } else if vk == VK_RETURN.0 {
                let sel = PALETTE.lock().ok().and_then(|p| p.as_ref().map(|p| p.sel));
                if let Some(i) = sel {
                    run(i);
                }
            } else if vk == VK_DOWN.0 {
                if let Ok(mut g) = PALETTE.lock() {
                    if let Some(p) = g.as_mut() {
                        p.sel = (p.sel + 1) % COMMANDS.len();
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                }
            } else if vk == VK_UP.0 {
                if let Ok(mut g) = PALETTE.lock() {
                    if let Some(p) = g.as_mut() {
                        p.sel = (p.sel + COMMANDS.len() - 1) % COMMANDS.len();
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                }
            } else if (0x31..=0x35).contains(&vk) {
                run((vk - 0x31) as usize);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn paint(hwnd: HWND) {
    let Ok(g) = PALETTE.lock() else { return };
    let Some(p) = g.as_ref() else { return };
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        crate::report(&format!("SELPAINT hwnd={:?} text={}", hwnd.0, p.text.chars().count()));
        let mut rc = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc);
        let _ = FillRect(hdc, &rc, p.brush_bg);
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SelectObject(hdc, p.font_title);
        let _ = SetTextColor(hdc, COLORREF(C_MUTED));
        // 顶部说明
        let mut title = wide(&format!("改写选中的 {} 字 · ↑↓ 选择，回车执行，Esc 取消", p.text.chars().count()));
        let mut trc = RECT {
            left: px(p.k, 16.0),
            top: px(p.k, 12.0),
            right: rc.right - px(p.k, 16.0),
            bottom: px(p.k, 32.0),
        };
        let r1 = DrawTextW(hdc, &mut title, &mut trc, windows::Win32::Graphics::Gdi::DT_LEFT);
        crate::report(&format!("SELPAINT rc=({},{},{},{}) draw1={} err={:?}", rc.left, rc.top, rc.right, rc.bottom, r1, windows::Win32::Foundation::GetLastError()));
        // 预览
        let _ = SelectObject(hdc, p.font);
        let _ = SetTextColor(hdc, COLORREF(C_TEXT));
        let mut preview: Vec<u16> = p.text.chars().take(60).collect::<String>().encode_utf16().collect();
        let mut pr = RECT {
            left: px(p.k, 16.0),
            top: px(p.k, 34.0),
            right: rc.right - px(p.k, 16.0),
            bottom: px(p.k, 30.0 + 22.0 * 2.0),
        };
        let _ = DrawTextW(
            hdc,
            &mut preview,
            &mut pr,
            windows::Win32::Graphics::Gdi::DT_LEFT | windows::Win32::Graphics::Gdi::DT_WORDBREAK,
        );
        // 指令行
        for (i, (label, _)) in COMMANDS.iter().enumerate() {
            let top = px(p.k, 88.0 + 30.0 * i as f32);
            let row = RECT {
                left: px(p.k, 8.0),
                top,
                right: rc.right - px(p.k, 8.0),
                bottom: top + px(p.k, 26.0),
            };
            let selected = i == p.sel;
            if selected {
                // 选中行画成圆角条，别在圆角窗里露一个直角块
                let brush = CreateSolidBrush(COLORREF(0x00553A2B));
                let pen = CreatePen(PEN_STYLE(0), 1, COLORREF(0x00553A2B));
                let old_brush = SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(brush.0));
                let old_pen = SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(pen.0));
                let _ = RoundRect(hdc, row.left, row.top, row.right, row.bottom, px(p.k, 6.0), px(p.k, 6.0));
                let _ = SelectObject(hdc, old_brush);
                let _ = SelectObject(hdc, old_pen);
                let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(pen.0));
                let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(brush.0));
                let _ = SetTextColor(hdc, COLORREF(0x00FFFFFF));
            } else {
                let _ = FillRect(hdc, &row, p.brush_card);
                let _ = SetTextColor(hdc, COLORREF(C_TEXT));
            }
            let mut t = wide(&format!("{}. {}", i + 1, label));
            let mut trect = RECT {
                left: row.left + px(p.k, 12.0),
                top,
                right: row.right,
                bottom: row.bottom,
            };
            let _ = DrawTextW(hdc, &mut t, &mut trect, windows::Win32::Graphics::Gdi::DT_LEFT | windows::Win32::Graphics::Gdi::DT_VCENTER | windows::Win32::Graphics::Gdi::DT_SINGLELINE);
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

/// 自测用：不抓选区，直接拿一段示例文字开面板（截图/布局核对）
pub fn open_demo(sample: &str) {
    unsafe {
        PREV_FG.store(
            windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow().0 as isize,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
    show_palette(sample.to_string());
}

pub fn open() {
    // 热键触发时前台就是用户正在编辑的窗口：先记下来，别等面板抢走前台再取
    unsafe {
        PREV_FG.store(
            windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow().0 as isize,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
    let (text, _orig) = match grab_selection() {
        Ok(v) => v,
        Err(e) => {
            report(&format!("SELECTION {e}"));
            crate::message_box(&format!("选区改写：{e}"));
            return;
        }
    };
    report(&format!("SELECTION 选中 {} 字", text.chars().count()));
    show_palette(text);
}

fn show_palette(text: String) {
    if PALETTE.lock().map(|p| p.is_some()).unwrap_or(false) {
        hide();
    }
    unsafe {
        let k = dpi_k();
        let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        let cls = wide("EryuSelectionPalette");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
            lpfnWndProc: Some(palette_proc),
            hInstance: hinst.into(),
            lpszClassName: PCWSTR(cls.as_ptr()),
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        let w = px(k, 380.0);
        let h = px(k, 268.0);
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let x = (pt.x - w / 2).clamp(8, GetSystemMetrics(SM_CXSCREEN) - w - 8);
        let y = (pt.y + px(k, 12.0)).clamp(8, GetSystemMetrics(SM_CYSCREEN) - h - 8);
        crate::report(&format!("SELWINDOW x={x} y={y} w={w} h={h}"));
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            PCWSTR(cls.as_ptr()),
            PCWSTR(wide("选区改写").as_ptr()),
            WS_POPUP,
            x,
            y,
            w,
            h,
            None,
            None,
            hinst,
            None,
        )
        .unwrap_or_default();
        if hwnd.0.is_null() {
            report("SELECTION 面板建不出来");
            return;
        }
        let p = Palette {
            hwnd,
            font: font(px(k, 14.0), 400),
            font_title: font(px(k, 12.0), 400),
            brush_bg: CreateSolidBrush(COLORREF(C_BG)),
            brush_card: CreateSolidBrush(COLORREF(C_CARD)),
            sel: 0,
            text,
            k,
        };
        *crate::lock(&PALETTE) = Some(p);
        // 圆角：和设置页/引导页一套观感（弹层不加自绘标题栏，保持轻量）
        let rgn = windows::Win32::Graphics::Gdi::CreateRoundRectRgn(
            0, 0, w + 1, h + 1, px(k, 12.0), px(k, 12.0),
        );
        let _ = windows::Win32::Graphics::Gdi::SetWindowRgn(hwnd, rgn, true);
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, Default::default());
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// 收起面板：只发 WM_CLOSE，真正的 DestroyWindow 回到自己的窗口过程里做，
/// 否则在 wndproc 里（比如按键处理中）销毁自己，后面的消息就悬在已死的窗口上。
pub fn hide() {
    let hwnd = PALETTE.lock().ok().and_then(|g| g.as_ref().map(|p| p.hwnd));
    if let Some(h) = hwnd {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

fn destroy_palette() {
    if let Ok(mut g) = PALETTE.lock() {
        if let Some(p) = g.take() {
            unsafe {
                let _ = ShowWindow(p.hwnd, SW_HIDE);
                let _ = DestroyWindow(p.hwnd);
                let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(p.font.0));
                let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(p.font_title.0));
                let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(p.brush_bg.0));
                let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(p.brush_card.0));
            }
        }
    }
}

/// 执行第 i 条指令：后台调 core，回来后把焦点还给原窗口再粘回
pub fn run(i: usize) {
    let Some((label, instruction)) = COMMANDS.get(i).copied() else { return };
    let text = match PALETTE.lock().ok().and_then(|p| p.as_ref().map(|p| p.text.clone())) {
        Some(t) => t,
        None => return,
    };
    // 面板自己当前是前台：先把「用户原来在哪个窗口」记住，
    // 再把用户原本的剪贴板内容留一份，粘完还回去（不改动用户的剪贴板）。
    let recorded = PREV_FG.load(std::sync::atomic::Ordering::Relaxed);
    let prev_fg = if recorded != 0 {
        recorded
    } else {
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0 as isize
    };
    let prev_clip = clipboard_text();
    hide();
    report(&format!("SELECTION 执行「{label}」{} 字", text.chars().count()));
    std::thread::spawn(move || {
        let out = {
            let app = crate::lock(&crate::APP);
            match app.as_ref() {
                Some(a) => a.core.run_command(instruction, &text),
                None => Err("还没启动好".to_string()),
            }
        };
        match out {
            Ok(new_text) => {
                // 焦点还回去，否则粘贴会落在别的窗口（面板已销毁，前台可能已经变了）
                if prev_fg != 0 {
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(
                            HWND(prev_fg as *mut std::ffi::c_void),
                        );
                    }
                    std::thread::sleep(std::time::Duration::from_millis(60));
                }
                if put_clipboard(&new_text) {
                    std::thread::sleep(std::time::Duration::from_millis(80));
                    send_paste();
                    report(&format!("SELECTION 已粘回 {} 字", new_text.chars().count()));
                    crate::tray::set_tip("字在 · 改写完成");
                    // 粘贴读完之后再把剪贴板还原成用户原来的内容
                    if let Some(old) = prev_clip {
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(900));
                            let _ = put_clipboard(&old);
                        });
                    }
                } else {
                    report("SELECTION 剪贴板写不进去");
                    crate::tray::set_tip("字在 · 剪贴板被占用，改写结果没粘上");
                }
            }
            Err(e) => {
                report(&format!("SELECTION 失败：{e}"));
                crate::tray::set_tip(&format!("字在 · 改写失败：{e}"));
            }
        }
    });
}
