//! 首次启动的六步引导：模型 / 麦克风 / 按键 / 上屏方式 / 试说一句 / 润色。
//!
//! 说明文字直接用 GDI 画（和设置窗口同一套深色视觉），只有输入框、下拉框、按钮是真控件。
//! 「稍后再说」也会落一个 `onboarding.done` 标记，不再反复弹。

use std::sync::Mutex;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    GetStockObject, NULL_BRUSH, PS_SOLID,
    InvalidateRect, RoundRect, SelectObject, SetBkColor, SetBkMode, SetTextColor, TextOutW,
    CLIP_DEFAULT_PRECIS,
    CLEARTYPE_QUALITY, DEFAULT_PITCH, DT_CENTER, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, HFONT,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, GetKeyState, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::core::Core;

const C_BG: u32 = 0x001F1C1B;
const C_CARD: u32 = 0x002C2624;
const C_EDIT: u32 = 0x0038302E;
const C_ACCENT: u32 = 0x00F6823B;
const C_TEXT: u32 = 0x00ECE9E9;
const C_MUTED: u32 = 0x00A6A09A;
const C_LINE: u32 = 0x00453D3A;

const ID_NEXT: usize = 200;
const ID_BACK: usize = 201;
const ID_LATER: usize = 202;
const ID_PULL: usize = 203;
const ID_USE: usize = 204;
const ID_INSERT: usize = 205;
const ID_MODE_PASTE: usize = 207;
const ID_MODE_TYPE: usize = 208;

const ID_POLISH: usize = 206;
const ID_HOTKEY: usize = 207;
/// 自绘顶栏高度（与设置页一致：无边框窗 + 自己画标题栏）
const HEADER: f32 = 34.0;

/// 引导步序（改动时只改这里；`--onboarding-step N` 也用这套编号）
const STEP_MODEL: usize = 0;
const STEP_MIC: usize = 1;
const STEP_HOTKEY: usize = 2;
const STEP_INSERT: usize = 3;
const STEP_PROBE: usize = 4;
const STEP_POLISH: usize = 5;
const STEP_LAST: usize = STEP_POLISH;

struct Wizard {
    core: &'static Core,
    hwnd: HWND,
    step: usize,
    k: f64,
    f_title: HFONT,
    f_body: HFONT,
    f_small: HFONT,
    f_big: HFONT,
    brush_edit: windows::Win32::Graphics::Gdi::HBRUSH,
    ctrls: Vec<HWND>,
    /// 第 4 步（试说一句）的识别结果与判定
    probe_text: String,
    probe_hint: String,
    probe_passed: bool,
    /// 模型下载进度（0..=100）；None = 没有下载在跑
    model_pct: Option<u32>,
    /// 正在下载的模型类型（「语音模型」「降噪模型」…），跟进度一起显示，避免只说一个百分数
    model_pull_label: String,
    /// 麦克风自检页的实时电平（0..1），来自 core 的 meter 事件
    mic_rms: f32,
    /// 语音识别模型是否已经装好：没装好时不允许进「试说一句」，也不让它挡住下一步
    asr_ready: bool,
}

static WIN: Mutex<Option<Wizard>> = Mutex::new(None);

/// 第 4 步的朗读例句（与 macOS 一致）
pub const PROBE_EXAMPLE: &str = "今天天气不错，我们开始吧";

/// 只留字母数字/汉字：标点、空格不参与比对
fn probe_chars(s: &str) -> Vec<char> {
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// 字符多重集合交集（「多说了几个字」不扣分，「少说」按例句长度折算）
fn probe_overlap(a: &[char], b: &[char]) -> usize {
    let mut pool: Vec<char> = b.to_vec();
    let mut hit = 0;
    for c in a {
        if let Some(pos) = pool.iter().position(|x| x == c) {
            pool.remove(pos);
            hit += 1;
        }
    }
    hit
}

/// 与例句的字符重合率（分母是例句字数）
pub fn probe_ratio(text: &str) -> f64 {
    let example = probe_chars(PROBE_EXAMPLE);
    if example.is_empty() {
        return 0.0;
    }
    probe_overlap(&probe_chars(text), &example) as f64 / example.len() as f64
}

/// 判定：至少 4 个字，且重合率 ≥ 0.5（读半句也能过）
pub fn probe_passes(text: &str) -> bool {
    probe_chars(text).len() >= 4 && probe_ratio(text) >= 0.5
}

/// 引导是否正停在试说那一步（主循环据此吞掉上屏，见 D-110）
/// 麦克风自检页的实时电平（main.rs 的 meter 分支转发过来）
pub fn on_meter(rms: f32) {
    let mut g = match WIN.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let Some(w) = g.as_mut() else { return };
    if w.step != STEP_MIC {
        return;
    }
    w.mic_rms = rms;
    let hwnd = w.hwnd;
    drop(g);
    unsafe {
        let _ = InvalidateRect(hwnd, None, false);
    }
}

pub fn probe_active() -> bool {
    WIN.lock()
        .ok()
        .and_then(|g| g.as_ref().map(|w| w.step == STEP_PROBE))
        .unwrap_or(false)
}

fn probe_repaint(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(hwnd, None, true);
        // 自绘按钮是子窗口：父窗口 InvalidateRect 不会让它们重画，
        // 「下一步」会一直保持旧的灰样式（看着像不能点）。单独刷它。
        let next = GetDlgItem(hwnd, ID_NEXT as i32).unwrap_or_default();
        if !next.0.is_null() {
            let _ = InvalidateRect(next, None, true);
        }
    }
}

/// 识别结果回来：记下来、判一次，并在标题栏下面给一句话
pub fn offer_result(text: &str) -> bool {
    let mut g = match WIN.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let Some(w) = g.as_mut() else { return false };
    if w.step != STEP_PROBE {
        return false;
    }
    w.probe_text = text.trim().to_string();
    w.probe_passed = probe_passes(&w.probe_text);
    w.probe_hint = if w.probe_passed {
        "读到了，点「下一步」继续。".to_string()
    } else if w.probe_text.len() < 4 {
        "只听到一点点，按住多说几个字。".to_string()
    } else {
        "这句和例句差得有点多，再说一遍试试。".to_string()
    };
    let hwnd = w.hwnd;
    drop(g);
    probe_repaint(hwnd);
    true
}

