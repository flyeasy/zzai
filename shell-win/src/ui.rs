//! 统一的界面度量：DPI 缩放 + 字体 + 排版刻度。
//!
//! 之前设置页用 15/19px、迷你窗写死 12px、胶囊又是 13px，同一个产品里三套字号，
//! 看起来「又巨大又巨小」。这里定一套刻度，三个窗口都用它：
//!   - 正文 13.5（Windows 11 的正文常用 14，13.5 在 125% 下更稳）
//!   - 次要说明 11.5
//!   - 区块标题 15 / 页面标题 17
//! 字号单位是 96 DPI 下的逻辑像素，实际按窗口 DPI 缩放。

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{CreateFontW, HFONT};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::Foundation::HWND;

/// 正文字号（96 DPI 逻辑像素）。14 ≈ Windows 默认 10.5pt，比之前的 13 更接近系统字号。
pub const BODY: f32 = 14.0;
pub const CAPTION: f32 = 12.0;
pub const SECTION: f32 = 15.0;
pub const TITLE: f32 = 20.0;

/// 96 DPI = 1.0；高分屏按比例放大，避免「字小得看不清」。
pub fn scale(h: HWND) -> f32 {
    let mut dpi = unsafe { GetDpiForWindow(h) };
    if dpi == 0 {
        dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() };
    }
    if dpi == 0 {
        1.0
    } else {
        dpi as f32 / 96.0
    }
}

/// 按工作区把 DPI 缩放收一收：900×632 的窗在 125%/150% 下会高过 768p/1080p 的工作区，
/// 超出去的部分点不到。这里按「逻辑尺寸必须完整放得下」反推一个上限。
pub fn fit_scale(k: f32, logical_w: f32, logical_h: f32) -> f32 {
    let wa = crate::overlay::work_area_at(None);
    let avail_w = (wa.right - wa.left) as f32;
    let avail_h = (wa.bottom - wa.top) as f32;
    let by_w = (avail_w - 24.0) / logical_w;
    let by_h = (avail_h - 24.0) / logical_h;
    k.min(by_w).min(by_h).max(1.0)
}

pub fn client_to_screen(h: HWND, pt: &mut windows::Win32::Foundation::POINT) -> bool {
    unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(h, pt).as_bool() }
}

pub fn px(k: f32, v: f32) -> i32 {
    (v * k).round() as i32
}

/// 统一字体：中文用微软雅黑 UI（英文数字字形也正常），不再用 Segoe UI 兜中文。
/// `weight` 用 400 = 常规，500 = 中等（按钮），600 = 半粗（标题）。
pub fn font(k: f32, size: f32, weight: i32) -> HFONT {
    let face: Vec<u16> = "Microsoft YaHei UI".encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        CreateFontW(
            -(size * k).round() as i32,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            1,
            0,
            0,
            5, // CLEARTYPE_QUALITY
            0,
            PCWSTR(face.as_ptr()),
        )
    }
}