/// 边说边显（partial_text）
pub fn note_partial(text: &str) {
    let mut g = match WIN.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let Some(w) = g.as_mut() else { return };
    if w.step != STEP_PROBE {
        return;
    }
    w.probe_text = text.to_string();
    let hwnd = w.hwnd;
    drop(g);
    probe_repaint(hwnd);
}

/// 阶段变化（state 事件）
pub fn note_state(phase: &str) {
    let mut g = match WIN.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let Some(w) = g.as_mut() else { return };
    if w.step != STEP_PROBE {
        return;
    }
    w.probe_hint = match phase {
        "starting" | "listening" => "正在听…松开后出字".to_string(),
        "processing" | "decoding" => "听到了，正在识别…".to_string(),
        _ => w.probe_hint.clone(),
    };
    let hwnd = w.hwnd;
    drop(g);
    probe_repaint(hwnd);
}


unsafe impl Send for Wizard {}

/// WM_CTLCOLOR* 里绝不能再锁 WIN：控件创建/重绘时同步回调父窗口，
/// 而调用方（build）正持着 WIN 锁 → 死锁。这里用一次性创建的缓存画刷。
fn cached_brush(kind: u8) -> windows::Win32::Graphics::Gdi::HBRUSH {
    use std::sync::OnceLock;
    static EDIT: OnceLock<isize> = OnceLock::new();
    static CARD: OnceLock<isize> = OnceLock::new();
    let cell = if kind == 0 { &EDIT } else { &CARD };
    let v = *cell.get_or_init(|| unsafe {
        let c = if kind == 0 { C_EDIT } else { C_CARD };
        CreateSolidBrush(COLORREF(c)).0 as isize
    });
    windows::Win32::Graphics::Gdi::HBRUSH(v as *mut core::ffi::c_void)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn px<K: Into<f64>, V: Into<f64>>(k: K, v: V) -> i32 {
    (v.into() * k.into()).round() as i32
}

fn font(size: i32, weight: i32) -> HFONT {
    // 与设置页/迷你窗/胶囊同一套字面（用户反馈「字体各处不一样」）
    let face = wide("Microsoft YaHei UI");
    unsafe {
        CreateFontW(
            size, 0, 0, 0, weight, 0, 0, 0, 1u32,
            OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            PCWSTR(face.as_ptr()),
        )
    }
}

/// 打开引导时算出的有效缩放（已按工作区收过），绘制也用同一个，避免「窗口按 A 缩、内容按 B 缩」。
static EFFECTIVE_K: Mutex<f64> = Mutex::new(0.0);
/// 语音模型是否就绪（build() 里读，避免在持锁路径上再取 WIN 锁）
static ASR_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn scale() -> f64 {
    if let Ok(k) = EFFECTIVE_K.lock() {
        if *k > 0.0 {
            return *k;
        }
    }
    unsafe {
        let hdc = windows::Win32::Graphics::Gdi::GetDC(None);
        let d = windows::Win32::Graphics::Gdi::GetDeviceCaps(
            hdc,
            windows::Win32::Graphics::Gdi::LOGPIXELSX,
        ) as f64;
        let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, hdc);
        if d > 0.0 { d / 96.0 } else { 1.0 }
    }
}

fn text(hdc: windows::Win32::Graphics::Gdi::HDC, s: &str, x: i32, y: i32, color: u32, f: HFONT) {
    if s.is_empty() {
        return;
    }
    let old = unsafe { SelectObject(hdc, f) };
    let w = wide(s);
    unsafe {
        SetBkMode(hdc, windows::Win32::Graphics::Gdi::TRANSPARENT);
        SetTextColor(hdc, COLORREF(color));
        let _ = TextOutW(hdc, x, y, &w[..w.len() - 1]);
        SelectObject(hdc, old);
    }
}

fn round_fill(hdc: windows::Win32::Graphics::Gdi::HDC, r: RECT, radius: i32, color: u32) {
    unsafe {
        let b = CreateSolidBrush(COLORREF(color));
        let old = SelectObject(hdc, b);
        let _ = RoundRect(hdc, r.left, r.top, r.right, r.bottom, radius, radius);
        SelectObject(hdc, old);
        let _ = DeleteObject(b);
    }
}

fn cfg_json(core: &Core) -> serde_json::Value {
    serde_json::from_str(&core.config_json()).unwrap_or(serde_json::Value::Null)
}

fn set_cfg(core: &Core, json: serde_json::Value) {
    let _ = core.set_config(&json.to_string());
}

/// 引导窗现在开着吗（开着的时候不该再弹浮层/迷你窗去分散注意）
pub fn is_open() -> bool {
    WIN.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// 引导窗口句柄（未打开时 None）：消息泵与自测用
pub fn hwnd() -> Option<HWND> {
    WIN.lock()
        .ok()
        .and_then(|g| g.as_ref().map(|w| w.hwnd))
}


/// 语音识别模型装好没有（stub 不算）。没装好时「试说一句」必然失败，
/// 让用户读一句没有意义（用户反馈 2026-11-21）。
fn asr_ready(core: &Core) -> bool {
    serde_json::from_str::<serde_json::Value>(&core.models_json())
        .ok()
        .and_then(|v| {
            v.as_array().map(|l| {
                l.iter().any(|m| {
                    m["id"].as_str() != Some("stub")
                        && m["engine"].as_bool().unwrap_or(false)
                        && m["installed"].as_bool().unwrap_or(false)
                })
            })
        })
        .unwrap_or(false)
}

/// 引导是否已走完（用本地标记文件，避免依赖 core 版本）
pub fn done_marker() -> std::path::PathBuf {
    std::path::Path::new(&crate::config_dir()).join("onboarding.done")
}

/// 首次启动时由 main 调用（UI 线程）。
pub fn open_if_needed(core: &'static Core) {
    let j = cfg_json(core);
    let done = j.get("onboarding_done").and_then(|v| v.as_bool()).unwrap_or(false);
    let probe_done = j.get("onboarding_probe_done").and_then(|v| v.as_bool()).unwrap_or(false);
    if (done && probe_done) || (done_marker().exists() && probe_done) {
        return;
    }
    // 模型没装好就永远先回模型页：否则「试说一句」注定没字可出（用户反馈 2026-11-21）。
    // 模型在、但上次没试说成功：回试说那一步。
    let start_step = if !asr_ready(core) {
        STEP_MODEL
    } else if done || done_marker().exists() {
        STEP_PROBE
    } else {
        STEP_MODEL
    };
    open_at(core, start_step);
}

pub(crate) fn open_at(core: &'static Core, start_step: usize) {
    open_with_step(core, start_step);
}

pub fn open(core: &'static Core) {
    let start = std::env::args()
        .position(|a| a == "--onboarding-step")
        .and_then(|i| std::env::args().nth(i + 1))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    open_with_step(core, start);
}

fn open_with_step(core: &'static Core, start_step: usize) {
    if crate::lock(&WIN).is_some() {
        return;
    }
    // 高 DPI 小屏：640×536 逻辑尺寸在 150% 下会高过 768p 工作区，按工作区收一个上限
    let mut k = scale();
    if let Ok(v) = std::env::var("ERYU_SETTINGS_DPI") {
        if let Ok(p) = v.parse::<u32>() {
            k = p as f64 / 100.0;
        }
    }
    let k = crate::ui::fit_scale(k.max(1.0) as f32, 640.0, 500.0 + HEADER) as f64;
    if let Ok(mut g) = EFFECTIVE_K.lock() {
        *g = k;
    }
    crate::report(&format!("ONBOARDING k={k:.2}"));
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("EryuOnboarding");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
            lpfnWndProc: Some(wndproc),
            hInstance: HINSTANCE(hinst.0),
            hIcon: crate::icon::app_icon(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: CreateSolidBrush(COLORREF(C_BG)),
            lpszClassName: class,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        // 居中（CW_USEDEFAULT 会把引导窗扔到左上角，很丑）
        let (wx, wy) = {
            let w = px(k, 640.0);
            let h = px(k, 500.0 + HEADER);
            let wa = crate::overlay::work_area_at(None);
            (
                wa.left + ((wa.right - wa.left) - w) / 2,
                wa.top + ((wa.bottom - wa.top) - h) / 2,
            )
        };
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("欢迎使用 Eryu"),
            WS_POPUP | WS_CLIPCHILDREN,
            wx, wy,
            px(k, 640.0), px(k, 500.0 + HEADER),
            None, HMENU(std::ptr::null_mut()), HINSTANCE(hinst.0), None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };
        // 圆角：和设置页同一套观感
        let rgn = windows::Win32::Graphics::Gdi::CreateRoundRectRgn(
            0, 0,
            px(k, 640.0) + 1,
            px(k, 500.0 + HEADER) + 1,
            px(k, 16.0), px(k, 16.0),
        );
        let _ = windows::Win32::Graphics::Gdi::SetWindowRgn(hwnd, rgn, true);
        {
            let on: i32 = 1;
            let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                hwnd,
                windows::Win32::Graphics::Dwm::DWMWA_USE_IMMERSIVE_DARK_MODE,
                &on as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            );
        }
        let wiz = Wizard {
            core,
            hwnd,
            step: start_step.min(STEP_LAST),
            asr_ready: asr_ready(core),
            k,
            f_title: font(px(k, 20.0), 600),
            f_body: font(px(k, 14.0), 400),
            f_small: font(px(k, 12.0), 400),
            f_big: font(px(k, 15.0), 600),
            brush_edit: CreateSolidBrush(COLORREF(C_EDIT)),
            ctrls: Vec::new(),
            probe_text: String::new(),
            probe_hint: String::new(),
            probe_passed: false,
            model_pct: None,
            model_pull_label: String::new(),
            mic_rms: 0.0,
        };
        ASR_READY.store(wiz.asr_ready, std::sync::atomic::Ordering::Relaxed);
        *crate::lock(&WIN) = Some(wiz);
        if start_step.min(STEP_LAST) == STEP_MIC {
            let _ = core.meter_start();
        }
        build(hwnd);
        // 引导在的时候把迷你窗和浮层收起来：几份窗口同时在屏幕上很乱（mac 也是引导结束才落主窗）
        crate::mini::hide();
        crate::overlay_hide();
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

fn child(class: &str, text: &str, style: u32, hwnd: HWND, k: f64, x: f32, y: f32, cw: f32, ch: f32, id: usize, f: HFONT) -> HWND {
    let cls = wide(class);
    let txt = wide(text);
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let h = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(cls.as_ptr()),
            PCWSTR(txt.as_ptr()),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style | WS_TABSTOP.0),
            px(k, x), px(k, y + HEADER), px(k, cw), px(k, ch),
            hwnd,
            HMENU(if id == 0 { std::ptr::null_mut() } else { id as *mut std::ffi::c_void }),
            HINSTANCE(hinst.0),
            None,
        )
        .unwrap_or_default();
        let _ = SendMessageW(h, WM_SETFONT, WPARAM(f.0 as usize), LPARAM(1));
        // 关掉 EDIT/COMBO 的视觉样式，否则 WM_CTLCOLOREDIT 不生效、输入框仍是浅色
        if class.eq_ignore_ascii_case("EDIT") || class.eq_ignore_ascii_case("COMBOBOX") {
            let blank = wide(" ");
            let _ = windows::Win32::UI::Controls::SetWindowTheme(h, PCWSTR(blank.as_ptr()), PCWSTR(blank.as_ptr()));
        }
        h
    }
}

fn build(hwnd: HWND) {
    let mut guard = match WIN.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let Some(w) = guard.as_mut() else { return };
    unsafe {
        for h in w.ctrls.drain(..) {
            let _ = DestroyWindow(h);
        }
    }
    let k = w.k;
    let f = w.f_body;
    let mut made: Vec<HWND> = Vec::new();
    match w.step {
        STEP_MIC => {
            // 麦克风自检页没有控件：电平条用 GDI 画（见 paint）
        }
        STEP_HOTKEY => {
            let cur = cfg_json(w.core)
                .get("hotkey")
                .and_then(|v| v.as_str())
                .unwrap_or("ALT_R")
                .to_string();
            made.push(crate::keycap::create(
                hwnd,
                px(k, 24.0),
                px(k, 214.0),
                px(k, 260.0),
                px(k, 30.0),
                ID_HOTKEY as i32,
                &cur,
                k as f32,
            ));
        }
        STEP_INSERT => {
            // 分段按钮（原生 COMBOBOX 在深色界面里永远是白底，跟设置页一致改自绘）
            made.push(child("BUTTON", "粘贴", BS_OWNERDRAW as u32, hwnd, k, 24.0, 214.0, 110.0, 30.0, ID_MODE_PASTE, f));
            made.push(child("BUTTON", "逐字打字", BS_OWNERDRAW as u32, hwnd, k, 142.0, 214.0, 120.0, 30.0, ID_MODE_TYPE, f));
        }
        STEP_PROBE => {
            // 试说这一步没有控件：用热键按住说话（鼠标按住说话是 R-110 的后续）
        }
        STEP_POLISH => {
            let on = cfg_json(w.core).get("polish_enabled").and_then(|v| v.as_bool()).unwrap_or(false);
            made.push(child(
                "BUTTON",
                if on { "已开启，点此关闭" } else { "开启润色" },
                BS_OWNERDRAW as u32,
                hwnd,
                k,
                24.0,
                216.0,
                150.0,
                28.0,
                ID_POLISH,
                f,
            ));
        }
        _ => {
            let ready = ASR_READY.load(std::sync::atomic::Ordering::Relaxed);
            if ready {
                // 已装好（安装包自带 SenseVoice）：不再摆「下载/用它」两个按钮，省得以为还要装
            } else {
                made.push(child("BUTTON", "下载 SenseVoice", BS_OWNERDRAW as u32, hwnd, k, 24.0, 300.0, 150.0, 30.0, ID_PULL, f));
                made.push(child("BUTTON", "用它", BS_OWNERDRAW as u32, hwnd, k, 186.0, 300.0, 90.0, 30.0, ID_USE, f));
            }
        }
    }
    made.push(child("BUTTON", "稍后再说", BS_OWNERDRAW as u32, hwnd, k, 24.0, 430.0, 96.0, 30.0, ID_LATER, f));
    if w.step > 0 {
        made.push(child("BUTTON", "上一步", BS_OWNERDRAW as u32, hwnd, k, 420.0, 430.0, 88.0, 30.0, ID_BACK, f));
    }
    let next_text = if w.step == STEP_LAST { "开始使用" } else { "下一步" };
    made.push(child("BUTTON", next_text, BS_OWNERDRAW as u32, hwnd, k, 518.0, 430.0, 98.0, 30.0, ID_NEXT, f));
    w.ctrls.extend(made);
    unsafe {
        let _ = InvalidateRect(hwnd, None, true);
    }
}

fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    unsafe {
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let bg = CreateSolidBrush(COLORREF(C_BG));
        let _ = FillRect(hdc, &rc, bg);
        let _ = DeleteObject(bg);

        // 自绘顶栏：画完再把绘图原点下移，内容坐标就不用整体改
        let k0 = scale();
        let bar = RECT { left: 0, top: 0, right: rc.right, bottom: px(k0, HEADER) };
        let bbg = CreateSolidBrush(COLORREF(C_CARD));
        let _ = FillRect(hdc, &bar, bbg);
        let _ = DeleteObject(bbg);
        let line = CreateSolidBrush(COLORREF(C_LINE));
        let lr = RECT { left: 0, top: px(k0, HEADER) - 1, right: rc.right, bottom: px(k0, HEADER) };
        let _ = FillRect(hdc, &lr, line);
        let _ = DeleteObject(line);
        text(hdc, "字在", px(k0, 16.0), px(k0, 10.0), C_TEXT, font(px(k0, 13.0), 600));
        text(hdc, "✕", px(k0, 610.0), px(k0, 9.0), C_MUTED, font(px(k0, 12.0), 400));
        let _ = windows::Win32::Graphics::Gdi::SetViewportOrgEx(hdc, 0, px(k0, HEADER), None);
        rc.bottom -= px(k0, HEADER);

        // 重绘期间不能阻塞等锁：build() 持锁创建控件时也会触发 WM_PAINT，
        // 这里再 lock 同一把不可重入的 Mutex 就是死锁（表现为「未响应」）。
        let (step, k, ft, fb, fbig, fsm) = {
            let g = match WIN.try_lock() {
                Ok(g) => g,
                Err(_) => {
                    let _ = EndPaint(hwnd, &mut ps);
                    return;
                }
            };
            match g.as_ref() {
                Some(w) => (w.step, w.k, w.f_title, w.f_body, w.f_big, w.f_small),
                None => {
                    let _ = EndPaint(hwnd, &mut ps);
                    return;
                }
            }
        };
        let (title, sub) = match step {
            STEP_MIC => ("先确认麦克风", "对着麦克风说句话，下面的条会跟着动。"),
            STEP_HOTKEY => ("设定按住说话的键", "任何单键都能按住说话，也可以选组合键。"),
            STEP_INSERT => ("选上屏方式", "粘贴最快；逐字打字兼容性最好。"),
            STEP_PROBE => ("试说一句", "按住刚才设定的键，把左边这句话读一遍。"),
            STEP_POLISH => ("要不要润色", "润色可选，默认关。会把文字发到你配置的服务。"),
            _ => ("欢迎使用 Eryu", "按住一个键说话，松手上屏。全部在本机识别，不上传音频。"),
        };
        text(hdc, title, px(k, 22.0), px(k, 22.0), C_TEXT, ft);
        text(hdc, sub, px(k, 24.0), px(k, 54.0), C_MUTED, fsm);

        // 步骤点
        for i in 0..=STEP_LAST {
            let x = px(k, 24.0 + i as f32 * 16.0);
            let color = if i <= step { C_ACCENT } else { C_LINE };
            round_fill(hdc, RECT { left: x, top: px(k, 84.0), right: x + px(k, 8.0), bottom: px(k, 92.0) }, px(k, 4.0), color);
        }

        // 内容卡片
        round_fill(
            hdc,
            RECT { left: px(k, 16.0), top: px(k, 112.0), right: cx(k, rc.right, 16.0), bottom: px(k, 400.0) },
            px(k, 12.0),
            C_CARD,
        );

        match step {
            STEP_MIC => {
                let (rms, device) = match WIN.try_lock() {
                    Ok(g) => match g.as_ref() {
                        Some(w) => (
                            w.mic_rms,
                            cfg_json(w.core)
                                .get("device")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        ),
                        None => (0.0, String::new()),
                    },
                    Err(_) => (0.0, String::new()),
                };
                text(hdc, "输入电平", px(k, 24.0), px(k, 132.0), C_TEXT, fbig);
                // 电平条：底槽 + 渐变（绿→黄→红）
                let bar = RECT {
                    left: px(k, 24.0),
                    top: px(k, 160.0),
                    right: cx(k, rc.right, 24.0),
                    bottom: px(k, 176.0),
                };
                round_fill(hdc, bar, px(k, 8.0), C_EDIT);
                let filled = ((rms * 1.6).clamp(0.0, 1.0) * (bar.right - bar.left) as f32) as i32;
                if filled > 2 {
                    let color = if rms < 0.003 {
                        C_MUTED
                    } else if rms < 0.03 {
                        0x00_3B_B6_3B
                    } else if rms < 0.15 {
                        0x00_6E_C5_F2
                    } else {
                        0x00_5B_5B_EF
                    };
                    round_fill(hdc, RECT { right: bar.left + filled, ..bar }, px(k, 8.0), color);
                }
                let db = if rms <= 1e-5 { -60.0 } else { 20.0 * rms.log10() };
                text(hdc, &format!("约 {:.0} dB", db), px(k, 24.0), px(k, 184.0), C_MUTED, fsm);
                let shown = if device.is_empty() { "系统默认麦克风".to_string() } else { device.clone() };
                text(hdc, &format!("当前输入：{shown}"), px(k, 24.0), px(k, 206.0), C_TEXT, fb);
                // 系统权限被关掉时，条永远不动：先说清楚，别让人怀疑硬件
                if crate::settings::mic_privacy_state() == "已拒绝" {
                    text(hdc, "系统麦克风权限被关了：设置 → 隐私和安全性 → 麦克风，允许桌面应用访问。", px(k, 24.0), px(k, 238.0), C_ACCENT, fsm);
                } else {
                    text(hdc, "条在动就说明能用。没反应：", px(k, 24.0), px(k, 238.0), C_MUTED, fsm);
                    text(hdc, "Windows 设置 → 隐私和安全性 → 麦克风，允许桌面应用访问。", px(k, 24.0), px(k, 256.0), C_MUTED, fsm);
                }
            }
            STEP_HOTKEY => {
                text(hdc, &format!("当前：{}", crate::hotkey_hint()), px(k, 24.0), px(k, 132.0), C_TEXT, fbig);
                text(hdc, "也可以写 F13 / ALT_R / CTRL+ALT+SPACE，改完立即生效。", px(k, 24.0), px(k, 164.0), C_MUTED, fsm);
                text(hdc, "按键（写名字即可）", px(k, 24.0), px(k, 194.0), C_MUTED, fsm);
            }
            STEP_INSERT => {
                text(hdc, "识别完怎么上屏", px(k, 24.0), px(k, 132.0), C_TEXT, fbig);
                text(hdc, "粘贴：写剪贴板 + 合成 Ctrl+V，最快最稳，原剪贴板会恢复。", px(k, 24.0), px(k, 164.0), C_MUTED, fsm);
                text(hdc, "逐字打字：一个字一个字打，慢一点，但在终端/安全输入框更可靠。", px(k, 24.0), px(k, 182.0), C_MUTED, fsm);
            }
            STEP_PROBE => {
                text(hdc, "请朗读这句话", px(k, 24.0), px(k, 128.0), C_MUTED, fsm);
                text(hdc, PROBE_EXAMPLE, px(k, 24.0), px(k, 146.0), C_TEXT, fbig);
                if let Ok(g) = WIN.try_lock() {
                    if let Some(w) = g.as_ref() {
                        text(hdc, &w.probe_hint, px(k, 24.0), px(k, 186.0), C_ACCENT, fsm);
                        round_fill(
                            hdc,
                            RECT { left: px(k, 24.0), top: px(k, 214.0), right: cx(k, rc.right, 24.0), bottom: px(k, 366.0) },
                            px(k, 8.0),
                            C_EDIT,
                        );
                        let shown = if w.probe_text.is_empty() {
                            "（按住说话，松手后文字会出现在这里）"
                        } else {
                            w.probe_text.as_str()
                        };
                        let color = if w.probe_text.is_empty() { C_MUTED } else { C_TEXT };
                        text(hdc, shown, px(k, 34.0), px(k, 226.0), color, fb);
                        let tip = if !engine_ready(w.core) {
                            "还没有可用的语音模型：回上一步点「下载 SenseVoice」（或换成已装的 Qwen3）。"
                        } else if w.probe_passed {
                            "成功：这句话会被记进历史，但不会打进别的程序。"
                        } else {
                            "没出字时「下一步」是灰的；也可以点「稍后再说」，下次启动会回到这一步。"
                        };
                        let tip_color = if !engine_ready(w.core) { C_ACCENT } else { C_MUTED };
                        text(hdc, tip, px(k, 24.0), px(k, 374.0), tip_color, fsm);
                    }
                }
            }
            STEP_POLISH => {
                text(hdc, "润色（可选）", px(k, 24.0), px(k, 132.0), C_TEXT, fbig);
                text(hdc, "开启后文字会先发到你配置的服务整理；不填 Key 就别开，不开也能正常听写。", px(k, 24.0), px(k, 164.0), C_MUTED, fsm);
            }
            _ => {
                text(hdc, "选一个识别模型", px(k, 24.0), px(k, 132.0), C_TEXT, fbig);
                let ready = WIN.try_lock().ok().and_then(|g| g.as_ref().map(|w| w.asr_ready)).unwrap_or(false);
                if ready {
                    text(hdc, "语音模型已就绪（随安装包装好），点「下一步」继续。", px(k, 24.0), px(k, 164.0), C_MUTED, fsm);
                } else {
                    text(hdc, "语音只在本机识别。默认 SenseVoice 体积小、速度快。", px(k, 24.0), px(k, 164.0), C_MUTED, fsm);
                }
                let models = match WIN.try_lock() {
                    Ok(g) => match g.as_ref() {
                        Some(w) => cfg_json_models(w.core),
                        None => Vec::new(),
                    },
                    Err(_) => Vec::new(),
                };
                let mut y = 196.0;
                for (name, size, state) in models {
                    let line = format!("{name}   {size}   {state}");
                    text(hdc, &line, px(k, 24.0), px(k, y), C_TEXT, fb);
                    y += 28.0;
                }
                // 下载中要让用户看见在动（否则点了「下载」像没反应）
                let pct = match WIN.try_lock() {
                    Ok(g) => g.as_ref().and_then(|w| w.model_pct),
                    Err(_) => None,
                };
                if let Some(p) = pct {
                    let label = match WIN.try_lock() {
                        Ok(g) => g
                            .as_ref()
                            .map(|w| w.model_pull_label.clone())
                            .unwrap_or_default(),
                        Err(_) => String::new(),
                    };
                    let msg = if p == 0 && label.is_empty() {
                        "正在准备下载…".to_string()
                    } else if p == 0 {
                        format!("正在准备下载：{label}…")
                    } else {
                        format!("正在下载{label}：{p}%（可以继续用别的功能，完成后会提示）")
                    };
                    text(hdc, &msg, px(k, 24.0), px(k, 348.0), C_ACCENT, fsm);
                }
            }
        }
        let _ = EndPaint(hwnd, &mut ps);
    }
}

fn cx(k: f64, right: i32, pad: f32) -> i32 {
    right - px(k, pad)
}

/// 当前选的引擎模型是否真的装了（没装时试说一定过不了，要先把原因说清楚）。
fn engine_ready(core: &'static Core) -> bool {
    let j: serde_json::Value = serde_json::from_str(&core.models_json()).unwrap_or_default();
    j.as_array()
        .map(|list| {
            list.iter().any(|m| {
                m.get("active").and_then(|v| v.as_bool()).unwrap_or(false)
                    && m.get("installed").and_then(|v| v.as_bool()).unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn cfg_json_models(core: &'static Core) -> Vec<(String, String, String)> {
    let j: serde_json::Value = serde_json::from_str(&core.models_json()).unwrap_or_default();
    let mut out = Vec::new();
    if let Some(list) = j.as_array() {
        for m in list {
            let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if id != "sense" && id != "qwen3" {
                continue;
            }
            let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("模型").to_string();
            let size = m.get("size_text").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let installed = m.get("installed").and_then(|v| v.as_bool()).unwrap_or(false);
            let active = m.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
            let state = if active && installed {
                "使用中"
            } else if active {
                // 选定但文件还没下：别写「使用中」骗人，实话实说还在下载/未安装
                "已选中（未安装）"
            } else if installed {
                "已安装"
            } else {
                "未安装（在「设置 → 引擎」里下载）"
            };
            out.push((name, size, state.to_string()));
        }
    }
    out
}

/// 下载进度：只在第 0 步（模型）显示，避免别的页面刷出无关数字。
pub fn on_model_progress(recv: u64, total: u64, id: &str, phase: &str) {
    let Ok(mut g) = WIN.lock() else { return };
    let Some(w) = g.as_mut() else { return };
    let pct = if total > 0 {
        (recv.saturating_mul(100) / total) as u32
    } else if phase == "starting" {
        0
    } else {
        0
    };
    w.model_pct = Some(pct);
    w.model_pull_label = crate::model_label(id).to_string();
    if w.step == STEP_MODEL {
        let hwnd = w.hwnd;
        let _ = unsafe { InvalidateRect(hwnd, None, false) };
    }
}

/// 模型下载完成后刷新「可试说」状态（main.rs 的 model_done 调用）。
pub fn refresh_asr() {
    let core = match WIN.lock() {
        Ok(g) => g.as_ref().map(|w| w.core),
        Err(_) => None,
    };
    let Some(core) = core else { return };
    let ready = asr_ready(core);
    let mut g = match WIN.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let Some(w) = g.as_mut() else { return };
    w.asr_ready = ready;
    ASR_READY.store(ready, std::sync::atomic::Ordering::Relaxed);
    w.model_pct = None;
    let hwnd = w.hwnd;
    drop(g);
    probe_repaint(hwnd);
}

fn finish(hwnd: HWND) {
    let (core, passed) = {
        match WIN.lock() {
            Ok(g) => match g.as_ref() {
                Some(w) => (Some(w.core), w.probe_passed),
                None => (None, false),
            },
            Err(_) => (None, false),
        }
    };
    if let Some(w) = core {
        let _ = std::fs::write(done_marker(), b"1");
        // 引导结束才把迷你主窗落下来（与 mac 一致）
        crate::mini::show();
        // 试说通过了才记 probe_done；没通过下次启动还会回到这一步（与 macOS 同）
        set_cfg(
            w,
            serde_json::json!({"onboarding_done": true, "onboarding_probe_done": passed}),
        );
    }
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

fn advance(hwnd: HWND, step: usize) {
    // 先把要提交的值取出来、把锁放掉，再去调 core —— core 的 FFI 可能会同步回调事件，
    // 在持锁状态下回调 → 再次进 WM_COMMAND → 撞同一把不可重入的 Mutex，表现就是「未响应」。
    let (core, cur_step, hotkey, mode) = {
        let mut g = crate::lock(&WIN);
        match g.as_mut() {
            None => return,
            Some(w) => {
                let hotkey = if w.step == STEP_HOTKEY {
                    w.ctrls.first().copied().map(crate::keycap::text).unwrap_or_default()
                } else {
                    String::new()
                };
                // 上屏方式已经改成两个自绘分段按钮、点了就写库，这里不再从控件读
                let mode: Option<&str> = None;
                if step == STEP_PROBE {
                    w.probe_text.clear();
                    w.probe_hint.clear();
                    w.probe_passed = false;
                }
                let core = w.core;
                let cur = w.step;
                w.step = step;
                (core, cur, hotkey, mode)
            }
        }
    };
    let _ = cur_step;
    if !hotkey.trim().is_empty() {
        set_cfg(core, serde_json::json!({"hotkey": hotkey.trim()}));
    }
    if let Some(m) = mode {
        set_cfg(core, serde_json::json!({"insert_mode": m}));
    }
    crate::reload_hotkey();
    // 麦克风自检页：进来开电平、离开就停（别让 meter 一直跑着占麦克风）
    // 注意：core 的 meter 开关是引用计数的语义之外的独立开关，这里按页切换即可。
    if step == STEP_MIC {
        let _ = core.meter_start();
    } else if cur_step == STEP_MIC {
        let _ = core.meter_stop();
    }
    build(hwnd);
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
                let hdc = windows::Win32::Graphics::Gdi::HDC(lp.0 as *mut core::ffi::c_void);
                SetTextColor(hdc, COLORREF(C_TEXT));
                SetBkColor(hdc, COLORREF(C_EDIT));
                let b = cached_brush(0);
                LRESULT(b.0 as isize)
            }
            WM_DRAWITEM => {
                let dis = &*(lp.0 as *const DRAWITEMSTRUCT);
                let id = dis.CtlID as usize;
                // 「试说一句」没成功时，下一步长得要像不能点（逻辑上 ID_NEXT 也不前进）
                // 这里绝不能用阻塞锁：WM_DRAWITEM 可能是 build() 创建控件时同步回调进来的，
                // 而 build 正持着 WIN 锁 → 直接死锁。拿不到就按默认样子画，下一次 WM_PAINT 再修正。
                let (blocked, mode_paste, polish_on) = match WIN.try_lock() {
                    Ok(g) => match g.as_ref() {
                        Some(w) => {
                            let c = cfg_json(w.core);
                            (
                                id == ID_NEXT
                                    && ((w.step == STEP_PROBE && !w.probe_passed)
                                        || (w.step == STEP_MODEL && !w.asr_ready)),
                                c.get("insert_mode").and_then(|v| v.as_str()).unwrap_or("paste")
                                    != "keystroke",
                                c.get("polish_enabled").and_then(|v| v.as_bool()).unwrap_or(false),
                            )
                        }
                        None => (false, true, false),
                    },
                    Err(_) => (false, true, false),
                };
                let segment_on = (id == ID_MODE_PASTE && mode_paste) || (id == ID_MODE_TYPE && !mode_paste);
                let primary = (matches!(id, ID_NEXT | ID_PULL) || segment_on) && !blocked;
                // 按钮圆角外的四个角会露出系统按钮的浅色底，先按所在区域填一次
                let surround = if matches!(id, ID_NEXT | ID_BACK | ID_LATER) { C_BG } else { C_CARD };
                let fill = CreateSolidBrush(COLORREF(surround));
                let _ = FillRect(dis.hDC, &dis.rcItem, fill);
                let _ = DeleteObject(fill);
                let bg = if primary {
                    C_ACCENT
                } else if blocked {
                    // 比「上一步」的底更深一点，和可用按钮一眼能分开
                    (0x00_3A_3A_3A) as u32
                } else {
                    C_EDIT
                };
                round_fill(dis.hDC, dis.rcItem, px(1.0, 6.0), bg);
                // 键盘焦点：Tab 到这里时能看见（主操作蓝底用白描边）
                if GetFocus() == dis.hwndItem {
                    let pen = CreatePen(PS_SOLID, 2, COLORREF(if primary { 0x00FFFFFF } else { C_ACCENT }));
                    let oldpen = SelectObject(dis.hDC, pen);
                    let oldbrush = SelectObject(dis.hDC, GetStockObject(NULL_BRUSH));
                    let mut ring = dis.rcItem;
                    ring.left += 2;
                    ring.top += 2;
                    ring.right -= 2;
                    ring.bottom -= 2;
                    let _ = RoundRect(dis.hDC, ring.left, ring.top, ring.right, ring.bottom, px(1.0, 5.0), px(1.0, 5.0));
                    SelectObject(dis.hDC, oldbrush);
                    SelectObject(dis.hDC, oldpen);
                    let _ = DeleteObject(pen);
                }
                let label = match id {
                    ID_NEXT => {
                        let step = crate::lock(&WIN).as_ref().map(|w| w.step).unwrap_or(0);
                        if step == STEP_LAST { "开始使用" } else { "下一步" }
                    }
                    ID_BACK => "上一步",
                    ID_LATER => "稍后再说",
                    ID_PULL => "下载 SenseVoice",
                    ID_USE => "用它",
                    ID_POLISH if polish_on => "已开启，点此关闭",
                    ID_POLISH => "开启润色",
                    ID_MODE_PASTE => "粘贴",
                    ID_MODE_TYPE => "逐字打字",
                    _ => "",
                };
                let color = if primary {
                    0x00FFFFFF
                } else if blocked {
                    C_MUTED
                } else {
                    C_TEXT
                };
                let f = crate::lock(&WIN).as_ref().map(|w| w.f_body).unwrap_or_default();
                let old = SelectObject(dis.hDC, f);
                let t = wide(label);
                let mut r = dis.rcItem;
                SetBkMode(dis.hDC, windows::Win32::Graphics::Gdi::TRANSPARENT);
                SetTextColor(dis.hDC, COLORREF(color));
                let _ = DrawTextW(dis.hDC, &mut t[..t.len() - 1].to_vec(), &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                SelectObject(dis.hDC, old);
                LRESULT(1)
            }
            WM_COMMAND => {
                let id = (wp.0 & 0xffff) as usize;
                // 控件通知（EN_SETFOCUS/EN_KILLFOCUS 等）不要走下面的分支：销毁控件时
                // 会同步发这些通知，而此刻可能正持着 WIN 锁（build 里），再锁一次就是死锁。
                if (wp.0 >> 16) != 0 {
                    return LRESULT(0);
                }
                match id {
                    ID_NEXT => {
                        let (step, passed) = {
                            let g = crate::lock(&WIN);
                            match g.as_ref() {
                                Some(w) => (w.step, w.probe_passed),
                                None => (0, false),
                            }
                        };
                        let ready = crate::lock(&WIN).as_ref().map(|w| w.asr_ready).unwrap_or(false);
                        if step == STEP_MODEL && !ready {
                            // 没模型就往下走只会死在试说那一步，这里直接说清楚
                            crate::message_box(
                                "先下载语音模型（点「下载 SenseVoice」，约 233MB），装好会提示。\n也可以点「稍后再说」，下次启动会回到这一步。",
                            );
                        } else if step == STEP_PROBE && !passed {
                            // 试说没过：不往下走，提示继续试
                            if let Ok(mut g) = WIN.lock() {
                                if let Some(w) = g.as_mut() {
                                    if w.probe_hint.is_empty() {
                                        w.probe_hint = "还没出字：按住键把例句读一遍。".to_string();
                                    }
                                }
                            }
                            probe_repaint(hwnd);
                        } else if step >= STEP_LAST {
                            finish(hwnd);
                        } else {
                            advance(hwnd, step + 1);
                        }
                    }
                    ID_BACK => {
                        let step = crate::lock(&WIN).as_ref().map(|w| w.step).unwrap_or(0);
                        advance(hwnd, step.saturating_sub(1));
                    }
                    ID_LATER => finish(hwnd),
                    ID_PULL => {
                        let c = crate::lock(&WIN).as_ref().map(|w| w.core);
                        if let Some(c) = c {
                            let _ = c.pull_model("sense");
                        }
                    }
                    ID_USE => {
                        let c = crate::lock(&WIN).as_ref().map(|w| w.core);
                        if let Some(c) = c {
                            set_cfg(c, serde_json::json!({"engine": "sense"}));
                        }
                    }
                    ID_MODE_PASTE | ID_MODE_TYPE => {
                        if let Some(c) = crate::lock(&WIN).as_ref().map(|w| w.core) {
                            let mode = if id == ID_MODE_TYPE { "keystroke" } else { "paste" };
                            set_cfg(c, serde_json::json!({"insert_mode": mode}));
                        }
                        build(hwnd);
                    }
                    ID_POLISH => {
                        let on = crate::lock(&WIN)
                            .as_ref()
                            .map(|w| cfg_json(w.core).get("polish_enabled").and_then(|v| v.as_bool()).unwrap_or(false))
                            .unwrap_or(false);
                        if let Some(c) = crate::lock(&WIN).as_ref().map(|w| w.core) {
                            set_cfg(c, serde_json::json!({"polish_enabled": !on}));
                        }
                        build(hwnd);
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_NCHITTEST => {
                let x = (lp.0 & 0xffff) as i16 as i32;
                let y = ((lp.0 >> 16) & 0xffff) as i16 as i32;
                let mut wr = RECT::default();
                let _ = GetWindowRect(hwnd, &mut wr);
                let k = scale();
                let in_header = y - wr.top < px(k, HEADER);
                let in_close = in_header && x - wr.left > wr.right - wr.left - px(k, 40.0);
                if in_close {
                    LRESULT(HTCLIENT as isize)
                } else if in_header {
                    LRESULT(HTCAPTION as isize)
                } else {
                    DefWindowProcW(hwnd, msg, wp, lp)
                }
            }
            WM_KEYDOWN => {
                let vk = wp.0 as u32;
                match vk {
                    0x09 => {
                        // Tab / Shift+Tab
                        let back = GetKeyState(0x10) < 0;
                        if let Ok(next) = GetNextDlgTabItem(hwnd, GetFocus(), windows::Win32::Foundation::BOOL::from(back)) {
                            let _ = SetFocus(next);
                        }
                        LRESULT(0)
                    }
                    0x0D => {
                        // 回车 = 按下当前聚焦的按钮
                        let f = GetFocus();
                        let mut buf = [0u16; 16];
                        let n = GetClassNameW(f, &mut buf);
                        if String::from_utf16_lossy(&buf[..n as usize]) == "Button" {
                            let _ = PostMessageW(f, 0x00F5, WPARAM(0), LPARAM(0));
                        }
                        LRESULT(0)
                    }
                    0x1B => {
                        // Esc = 稍后再说（与点✕同一语义：记 done=true，下次从试说页回来）
                        finish(hwnd);
                        LRESULT(0)
                    }
                    _ => DefWindowProcW(hwnd, msg, wp, lp),
                }
            }
            WM_LBUTTONUP => {
                let x = (lp.0 & 0xffff) as i16 as i32;
                let y = ((lp.0 >> 16) & 0xffff) as i16 as i32;
                let mut wr = RECT::default();
                let _ = GetWindowRect(hwnd, &mut wr);
                let k = scale();
                // 注意：x/y 是客户区坐标，别拿屏幕坐标的 wr.left/top 混着算（之前 ✕ 永远点不到）
                let client_w = wr.right - wr.left;
                if y < px(k, HEADER) && x > client_w - px(k, 40.0) {
                    finish(hwnd);
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                if let Ok(mut g) = EFFECTIVE_K.lock() {
                    *g = 0.0;
                }
                *crate::lock(&WIN) = None;
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
