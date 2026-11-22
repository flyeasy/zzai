//! Windows 设置窗口（W3）：左侧分页 + 右侧原生控件。
//!
//! 目标不是「好看」，而是**让 Windows 版不再只能手改 TOML**：
//! 通用（快捷键/最短录音/上限/上屏方式/隐私/开机启动）、引擎（模型列表/切换/下载）、
//! 音频（设备/降噪）、诊断（配置全文/复制/打开目录）。
//!
//! 所有读写都走 core 的 JSON 面（`eryu_get_config_json` / `eryu_set_config_json`），
//! 与 macOS 设置页改的是同一份配置，不存在两套真相。

use std::sync::atomic::Ordering;
use std::sync::Mutex;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush, DEFAULT_PITCH,
    DeleteObject, DrawTextW, FF_DONTCARE, FillRect, GB2312_CHARSET, HBRUSH, HFONT, InvalidateRect,
    UpdateWindow,
    OUT_DEFAULT_PRECIS, RoundRect, SelectObject, SetBkColor, SetBkMode, SetTextColor,
    DT_CENTER, DT_SINGLELINE, DT_VCENTER, DT_LEFT, DT_WORDBREAK, DT_END_ELLIPSIS, TRANSPARENT, OPAQUE, PAINTSTRUCT,
    BeginPaint, EndPaint,
    DRAW_TEXT_FORMAT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetFocus, GetKeyState, SetFocus,
};

use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics;
use windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN;
use windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::WindowsAndMessaging::HMENU;

const CLASS: &str = "EryuSettingsWnd";
/// 自绘 ⓘ 悬停提示（系统 tooltip 在自绘窗口上不可靠，自己画一个）
const TIP_CLASS: &str = "EryuTipWnd";
static TIPS: Mutex<Vec<(i32, i32, i32, i32, String)>> = Mutex::new(Vec::new());
static TIP_HWND: Mutex<isize> = Mutex::new(0);
static TIP_TEXT: Mutex<String> = Mutex::new(String::new());
const ID_NAV: usize = 100;
const ID_CLOSE_HEADER: usize = 251;
const ID_SAVE_TOML: usize = 252;
const ID_HOTKEY_PRESET_ALT: usize = 240;
const ID_HOTKEY_PRESET_CAS: usize = 241;
const ID_HOTKEY_PRESET_F13: usize = 242;
const ID_HOTKEY_CLEAR: usize = 243;
/// 上屏方式：自绘分段按钮（原生 COMBOBOX 在深色界面里永远是白底，破坏一致性）
const ID_MODE_PASTE: usize = 156;
const ID_MODE_TYPE: usize = 157;
const ID_POLISH_PRESET: usize = 260; // 260..266 = 服务商预设（对齐 mac ProviderPreset）

/// 自绘顶栏高度（原生标题栏在部分系统上仍是浅色，直接自己画一条）
const HEADER: f32 = 36.0;
const ID_SAVE: usize = 101;
const ID_LIST: usize = 102;
const ID_USE: usize = 103;
const ID_PULL: usize = 104;
/// 引擎页「下载设置」两行共用一个保存
const ID_ENGINE_SAVE: usize = 109;
const ID_COPY: usize = 105;
const ID_OPENDIR: usize = 106;
const ID_DENOISE: usize = 110;
const ID_HW_SUG: usize = 190;
/// 「学到的改法」采纳 / 忽略（DESIGN §10.2 热词自动学习）
const ID_OBS_ADOPT: usize = 270;
const ID_OBS_IGNORE: usize = 271;
const ID_HW_ADD: usize = 191;
const ID_HW_REFRESH: usize = 192;
const ID_SAVE_HW: usize = 111;
const ID_HIST_REFRESH: usize = 120;
const ID_HIST_COPY: usize = 121;
const ID_HIST_CLEAR: usize = 122;
const ID_HIST_LIST: usize = 123;
const ID_HIST_EN: usize = 124;
const ID_HIST_DEL: usize = 125;
const ID_HIST_RETRY: usize = 126;
const ID_POLISH_EN: usize = 130;
const ID_POLISH_SAVE: usize = 131;
const ID_POLISH_ACT: usize = 132;
const ID_POLISH_SLOT: usize = 134;
const ID_PAUSE_MEDIA: usize = 135;
const ID_MIC_TEST: usize = 140;
const ID_MIC_LABEL: usize = 141;
const ID_PUNCT: usize = 142;
const ID_SOURCE_LOCK: usize = 143;
const ID_PULL_LABEL: usize = 144;
const ID_SELFTEST: usize = 150;
const ID_COPY_DIAG: usize = 152;
const ID_OPENLOG: usize = 153;
const ID_TOML: usize = 154;
/// 诊断包：导出 / 上传（DESIGN §12.8 / D-114）
const ID_DIAG_SAVE: usize = 155;
const ID_DIAG_UPLOAD: usize = 156;
const WM_DIAG_DONE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 10;
static DIAG_RESULT: Mutex<String> = Mutex::new(String::new());
const ID_MICPRIV: usize = 151;
const ID_PERAPP: usize = 160;
const WM_VOICE_DONE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 7;
static VOICE_RESULT: std::sync::Mutex<Option<(i32, String)>> = std::sync::Mutex::new(None);
const ID_POLISH_TEST: usize = 180;
const ID_POLISH_PROMPT_SAVE: usize = 265;
const ID_POLISH_MODELS: usize = 266;
const WM_POLISH_MODELS_DONE: u32 = WM_APP + 9;
static POLISH_MODELS: Mutex<Vec<String>> = Mutex::new(Vec::new());
const ID_POLISH_PROMPT: usize = 227; // 编辑框编号落在 200..229 段内（ID_EDIT_BASE + 27）
const WM_POLISH_DONE: u32 = WM_APP + 8;
static POLISH_RESULT: Mutex<String> = Mutex::new(String::new());

const ID_VOICE_LOCK: usize = 170;
const ID_LAN_EN: usize = 230;
const ID_LAN_PORT: usize = 231;
const ID_LAN_TOKEN: usize = 232;
const ID_LAN_ALLOW: usize = 233;
const ID_LAN_SAVE: usize = 234;
const ID_LAN_REFRESH: usize = 235;
const ID_LAN_GEN: usize = 236;
const ID_MIRROR_OPEN: usize = 403;
const ID_LAN_URL: usize = 237;
const ID_VOICE_ENROLL: usize = 171;
const ID_VOICE_CLEAR: usize = 172;
const ID_PERAPP_SAVE: usize = 161;
const ID_PROXY_SAVE: usize = 136;
const ID_MIRROR_SAVE: usize = 137;
const ID_POLISH_LIST: usize = 133;
const ID_HW_EDIT: usize = 112;
const ID_EDIT_BASE: usize = 200; // 200..229 = 普通输入框（局域网页用 230..235，别撞进来）

pub fn page_count() -> usize { PAGES.len() }

const PAGES: [&str; 9] = ["通用", "引擎", "音频", "热词", "历史", "润色", "诊断", "声纹", "局域网"];
const PAGE_ICONS: [&str; 9] = ["⚙", "◈", "◎", "✎", "≡", "✦", "⌘", "◉", "⇄"];

// 设计稿（design/settings.png）取的色值，COLORREF 是 0x00BBGGRR
const C_BG: u32 = 0x001F1C1B; // #1b1c1f
const C_CARD: u32 = 0x002C2624; // #24262c
const C_EDIT: u32 = 0x0038302E; // #2e3038
const C_ACCENT: u32 = 0x00F6823B; // #3b82f6
const C_TEXT: u32 = 0x00ECE9E9; // #e9e9ec
const C_MUTED: u32 = 0x00A6A09A; // #9aa0a6
const C_LINE: u32 = 0x00453D3A; // #3a3d45

struct Window {
    hwnd: HWND,
    page: usize,
    ctrls: Vec<HWND>,
    k: f32,
    font: HFONT,
    font_section: HFONT,
    font_title: HFONT,
    /// 当前页用到的控件（None = 本页没有它），避免用下标猜
    e_hotkey: Option<HWND>,
    e_min: Option<HWND>,
    e_max: Option<HWND>,
    /// 上屏方式：「粘贴」选中 = true（自绘分段按钮，不用 COMBOBOX）
    mode_paste: bool,
    c_privacy: Option<HWND>,
    c_hist_on: Option<HWND>,
    c_punct: Option<HWND>,
    c_source_lock: Option<HWND>,
    pull_lbl: Option<HWND>,
    e_perapp: Option<HWND>,
    c_voice: Option<HWND>,
    c_lan_en: Option<HWND>,
    c_lan_allow: Option<HWND>,
    e_lan_port: Option<HWND>,
    e_lan_token: Option<HWND>,
    /// 配置里已经有令牌（令牌框留空＝不改）
    lan_has_token: bool,
    lbl_lan: Option<HWND>,
    c_launch: Option<HWND>,
    c_denoise: Option<HWND>,
    lst: Option<HWND>,
    /// 引擎页列表里每一行对应的模型 id（只列识别引擎，顺序与列表一致）
    model_ids: Vec<String>,
    e_diag: Option<HWND>,
    e_toml: Option<HWND>,
    e_hotwords: Option<HWND>,
    e_polish_url: Option<HWND>,
    e_polish_model: Option<HWND>,
    e_polish_key: Option<HWND>,
    e_slot: Option<HWND>,
    e_prompt: Option<HWND>,
    c_pause_media: Option<HWND>,
    lst2: Option<HWND>,
    lst3: Option<HWND>,
    lst_sug: Option<HWND>,
    c_polish: Option<HWND>,
    /// 悬停的 ⓘ 说明（画在同一个窗口里，不做独立弹窗，避免子窗口收不到鼠标）
    hover: Option<(String, i32, i32)>,
    /// 音频页「输入电平」测试：开着的时候 core 每 66ms 推一次 meter
    meter_on: bool,
    meter_lbl: Option<HWND>,
    e_proxy: Option<HWND>,
    e_mirror: Option<HWND>,
    /// 左侧导航每项的可点区域（WM_PAINT 里画，WM_LBUTTONDOWN 里判）
    nav_rects: Vec<(i32, i32, i32, i32)>,
    brush_bg: HBRUSH,
    brush_card: HBRUSH,
    brush_edit: HBRUSH,
}

static WIN: Mutex<Option<Window>> = Mutex::new(None);
/// 导航项的可点区域：单独一把锁，避免 WM_PAINT 里再取 WIN 造成与
/// 「持 WIN 时 SendMessage 回本窗口」的互等（第一版就死在这上面）。
static NAV: Mutex<Vec<(i32, i32, i32, i32)>> = Mutex::new(Vec::new());

// 设置窗口只在 UI 线程创建与访问；HWND 不是 Send，这里由我们保证线程约束
// （与 main.rs 的 App 同一条约定）。
unsafe impl Send for Window {}

/// WM_CTLCOLOR* 里绝不能再锁 WIN：控件创建/重绘时同步回调父窗口，
/// 而调用方（build）正持着 WIN 锁 → 死锁。这里用一次性创建的缓存画刷。
fn cached_brush(kind: u8) -> HBRUSH {
    use std::sync::OnceLock;
    static EDIT: OnceLock<isize> = OnceLock::new();
    static CARD: OnceLock<isize> = OnceLock::new();
    let cell = if kind == 0 { &EDIT } else { &CARD };
    let v = *cell.get_or_init(|| unsafe {
        let c = if kind == 0 { C_EDIT } else { C_CARD };
        CreateSolidBrush(COLORREF(c)).0 as isize
    });
    HBRUSH(v as *mut core::ffi::c_void)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn px<K: Into<f64>, V: Into<f64>>(k: K, v: V) -> i32 {
    (v.into() * k.into()).round() as i32
}

/// 统一字体：见 `crate::ui`
fn ui_font(k: f32, size: f32, weight: i32) -> HFONT {
    crate::ui::font(k, size, weight)
}

fn font(size: i32, weight: i32) -> HFONT {
    let face = wide("Segoe UI");
    unsafe {
        CreateFontW(
            size,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            1u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            PCWSTR(face.as_ptr()),
        )
    }
}

/// 让系统标题栏跟随深色主题（Win11 20H1+ 支持；旧系统静默失败）
fn dark_titlebar(hwnd: HWND) {
    unsafe {
        let on: i32 = 1;
        let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd,
            windows::Win32::Graphics::Dwm::DWMWA_USE_IMMERSIVE_DARK_MODE,
            &on as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// 打开（或聚焦）设置窗口；只能从 UI 线程调用。
pub fn open() {
    open_at(0);
}

/// 打开并直接落在某一页（只建一次窗口，避免「先建第 0 页再重建」的空窗问题）
pub fn open_at(page: usize) {
    {
        let g = crate::lock(&WIN);
        if let Some(w) = g.as_ref() {
            unsafe {
                ShowWindow(w.hwnd, SW_SHOWNORMAL);
                SetForegroundWindow(w.hwnd);
            }
            return;
        }
    }
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let mut wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: PCWSTR(wide(CLASS).as_ptr()),
            hbrBackground: CreateSolidBrush(COLORREF(C_BG)),
            style: CS_DROPSHADOW,
            ..Default::default()
        };
        RegisterClassW(&wc);
        {
            let mut tc = WNDCLASSW {
                lpfnWndProc: Some(tip_wndproc),
                hInstance: hinst.into(),
                lpszClassName: PCWSTR(wide(TIP_CLASS).as_ptr()),
                hbrBackground: CreateSolidBrush(COLORREF(C_CARD)),
                ..Default::default()
            };
            RegisterClassW(&tc);
            let _ = &mut tc;
        }
        if let Ok(mut t) = TIPS.lock() {
            t.clear();
        }
        if page == 3 {
            refresh_suggestions();
        }
        // 居中出现，避免贴屏幕边缘把左侧导航裁掉
        let (sx, sy) = unsafe {
            (
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
            )
        };
        let wx = ((sx - px(1.0, 900)) / 2).max(0);
        let wy = ((sy - px(1.0, 632)) / 2).max(0);
        crate::report("SETTINGS create 开始");
        // WS_EX_APPWINDOW：无边框 WS_POPUP 默认不进任务栏/Alt+Tab，用户把窗口
        // 点到别的窗口后面就只剩托盘入口能找回来。加这个让它像正常应用窗口一样
        // 出现在任务栏（隐藏时 Windows 会自动摘掉，不留残影）。
        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            PCWSTR(wide(CLASS).as_ptr()),
            PCWSTR(wide("字在 设置").as_ptr()),
            WS_POPUP | WS_CLIPCHILDREN,
            wx,
            wy,
            px(1.0, 900),
            px(1.0, 632),
            HWND::default(),
            None,
            hinst,
            None,
        )
        .unwrap_or_default();
        if hwnd.0.is_null() {
            crate::report("SETTINGS create 失败 hwnd=null");
            return;
        }
        crate::report("SETTINGS create 成功");
        let mut k = GetDpiForWindow(hwnd) as f32 / 96.0;
        if let Ok(v) = std::env::var("ERYU_SETTINGS_DPI") {
            if let Ok(p) = v.parse::<u32>() {
                k = p as f32 / 100.0;
            }
        }
        // 125%/150% 下 900×632 会高过工作区 → 按工作区收一个上限，保证整窗点得到
        let k = crate::ui::fit_scale(if k < 1.0 { 1.0 } else { k }, 900.0, 632.0);
        crate::report(&format!("SETTINGS dpi={} k={k}", GetDpiForWindow(hwnd)));
        SetWindowPos(
            hwnd,
            HWND::default(),
            0,
            0,
            px(k, 900),
            px(k, 632),
            SWP_NOMOVE | SWP_NOZORDER,
        );
        dark_titlebar(hwnd);
        // 无边框窗口自己做圆角（12px），否则四角是直角
        let rgn = windows::Win32::Graphics::Gdi::CreateRoundRectRgn(
            0,
            0,
            px(k, 900) + 1,
            px(k, 632) + 1,
            px(k, 24),
            px(k, 24),
        );
        let _ = windows::Win32::Graphics::Gdi::SetWindowRgn(hwnd, rgn, true);
        let w = Window {
            hwnd,
            page: page.min(PAGES.len() - 1),
            ctrls: Vec::new(),
            k,
            font: ui_font(k, crate::ui::BODY, 400),
            font_section: ui_font(k, crate::ui::SECTION, 600),
            font_title: ui_font(k, crate::ui::TITLE, 600),
            e_hotkey: None,
            e_min: None,
            e_max: None,
            mode_paste: true,
            c_privacy: None,
            c_hist_on: None,
            c_punct: None,
            c_source_lock: None,
            pull_lbl: None,
            e_perapp: None,
            c_voice: None,
            c_lan_en: None,
            c_lan_allow: None,
            e_lan_port: None,
            e_lan_token: None,
            lan_has_token: false,
            lbl_lan: None,
            c_launch: None,
            c_denoise: None,
            lst: None,
            model_ids: Vec::new(),
            e_diag: None,
            e_toml: None,
            e_hotwords: None,
            e_polish_url: None,
            e_polish_model: None,
            e_polish_key: None,
            e_slot: None,
            e_prompt: None,
            c_pause_media: None,
            e_proxy: None,
            e_mirror: None,
            lst2: None,
            lst3: None,
            lst_sug: None,
            c_polish: None,
            hover: None,
            meter_on: false,
            meter_lbl: None,
            nav_rects: Vec::new(),
            brush_bg: CreateSolidBrush(COLORREF(C_BG)),
            brush_card: CreateSolidBrush(COLORREF(C_CARD)),
            brush_edit: CreateSolidBrush(COLORREF(C_EDIT)),
        };
        *crate::lock(&WIN) = Some(w);
        build(hwnd);
        // 焦点放到第一个输入框：键盘能直接改，也让人一眼看到「这里可以输入」。
        // 注意不能无脑取 ctrls.first()——历史/热词页第一个控件是 LISTBOX，
        // 蓝色焦点框会画在一个大列表外面，看起来像"选中了整块区域"。
        let first_input = crate::lock(&WIN)
            .as_ref()
            .and_then(|w| w.ctrls.iter().copied().find(|h| is_input(*h)));
        if let Some(first) = first_input {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(first);
        }
        // 居中
        let mut r = windows::Win32::Foundation::RECT::default();
        GetWindowRect(hwnd, &mut r).ok();
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        SetWindowPos(
            hwnd,
            HWND::default(),
            (sw - (r.right - r.left)) / 2,
            (sh - (r.bottom - r.top)) / 2,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER,
        );
        ShowWindow(hwnd, SW_SHOWNORMAL);
        SetForegroundWindow(hwnd);
        // build() 创建控件时会同步触发 WM_PAINT，那时 WIN 锁还被持有；旧版 paint()
        // 拿不到锁就整帧返回，卡片底色因此缺失（I-105）。这里强制在锁释放后重画一遍。
        let _ = InvalidateRect(hwnd, None, true);
        let _ = UpdateWindow(hwnd);
    }
}

fn child(class: &str, text: &str, style: u32, w: &Window, x: f32, y: f32, cw: f32, ch: f32, id: usize) -> HWND {
    unsafe {
        let _ = class;
        let h = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(wide(class).as_ptr()),
            PCWSTR(wide(text).as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | style),
            px(w.k, x),
            px(w.k, y + HEADER),
            px(w.k, cw),
            px(w.k, ch),
            w.hwnd,
            HMENU(id as *mut core::ffi::c_void),
            GetModuleHandleW(None).unwrap_or_default(),
            None,
        )
        .unwrap_or_default();
        SendMessageW(h, WM_SETFONT, WPARAM(w.font.0 as usize), LPARAM(1));
        if class == "EDIT" || class == "COMBOBOX" {
            round_control(h, (6.0 * w.k).round());
        } else if class == "LISTBOX" {
            round_control(h, (8.0 * w.k).round());
        }
        // 列表的滚动条是原生绘制的：不套深色主题时，深色页面上会出现一条白槽（150% 缩放下
        // 行高变大、列表放不下全部行就会露出来）。Win11 上换 Explorer 深色主题即可。
        if class == "LISTBOX" {
            let dark = wide("DarkMode_Explorer");
            let _ = windows::Win32::UI::Controls::SetWindowTheme(
                h,
                PCWSTR(dark.as_ptr()),
                PCWSTR::null(),
            );
        }
        // 关掉 EDIT/COMBO 的浅色视觉样式，否则系统主题把白底画回来；COMBO 再套深色主题
        if class == "EDIT" || class == "COMBOBOX" {
            let blank = wide(" ");
            let _ = windows::Win32::UI::Controls::SetWindowTheme(
                h,
                PCWSTR(blank.as_ptr()),
                PCWSTR(blank.as_ptr()),
            );
            if class == "COMBOBOX" {
                let dark = wide("DarkMode_CFD");
                let _ = windows::Win32::UI::Controls::SetWindowTheme(
                    h,
                    PCWSTR(dark.as_ptr()),
                    PCWSTR::null(),
                );
            }
        }
        h
    }
}

/// 给原生控件套圆角 region（EDIT/COMBOBOX/LISTBOX 默认是直角，看起来「断线」）
fn round_control(h: HWND, radius: f32) {
    unsafe {
        let mut rc = windows::Win32::Foundation::RECT::default();
        let _ = GetClientRect(h, &mut rc);
        let r = (radius as i32).max(2);
        let rgn = windows::Win32::Graphics::Gdi::CreateRoundRectRgn(
            0,
            0,
            rc.right + 1,
            rc.bottom + 1,
            r,
            r,
        );
        let _ = windows::Win32::Graphics::Gdi::SetWindowRgn(h, rgn, true);
    }
}

unsafe extern "system" fn tip_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc = windows::Win32::Foundation::RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let k = GetDpiForWindow(hwnd) as f32 / 96.0;
            let brush = CreateSolidBrush(COLORREF(C_CARD));
            FillRect(hdc, &rc, brush);
            let _ = DeleteObject(brush);
            let text = TIP_TEXT.lock().map(|t| t.clone()).unwrap_or_default();
            let f = crate::ui::font(k, 12.0, 400);
            let old = SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(f.0));
            SetTextColor(hdc, COLORREF(C_TEXT));
            SetBkMode(hdc, TRANSPARENT);
            let mut r = rc;
            r.left += px(k, 10.0);
            r.right -= px(k, 10.0);
            r.top += px(k, 6.0);
            r.bottom -= px(k, 6.0);
            DrawTextW(hdc, &mut wide(&text), &mut r, DT_LEFT | DT_WORDBREAK);
            let _ = SelectObject(hdc, old);
            let _ = DeleteObject(f);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn show_tip_at(x: i32, y: i32, text: &str) {
    let hinst = GetModuleHandleW(None).unwrap_or_default();
    let mut h = *crate::lock(&TIP_HWND);
    let k = GetDpiForWindow(HWND(std::ptr::null_mut())) as f32 / 96.0;
    // 尺寸：按文字宽度估算，最多两行
    let f = crate::ui::font(k, 12.0, 400);
    let mut rc = windows::Win32::Foundation::RECT { left: 0, top: 0, right: px(k, 300.0), bottom: 0 };
    let hdc = windows::Win32::Graphics::Gdi::GetDC(HWND(std::ptr::null_mut()));
    let old = SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(f.0));
    DrawTextW(
        hdc,
        &mut wide(text),
        &mut rc,
        DT_LEFT | DT_WORDBREAK | DRAW_TEXT_FORMAT(0x0400),
    );
    let _ = SelectObject(hdc, old);
    let _ = DeleteObject(f);
    let _ = windows::Win32::Graphics::Gdi::ReleaseDC(HWND(std::ptr::null_mut()), hdc);
    let tw = (rc.right + px(k, 20.0)).min(px(k, 360.0));
    let th = rc.bottom + px(k, 12.0);
    if h == 0 {
        h = CreateWindowExW(
            WINDOW_EX_STYLE(0x08000000 | 0x00000008 | 0x00000080),
            PCWSTR(wide(TIP_CLASS).as_ptr()),
            PCWSTR::null(),
            WINDOW_STYLE(WS_POPUP.0),
            x + px(k, 14.0),
            y,
            tw,
            th,
            HWND::default(),
            None,
            hinst,
            None,
        )
        .map(|w| w.0 as isize)
        .unwrap_or(0);
        *crate::lock(&TIP_HWND) = h;
        let rgn = windows::Win32::Graphics::Gdi::CreateRoundRectRgn(0, 0, tw + 1, th + 1, px(k, 8.0), px(k, 8.0));
        let _ = windows::Win32::Graphics::Gdi::SetWindowRgn(HWND(h as *mut _), rgn, true);
    } else {
        SetWindowPos(
            HWND(h as *mut _),
            HWND_TOP,
            x + px(k, 14.0),
            y,
            tw,
            th,
            SWP_NOACTIVATE,
        );
    }
    *crate::lock(&TIP_TEXT) = text.to_string();
    if let Some(w) = TIP_HWND.lock().ok().map(|h| *h) {
        let _ = SetWindowPos(HWND(w as *mut _), HWND_TOP, x + px(k, 14.0), y, tw, th, SWP_NOACTIVATE);
        let _ = ShowWindow(HWND(w as *mut _), SW_SHOWNA);
        let _ = InvalidateRect(HWND(w as *mut _), None, true);
    }
}

unsafe fn hide_tip() {
    let h = *crate::lock(&TIP_HWND);
    if h != 0 {
        let _ = ShowWindow(HWND(h as *mut _), SW_HIDE);
    }
}

fn label(w: &mut Window, text: &str, x: f32, y: f32, cw: f32, ch: f32) {
    let h = child("STATIC", text, 0, w, x, y, cw, ch, 0);
    w.ctrls.push(h);
}

/// ⓘ 图标：只登记热区，图标由 paint() 画（不放控件，父窗口才能收到 WM_MOUSEMOVE）。
fn info(w: &mut Window, x: f32, y: f32, text: &str) {
    if let Ok(mut t) = TIPS.lock() {
        t.push((
            px(w.k, x),
            px(w.k, y + HEADER),
            px(w.k, 16.0),
            px(w.k, 18.0),
            text.to_string(),
        ));
    }
}

fn edit(w: &mut Window, text: &str, id: usize, x: f32, y: f32, cw: f32) -> HWND {
    // WS_TABSTOP：Tab 能走到输入框（I-133 的键盘可达不能漏掉文本框）
    let h = child(
        "EDIT",
        text,
        WS_BORDER.0 | ES_AUTOHSCROLL as u32 | WS_TABSTOP.0,
        w,
        x,
        y,
        cw,
        28.0,
        id,
    );
    w.ctrls.push(h);
    h
}

fn button(w: &mut Window, text: &str, id: usize, x: f32, y: f32, cw: f32) {
    // 自绘按钮：深色圆角，主操作蓝色（与设计稿一致），其余灰边
    // WS_TABSTOP：键盘用户能 Tab 到按钮（Windows 规范里焦点必须能走到主操作）
    let h = child(
        "BUTTON",
        text,
        BS_OWNERDRAW as u32 | WS_TABSTOP.0,
        w,
        x,
        y,
        cw,
        34.0,
        id,
    );
    w.ctrls.push(h);
}

/// 哪些按钮算「主操作」（蓝色实心）
fn is_primary(id: usize) -> bool {
    matches!(id, ID_SAVE | ID_SAVE_HW | ID_POLISH_SAVE | ID_USE)
}

/// 分段按钮：选中的那一半用主操作配色
fn segment_on(id: usize) -> Option<bool> {
    match id {
        ID_MODE_PASTE | ID_MODE_TYPE => WIN
            .try_lock()
            .ok()
            .and_then(|g| g.as_ref().map(|w| w.mode_paste))
            .map(|paste| (id == ID_MODE_PASTE) == paste),
        _ => None,
    }
}

fn checkbox(w: &mut Window, text: &str, id: usize, checked: bool, x: f32, y: f32, cw: f32) -> HWND {
    // 自绘开关（设计稿是 iOS 式 pill，不是系统方框）
    let h = child(
        "BUTTON",
        text,
        BS_OWNERDRAW as u32 | WS_TABSTOP.0,
        w,
        x,
        y,
        cw,
        24.0,
        id,
    );
    set_toggle(id, checked);
    w.ctrls.push(h);
    h
}

fn listbox(w: &mut Window, id: usize, x: f32, y: f32, cw: f32, ch: f32) -> HWND {
    let h = child(
        "LISTBOX",
        "",
        WS_BORDER.0
            | WS_VSCROLL.0
            | LBS_NOTIFY as u32
            | LBS_OWNERDRAWFIXED as u32
            | LBS_HASSTRINGS as u32
            | WS_TABSTOP.0,
        w,
        x,
        y,
        cw,
        ch,
        id,
    );
    w.ctrls.push(h);
    // 行高固定，列表高度按它取整，末行就不会被窗口下沿切一半
    unsafe {
        SendMessageW(h, LB_SETITEMHEIGHT, WPARAM(0), LPARAM(px(w.k, 32.0) as isize));
    }
    h
}

fn text_of(h: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 512];
        let n = GetWindowTextW(h, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

fn set_text(h: HWND, s: &str) {
    unsafe {
        SetWindowTextW(h, PCWSTR(wide(s).as_ptr()));
    }
}

fn get_int(h: HWND) -> Option<i64> {
    text_of(h).trim().parse::<i64>().ok()
}

fn cfg() -> serde_json::Value {
    let app = crate::lock(&crate::APP);
    match app.as_ref() {
        Some(a) => serde_json::from_str(&a.core.config_json()).unwrap_or(serde_json::Value::Null),
        None => serde_json::Value::Null,
    }
}

fn build(hwnd: HWND) {
    let mut g = crate::lock(&WIN);
    let Some(w) = g.as_mut() else { return };
    let ctrls = std::mem::take(&mut w.ctrls);
    for h in ctrls {
        unsafe {
            DestroyWindow(h);
        }
    }
    // 兜底：清掉所有漏登记的普通子窗口（比如键帽控件），否则每重建一次就漏一个窗口。
    // 直接遍历 GW_CHILD 反复销毁，不依赖各页面有没有记得 push 到 ctrls。
    unsafe {
        for _ in 0..500 {
            let Ok(c) = GetWindow(hwnd, GW_CHILD) else {
                break;
            };
            let _ = DestroyWindow(c);
        }
    }
    if let Ok(mut t) = TIPS.lock() {
        t.clear();
    }
    // 关闭按钮放在清空之后建，否则会被上面这轮 DestroyWindow 一起删掉（之前顶栏没有 ✕ 就是因为这个）
    // 注意：这里已经持有 WIN 锁，不能再调 add_header_close()（会二次加锁死锁）
    // child() 统一给 y 加 HEADER，这里传负值让按钮落在顶栏上（4..40），否则会压到内容第一行
    button(w, "✕", ID_CLOSE_HEADER, 846.0, 4.0 - HEADER, 36.0);
    // 左侧导航由 WM_PAINT 自绘（见 paint()），这里不建子窗口
    w.e_hotkey = None;
    w.e_min = None;
    w.e_max = None;
    w.mode_paste = true;
    w.c_privacy = None;
    w.c_hist_on = None;
    w.c_punct = None;
    w.c_source_lock = None;
    w.pull_lbl = None;
    w.e_perapp = None;
    w.c_voice = None;
    w.c_launch = None;
    w.c_denoise = None;
    w.lst = None;
    w.e_diag = None;
    w.e_toml = None;
    w.e_hotwords = None;
    w.lst_sug = None;
    let page = w.page;
    let _ = hwnd;
    {
        use std::io::Write;
        let n = w.ctrls.len();
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!("{}/settings.log", crate::config_dir()))
        {
            let _ = writeln!(f, "build page={page} ctrls={n}");
        }
    }
    // 单页构建出错不能带走整个进程（I-108 的同类教训）：包一层 catch_unwind，
    // 出错时只打日志，页面留空，用户还能切到别的页。
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match page {
        0 => page_general(w),
        4 => page_history(w),
        5 => page_polish(w),
        1 => page_engine(w),
        2 => page_audio(w),
        3 => page_hotwords(w),
        7 => page_voice(w),
        8 => page_lan(w),
        _ => page_diag(w),
    }));
    if r.is_err() {
        eprintln!("[eryu] 设置页 {page} 构建出错（已跳过，程序继续）");
    }
}

fn section(w: &mut Window, text: &str, y: f32) {
    let h = child("STATIC", text, 0, w, 196.0, y, 664.0, 20.0, 0);
    let f = w.font_section;
    unsafe {
        SendMessageW(h, WM_SETFONT, WPARAM(f.0 as usize), LPARAM(1));
    }
    w.ctrls.push(h);
}

fn page_general(w: &mut Window) {
    let c = cfg();
    let hotkey = c["hotkey"].as_str().unwrap_or("ALT_R").to_string();
    let min_ms = c["min_record_ms"].as_i64().unwrap_or(400);
    let max_s = c["max_record_secs"].as_i64().unwrap_or(120);
    let mode = c["insert_mode"].as_str().unwrap_or("paste").to_string();
    let privacy = c["privacy_mode"].as_bool().unwrap_or(false);
    let launch = c["launch_at_login"].as_bool().unwrap_or(false);
    let kk = w.k;

    // 行式布局：左边标签，右边控件（对齐 design/settings.png 的行结构）
    section(w, "快捷键与拾音", 24.0);

    label(w, "按住说话的键", 196.0, 56.0, 420.0, 18.0);
    w.e_hotkey = Some(crate::keycap::create(
        w.hwnd,
        px(kk, 660.0),
        px(kk, 52.0 + HEADER),
        px(kk, 180.0),
        px(kk, 28.0),
        ID_EDIT_BASE as i32,
        &hotkey,
        kk,
    ));

    label(w, "常用", 196.0, 96.0, 60.0, 18.0);
    button(w, "右 Alt", ID_HOTKEY_PRESET_ALT, 500.0, 92.0, 78.0);
    button(w, "Ctrl+Alt+空格", ID_HOTKEY_PRESET_CAS, 588.0, 92.0, 116.0);
    button(w, "F13", ID_HOTKEY_PRESET_F13, 714.0, 92.0, 56.0);
    button(w, "清空", ID_HOTKEY_CLEAR, 780.0, 92.0, 60.0);

    label(w, "最短录音（毫秒）", 196.0, 144.0, 420.0, 18.0);
    w.e_min = Some(edit(w, &min_ms.to_string(), ID_EDIT_BASE + 1, 740.0, 140.0, 120.0));

    label(w, "单次录音上限（秒）", 196.0, 188.0, 420.0, 18.0);
    w.e_max = Some(edit(w, &max_s.to_string(), ID_EDIT_BASE + 2, 740.0, 184.0, 120.0));

    label(w, "上屏方式", 196.0, 232.0, 420.0, 18.0);
    w.mode_paste = mode != "keystroke";
    button(w, "粘贴", ID_MODE_PASTE, 620.0, 228.0, 110.0);
    button(w, "逐字打字", ID_MODE_TYPE, 738.0, 228.0, 122.0);

    // 开关：标签在左，滑块靠右（draw_toggle 里 track 右对齐）
    w.c_privacy = Some(checkbox(w, "隐私模式：不写历史、不落盘", ID_EDIT_BASE + 4, privacy, 196.0, 280.0, 664.0));
    w.c_launch = Some(checkbox(w, "开机自动启动", ID_EDIT_BASE + 5, launch, 196.0, 318.0, 664.0));

    button(w, "保存", ID_SAVE, 764.0, 362.0, 96.0);

    // 按应用覆盖上屏方式（对齐 mac 的「上屏 · 按应用覆盖」；Windows 版用行文本，省掉表格控件）
    label(w, "按应用覆盖", 196.0, 408.0, 96.0, 18.0);
    info(w, 300.0, 405.0, "一行一个程序：粘贴=模拟 Ctrl+V，打字=逐字输入。没列出的程序用上面选的默认方式。");
    label(w, "每行 进程名=粘贴 / 进程名=打字", 318.0, 522.0, 420.0, 18.0);

    let h = child(
        "EDIT",
        &per_app_text(&c),
        WS_BORDER.0 | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | WS_VSCROLL.0,
        w,
        196.0,
        432.0,
        664.0,
        74.0,
        ID_PERAPP,
    );
    w.e_perapp = Some(h);
    w.ctrls.push(h);

    // 上次 config.toml 读坏过（核心会用默认值起来并备份 .broken）：在页面上说清楚，
    // 否则用户只会觉得「我的设置全没了」（mac 的通用页也是同样处理，I-131）。
    if let Some(warn) = c["load_warning"].as_str().filter(|s| !s.is_empty()) {
        // 页面底部只有一行位置：首行（「config.toml 解析失败（已用默认值）」）足够让用户明白
        // 发生了什么，完整报错（含行列号、备份路径）在「诊断」页和托盘提示里。
        let first = warn.lines().next().unwrap_or(warn).trim();
        let brief: String = if first.chars().count() > 90 {
            format!("{}…", first.chars().take(90).collect::<String>())
        } else {
            format!("{first}　完整原因看「诊断」页")
        };
        section(w, "上次加载配置时出过问题", 524.0);
        label(w, &brief, 196.0, 548.0, 664.0, 20.0);
    }
    // 不再单独给「保存覆盖」一个按钮：同一页两个保存键等于让用户猜「我刚改的到底存没存」。
    // 顶部的「保存」会把这一页（含按应用覆盖）一起写。
}

fn per_app_text(c: &serde_json::Value) -> String {
    let mut lines = Vec::new();
    if let Some(map) = c["insert_per_app"].as_object() {
        for (k, v) in map {
            let mode = if v.as_str() == Some("keystroke") { "打字" } else { "粘贴" };
            lines.push(format!("{}={}", k, mode));
        }
    }
    lines.sort();
    lines.join("\r\n")
}

fn save_hotkey_preset(spec: &str) {
    if let Some(h) = with_win(|w| w.e_hotkey).flatten() {
        unsafe {
            crate::keycap::set(h, spec);
        }
    }
    apply(serde_json::json!({"hotkey": spec}).to_string());
    crate::report(&format!("HOTKEY PRESET {}", spec));
}

/// 切换设置页（鼠标点导航和键盘都走这里，避免两条路径行为不一致）
fn switch_page(p: usize) {
    let p = p % PAGES.len();
    let leave_mic = {
        let mut g = crate::lock(&WIN);
        match g.as_mut() {
            Some(w) if w.page != p => {
                let leave = w.page == 2 && w.meter_on;
                if leave {
                    w.meter_on = false;
                }
                w.page = p;
                leave
            }
            _ => return,
        }
    };
    if leave_mic {
        if let Some(a) = crate::lock(&crate::APP).as_ref() {
            a.core.meter_stop();
        }
    }
    if p == 3 {
        refresh_suggestions();
    }
    crate::report(&format!("PAGE {p}"));
    if let Some(h) = hwnd() {
        build(h);
        unsafe {
            InvalidateRect(h, None, true);
        }
    }
}

/// 当前页（自测用）
pub fn current_page() -> usize {
    with_win(|w| w.page).unwrap_or(0)
}

/// 设置窗口句柄（未打开时 None）；供消息泵与自测用
pub fn hwnd() -> Option<HWND> {
    WIN.lock().ok().and_then(|g| g.as_ref().map(|w| w.hwnd))
}

/// 自测用：把焦点放到某个控件上并重画一帧（同线程 SetFocus 才生效）
pub fn focus_demo(id: usize) {
    let hwnd = {
        let w = crate::lock(&WIN);
        w.as_ref().map(|w| w.hwnd)
    };
    let Some(hwnd) = hwnd else { return };
    unsafe {
        if let Ok(c) = GetDlgItem(hwnd, id as i32) {
            let _ = SetFocus(c);
            let _ = InvalidateRect(c, None, true);
            let _ = UpdateWindow(c);
            let _ = UpdateWindow(hwnd);
        }
    }
}

pub(crate) fn set_config_quiet(v: serde_json::Value) {
    let json = v.to_string();
    let r = {
        let app = crate::lock(&crate::APP);
        app.as_ref().map(|a| a.core.set_config(&json))
    };
    if let Some((c, e)) = r {
        if c != 0 {
            crate::message_box(&format!("保存失败（{c}）：{e}"));
        }
    }
}

/// 录 8 秒声纹：放到后台线程，完成后 PostMessage 回设置页重建
fn enroll_voice_async() {
    crate::report("VOICE 开始登记（8 秒）");
    crate::tray::set_tip("字在：正在录声纹，请说话…");
    let ptr = {
        let app = crate::lock(&crate::APP);
        app.as_ref().map(|a| a.core.raw_handle())
    };
    let Some(ptr) = ptr else { return };
    /// core 句柄要跨线程用：转成 usize 传（裸指针不是 Send；它本身是进程内 FFI 句柄，
    /// 生命周期跟 App 一样长，线程里再转回去用是安全的）
    let raw = ptr as usize;
    std::thread::spawn(move || {
        let (rc, err) = unsafe { crate::core::enroll_mic_raw(raw as *mut std::ffi::c_void, 8) };
        crate::report(&format!("VOICE 登记结束 rc={rc} {err}"));
        crate::lock(&VOICE_RESULT).replace((rc, err));
        let hwnd = { crate::lock(&WIN).as_ref().map(|w| w.hwnd) };
        if let Some(h) = hwnd {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    h,
                    WM_VOICE_DONE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    });
}

/// 润色连通性测试：后台线程跑（同步 HTTP），完成后 PostMessage 回设置页重建
fn polish_test_async() {
    crate::report("POLISH 测试连接开始");
    let ptr = {
        let app = crate::lock(&crate::APP);
        app.as_ref().map(|a| a.core.raw_handle() as usize)
    };
    let Some(raw) = ptr else { return };
    std::thread::spawn(move || {
        let (text, err) = unsafe { crate::core::polish_test_raw(raw as *mut std::ffi::c_void) };
        let msg = if !err.is_empty() {
            format!("连不上：{err}")
        } else {
            let t: String = text.chars().take(40).collect();
            format!("通了，返回：{t}")
        };
        crate::report(&format!("POLISH {msg}"));
        crate::lock(&POLISH_RESULT).clear();
        crate::lock(&POLISH_RESULT).push_str(&msg);
        let hwnd = { crate::lock(&WIN).as_ref().map(|w| w.hwnd) };
        if let Some(h) = hwnd {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    h,
                    WM_POLISH_DONE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    });
}

/// 自测：走真实的「按应用覆盖」保存路径（进程内点按钮等价，避开 UIPI）
pub fn self_test_per_app(sample: &str) -> String {
    open_at(0);
    // 主线程必须泵消息，否则窗口/子控件是在消息循环里才建出来的
    for _ in 0..20 {
        unsafe {
            let mut msg: windows::Win32::UI::WindowsAndMessaging::MSG = std::mem::zeroed();
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
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    let dbg = {
        let g = crate::lock(&WIN);
        match g.as_ref() {
            Some(w) => format!("page={} ctrls={} e_perapp={:?}", w.page, w.ctrls.len(), w.e_perapp),
            None => "no window".to_string(),
        }
    };
    let Some(h) = with_win(|w| w.e_perapp).flatten() else {
        return format!("PERAPP FAIL 输入框没起来（{dbg}）");
    };
    unsafe {
        SetWindowTextW(h, PCWSTR(wide(sample).as_ptr()));
    }
    save_per_app();
    let json = {
        let app = crate::lock(&crate::APP);
        app.as_ref().map(|a| a.core.config_json()).unwrap_or_default()
    };
    let v: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
    format!("PERAPP OK insert_per_app={}", v["insert_per_app"])
}

/// 保存「按应用覆盖」：从编辑框读文本再落盘（按钮路径用）
fn save_per_app() {
    let Some(h) = with_win(|w| w.e_perapp).flatten() else { return };
    let text = text_of(h);
    save_per_app_text(&text);
}

/// 只落盘：把行文本解析成 {进程名: paste|keystroke}
fn save_per_app_text(text: &str) {
    let mut map = serde_json::Map::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let k = k.trim().to_ascii_lowercase();
        if k.is_empty() {
            continue;
        }
        let v = v.trim();
        let mode = if v == "打字" || v.eq_ignore_ascii_case("keystroke") {
            "keystroke"
        } else {
            "paste"
        };
        map.insert(k, serde_json::Value::String(mode.to_string()));
    }
    let json = serde_json::json!({ "insert_per_app": map }).to_string();
    let r = {
        let app = crate::lock(&crate::APP);
        app.as_ref().map(|a| a.core.set_config(&json))
    };
    match r {
        Some((0, _)) => {
            crate::report(&format!("CFG insert_per_app={json}"));
            crate::tray::set_tip("按应用覆盖已保存");
        }
        Some((c, e)) => crate::message_box(&format!("保存失败（{c}）：{e}")),
        None => {}
    }
}

fn save_text_field(id: i32, key: &str, ok_msg: &str) {
    let h = with_win(|w| if id == 401 { w.e_proxy } else { w.e_mirror }).flatten();
    let Some(h) = h else { return };
    let v = text_of(h);
    let json = serde_json::json!({ key: v.trim() }).to_string();
    let r = {
        let app = crate::lock(&crate::APP);
        app.as_ref().map(|a| a.core.set_config(&json))
    };
    match r {
        Some((0, _)) => {
            crate::report(&format!("CFG {key}={} {ok_msg}", v.trim()));
            crate::tray::set_tip(ok_msg);
        }
        Some((c, e)) => crate::message_box(&format!("保存失败（{c}）：{e}")),
        None => {}
    }
}


/// 引擎页当前选中的是不是「附加模型」（标点/降噪/声纹）——那些只能下载，不能当识别引擎
fn engine_selected_is_aux() -> bool {
    // 列表现在只列识别引擎；附加模型改成用到时自动下载，所以这里恒为 false，
    // 保留函数是为了 ID_USE 分支的调用点不用改。
    false
}

fn page_engine(w: &mut Window) {
    let models: serde_json::Value = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => serde_json::from_str(&a.core.models_json()).unwrap_or(serde_json::Value::Null),
            None => serde_json::Value::Null,
        }
    };
    section(w, "语音模型（二选一）", 24.0);
    label(
        w,
        "默认的 SenseVoice 随安装包一起装好，装完就能用；想要更准再换 Qwen3（大得多，下载慢）。",
        196.0,
        50.0,
        664.0,
        18.0,
    );
    let list = listbox(w, ID_LIST, 196.0, 76.0, 664.0, 160.0);
    w.lst = Some(list);
    w.model_ids.clear();
    if let Some(items) = models.as_array() {
        let mut shown = 0usize;
        for m in items.iter() {
            // 只列「语音识别引擎」：Stub 是自测用的，标点/降噪/声纹模型改成用到时自动下载，
            // 不再摆在列表里让用户以为要装一堆东西（用户反馈 2026-11-21）。
            let is_engine = m["engine"].as_bool().unwrap_or(false);
            let id = m["id"].as_str().unwrap_or("");
            if !is_engine || id == "stub" {
                continue;
            }
            let name = m["name"].as_str().unwrap_or("");
            let active = m["active"].as_bool().unwrap_or(false);
            let installed = m["installed"].as_bool().unwrap_or(false);
            let mb = m["approx_mb"].as_i64().unwrap_or(0);
            let state = if active && installed {
                "使用中".to_string()
            } else if active {
                format!("已选中 · 未安装（{} MB）", mb)
            } else if installed {
                "已安装".to_string()
            } else {
                format!("未安装（{} MB，点下载）", mb)
            };
            let line = format!("{name}   —   {state}");
            unsafe {
                SendMessageW(list, LB_ADDSTRING, WPARAM(0), LPARAM(wide(&line).as_ptr() as isize));
            }
            w.model_ids.push(id.to_string());
            if active {
                unsafe {
                    SendMessageW(list, LB_SETCURSEL, WPARAM(shown), LPARAM(0));
                }
            }
            shown += 1;
        }
    }
    button(w, "使用", ID_USE, 196.0, 238.0, 96.0);
    button(w, "下载", ID_PULL, 300.0, 238.0, 96.0);
    // 标点/降噪/声纹不占列表：用到时自动下载。这句说明挪到 ⓘ 里，页面少一行灰字
    info(
        w,
        264.0,
        22.0,
        "标点、降噪、声纹模型不在这里：打开对应开关时会自动下载，不用手动装。",
    );

    section(w, "从哪下载", 286.0);
    // 地址不再要求用户理解：默认走内置镜像，只留一条能点的来源链接（R-130）
    label(
        w,
        "模型默认从这台机器的镜像装好；想自己下就点右边，把文件放进模型目录对应文件夹即可。",
        196.0,
        316.0,
        470.0,
        18.0,
    );
    button(w, "打开模型来源", ID_MIRROR_OPEN, 676.0, 312.0, 184.0);
    label(w, "下载代理（内网用）", 196.0, 352.0, 200.0, 18.0);
    info(
        w,
        264.0,
        357.0,
        "留空 = 跟随系统代理；内网需要时填形如 http://127.0.0.1:7890（不支持 socks）。",
    );
    let c = cfg();
    w.e_proxy = Some(edit(w, c["model_proxy"].as_str().unwrap_or(""), 401, 456.0, 348.0, 300.0));
    button(w, "保存", ID_ENGINE_SAVE, 770.0, 347.0, 90.0);

    // 下载进度（对齐 mac 的下载进度条文案）：没有下载时显示待机
    let l = child(
        "STATIC",
        "下载进度：待机（点「下载」开始）",
        WS_CHILD.0,
        w,
        196.0,
        400.0,
        664.0,
        18.0,
        ID_PULL_LABEL,
    );
    w.ctrls.push(l);
    w.pull_lbl = Some(l);
}

fn page_audio(w: &mut Window) {
    let dev_info: serde_json::Value = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => serde_json::from_str(&a.core.devices_json()).unwrap_or(serde_json::Value::Null),
            None => serde_json::Value::Null,
        }
    };
    let c = cfg();
    let selected = c["device"].as_str().unwrap_or("").to_string();
    let denoise = c["denoise"].as_bool().unwrap_or(true);
    section(w, "麦克风", 24.0);
    let list = listbox(w, ID_LIST, 196.0, 56.0, 664.0, 138.0);
    w.lst = Some(list);
    let mut sel = 0usize;
    if let Some(items) = dev_info["devices"].as_array() {
        for (i, d) in items.iter().enumerate() {
            let name = d["name"].as_str().unwrap_or("");
            let is_default = d["default"].as_bool().unwrap_or(false);
            let line = if is_default { format!("{name}　（系统默认）") } else { name.to_string() };
            unsafe {
                SendMessageW(list, LB_ADDSTRING, WPARAM(0), LPARAM(wide(&line).as_ptr() as isize));
            }
            if !selected.is_empty() && selected == name {
                sel = i;
            }
        }
    }
    let device_count = dev_info["devices"].as_array().map(|a| a.len()).unwrap_or(0);
    if device_count == 0 {
        unsafe {
            SendMessageW(
                list,
                LB_ADDSTRING,
                WPARAM(0),
                LPARAM(wide("（现在没有可用的录音设备——虚拟机里常见，真机请插麦）").as_ptr() as isize),
            );
        }
    }
    unsafe {
        SendMessageW(list, LB_SETCURSEL, WPARAM(sel), LPARAM(0));
    }
    button(w, "使用选中设备", ID_USE, 196.0, 206.0, 130.0);
    // 模型没下载时明说一句：否则打开开关只会「没降噪」，用户看不出来为什么
    let denoise_ready = c["denoise_ready"].as_bool().unwrap_or(false);
    let denoise_text = if denoise_ready { "背景降噪" } else { "背景降噪（模型未下载）" };
    w.c_denoise = Some(checkbox(w, denoise_text, ID_DENOISE, denoise, 196.0, 246.0, 560.0));
    info(
        w,
        772.0,
        244.0,
        if denoise_ready {
            "只在 48kHz 麦克风上生效：压掉稳定的底噪（风扇、空调）。"
        } else {
            "模型还没下载：去「引擎」页选中「降噪模型」点下载（1MB）。"
        },
    );
    let pause_media = c["pause_media"].as_bool().unwrap_or(false);
    w.c_pause_media = Some(checkbox(
        w,
        "录音时暂停系统播放",
        ID_PAUSE_MEDIA,
        pause_media,
        196.0,
        280.0,
        560.0,
    ));
    info(w, 772.0, 278.0, "只暂停本来就在放的声音，松手后自动恢复；看视频时录音不会把它录进去。");
    let punct = c["punct"].as_bool().unwrap_or(true);
    let punct_ready = c["punct_ready"].as_bool().unwrap_or(false);
    let punct_text = "句末补标点";
    w.c_punct = Some(checkbox(
        w,
        punct_text,
        ID_PUNCT,
        punct,
        196.0,
        314.0,
        560.0,
    ));
    info(w, 772.0, 312.0, if punct_ready {
        "说一句话自动补上句末标点，别的标点不动。"
    } else {
        "模型还没下载：去「引擎」页选中「标点模型」点下载（约 65MB）。"
    });

    let source_lock = c["source_lock"].as_bool().unwrap_or(false);
    info(w, 772.0, 346.0, "只认离麦克风最近的声音：外放视频时少录进扬声器，但两个人同时说话会挑一个。");
    w.c_source_lock = Some(checkbox(
        w,
        "音源锁定",
        ID_SOURCE_LOCK,
        source_lock,
        196.0,
        348.0,
        560.0,
    ));
    button(w, "保存", ID_SAVE, 196.0, 384.0, 96.0);

    // 输入电平：和 macOS 设置页的「输入电平」对齐——只看音量，不录音不落盘。
    section(w, "输入电平", 420.0);
    let lvl = child(
        "STATIC",
        if w.meter_on { "░░░░░░░░░░░░░░░░░░░░  听着…" } else { "░░░░░░░░░░░░░░░░░░░░  未开始" },
        0,
        w,
        196.0,
        452.0,
        400.0,
        22.0,
        ID_MIC_LABEL,
    );
    w.meter_lbl = Some(lvl);
    button(
        w,
        if w.meter_on { "停止测试" } else { "开始测试" },
        ID_MIC_TEST,
        610.0,
        448.0,
        110.0,
    );
    label(
        w,
        if w.meter_on { "正听着：说句话看看有没有起伏" } else { "点「开始测试」看有没有声音" },
        196.0,
        486.0,
        420.0,
        18.0,
    );
    
}


fn page_hotwords(w: &mut Window) {
    let json = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.hotwords_json(),
            None => String::new(),
        }
    };
    let mut lines = String::new();
    section(w, "热词表", 24.0);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) {
        if let Some(rules) = v["rules"].as_array() {
            for r in rules {
                let from = r["from"].as_str().unwrap_or("");
                let to = r["to"].as_str().unwrap_or("");
                // 单列：只显示「想听到的词」；错误写法由程序从历史里学（R-133）
                if !to.is_empty() || !from.is_empty() {
                    let word = if to.is_empty() { from } else { to };
                    lines.push_str(&format!("{word}\r\n"));
                }
            }
        }
        let count = v["rules"].as_array().map(|a| a.len()).unwrap_or(0);
        let err = v["error"].as_str().unwrap_or("");
        label(
            w,
            &if err.is_empty() {
                format!("共 {count} 条")
            } else {
                // 文件格式坏了：规则照用，但一定要让人看见（否则「共 0 条」像什么都没发生）
                format!("⚠ 共 {count} 条 · {err}")
            },
            196.0,
            52.0,
            664.0,
            18.0,
        );
    } else {
        }
    let h = child(
        "EDIT",
        &lines,
        WS_BORDER.0 | WS_VSCROLL.0 | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32,
        w,
        196.0,
        78.0,
        664.0,
        196.0,
        ID_HW_EDIT,
    );
    w.ctrls.push(h);
    w.e_hotwords = Some(h);
    button(w, "保存热词", ID_SAVE_HW, 196.0, 288.0, 110.0);

    // 建议：从历史里挑高频词，点一下加进热词表（对齐 mac 的热词建议）
    section(w, "建议（从历史里挑的高频词）", 322.0);
    // 注意：build() 期间持有 WIN 锁，调用 core 的阻塞式 FFI 会与事件回调抢锁，
    // 之前直接在页面里算建议会导致进程崩溃（STATUS_FATAL_USER_CALLBACK_EXCEPTION）。
    // 现在建议在页面打开/点刷新时算好放进 SUGGESTIONS，页面只读缓存。
    let sug: Vec<(String, usize)> = SUGGESTIONS.lock().map(|g| g.clone()).unwrap_or_default();
    if sug.is_empty() {
        label(
            w,
            "还没有足够的历史可参考：多用几次，或者点「刷新建议」。",
            196.0,
            346.0,
            664.0,
            18.0,
        );
    } else {
        // 用 STATIC 展示建议（不再用 LISTBOX：在 build 里建带条目的列表会在 VM 上
        // 触发 STATUS_FATAL_USER_CALLBACK_EXCEPTION，见 I-109）
        let text: Vec<String> = sug.iter().take(3).map(|(word, n)| format!("{word}（{n} 次）")).collect();
        label(w, &text.join("   ·   "), 196.0, 342.0, 664.0, 20.0);
    }
    button(w, "加进热词", ID_HW_ADD, 196.0, 388.0, 120.0);
    button(w, "刷新建议", ID_HW_REFRESH, 330.0, 388.0, 100.0);

    // 学到的改法：上屏后用户改了词，这里给一条候选（对齐 mac EditObserver）
    let learned = crate::observer::load();
    section(w, "学到的改法", 430.0);
    if let Some(s) = learned.first() {
        label(
            w,
            &format!("你刚才把「{}」改成了「{}」。", s.from, s.to),
            196.0,
            452.0,
            664.0,
            18.0,
        );
        button(w, "采纳", ID_OBS_ADOPT, 196.0, 476.0, 84.0);
        button(w, "忽略", ID_OBS_IGNORE, 292.0, 476.0, 84.0);
        label(w, &format!("还有 {} 条", learned.len().saturating_sub(1)), 392.0, 482.0, 160.0, 18.0);
    } else {
        label(
            w,
            "还没学到。",
            196.0,
            452.0,
            664.0,
            18.0,
        );
    }
}

/// 页面打开/点「刷新建议」时算好的建议词缓存（build 里不碰 core，避免锁顺序问题）
static SUGGESTIONS: std::sync::Mutex<Vec<(String, usize)>> = std::sync::Mutex::new(Vec::new());

pub fn refresh_suggestions() {
    let sug = suggested_words();
    if let Ok(mut g) = SUGGESTIONS.lock() {
        *g = sug;
    }
}

/// 从历史文本里挑高频词：2–5 个汉字/字母数字组成的片段，出现 ≥2 次、且还不在热词表里
fn suggested_words() -> Vec<(String, usize)> {
    let (hist, hw) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => (a.core.history_json(200), a.core.hotwords_json()),
            None => return Vec::new(),
        }
    };
    let mut texts: Vec<String> = Vec::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&hist) {
        if let Some(list) = v.as_array() {
            for it in list {
                if let Some(t) = it["text"].as_str() {
                    texts.push(t.to_string());
                }
            }
        }
    }
    let mut existing: Vec<String> = Vec::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&hw) {
        if let Some(rules) = v["rules"].as_array() {
            for r in rules {
                for k in ["from", "to"] {
                    if let Some(x) = r[k].as_str() {
                        existing.push(x.to_string());
                    }
                }
            }
        }
    }
    suggest_from(&texts, &existing)
}

/// 纯逻辑：给一批历史文本和已有热词，算出建议词（测试用，也方便以后换算法）
fn suggest_from(texts: &[String], existing: &[String]) -> Vec<(String, usize)> {
    use std::collections::{HashMap, HashSet};
    // 只靠 n-gram 计数会把「们开始」「天天气不错」这类碎片也当成高频词——
    // 它们在唯一那句样例里上下文固定。多一个边界熵过滤：一个词左右两侧
    // 至少各出现过两种不同的字，才像是独立词而不是从长句里切出来的碎片。
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut lefts: HashMap<String, HashSet<char>> = HashMap::new();
    let mut rights: HashMap<String, HashSet<char>> = HashMap::new();
    for t in texts {
        let chars: Vec<char> = t
            .chars()
            .filter(|c| c.is_alphanumeric() && !c.is_ascii())
            .collect();
        for n in 2..=6usize {
            if chars.len() < n {
                break;
            }
            for (i, w) in chars.windows(n).enumerate() {
                let word: String = w.iter().collect();
                *counts.entry(word.clone()).or_insert(0) += 1;
                // 句首/句尾不算「邻居」：否则「们开始」这种碎片靠一个句首就能骗过过滤
                if i > 0 {
                    lefts.entry(word.clone()).or_default().insert(chars[i - 1]);
                }
                if i + n < chars.len() {
                    rights.entry(word).or_default().insert(chars[i + n]);
                }
            }
        }
    }
    // 两字的功能词/口头语不当热词建议（「这个」「是一」「怎么样」…）
    const STOP2: [&str; 18] = [
        "这个", "那个", "是一", "就是", "可以", "然后", "所以", "但是", "什么", "怎么",
        "现在", "我们", "你们", "他们", "一个", "一下", "还是", "因为",
    ];
    let mut out: Vec<(String, usize)> = counts
        .into_iter()
        .filter(|(w, n)| {
            let lb = lefts.get(w).map_or(0, |s| s.len());
            let rb = rights.get(w).map_or(0, |s| s.len());
            // 左右加起来至少 3 种不同的邻字，才像是词；固定搭配的长句碎片过不了
            *n >= 2
                && !existing.iter().any(|e| e == w)
                && lb + rb >= 3
                && !(w.chars().count() <= 2 && STOP2.contains(&w.as_str()))
        })
        .collect();
    // 次数优先，次数相同取更长的词（更可能是专有名词）
    out.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.chars().count().cmp(&a.0.chars().count())));
    // 去掉被更长建议词包含的碎片（「设计文档」在列表里时，就不必再列「计文档」「文档」）
    let mut kept: Vec<(String, usize)> = Vec::new();
    for (w, n) in out {
        if kept.iter().any(|(k, _)| k.contains(&w)) {
            continue;
        }
        kept.push((w, n));
        if kept.len() >= 6 {
            break;
        }
    }
    kept
}

/// 把建议列表里选中的词加进热词表（from=to=词，等价于「这个词要按原样出现」）
fn add_suggested_hotword() {
    // 建议改成第一行文字展示后，这里直接取排第一的词（原先是从 LISTBOX 读选中项）
    let picks = SUGGESTIONS
        .lock()
        .ok()
        .and_then(|g| g.first().map(|(w, _)| w.clone()));
    let Some(word) = picks else {
        crate::message_box("还没有建议词：先多用几次，或点「刷新建议」。");
        return;
    };
    if word.is_empty() {
        return;
    }
    let json = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.hotwords_json(),
            None => return,
        }
    };
    let mut rules: Vec<serde_json::Value> = serde_json::from_str::<serde_json::Value>(&json)
        .ok()
        .and_then(|v| v["rules"].as_array().cloned())
        .unwrap_or_default();
    if rules.iter().any(|r| r["from"].as_str() == Some(word.as_str())) {
        crate::message_box("这个词已经在热词表里了。");
        return;
    }
    rules.push(serde_json::json!({ "from": word, "to": word }));
    let payload = serde_json::json!({ "rules": rules }).to_string();
    let (code, err) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.set_hotwords(&payload),
            None => (-1, "core 没起来".to_string()),
        }
    };
    if code != 0 {
        crate::message_box(&format!("加进热词失败：{err}"));
    } else {
        crate::tray::set_tip("字在：已加进热词");
        rebuild_page();
    }
}


/// 采纳学到的改法：把「原词 → 新词」写进热词表，并删掉这条候选
fn adopt_learned() {
    let Some(s) = crate::observer::load().into_iter().next() else {
        return;
    };
    let json = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.hotwords_json(),
            None => return,
        }
    };
    let mut rules: Vec<serde_json::Value> = serde_json::from_str::<serde_json::Value>(&json)
        .ok()
        .and_then(|v| v["rules"].as_array().cloned())
        .unwrap_or_default();
    if !rules
        .iter()
        .any(|r| r["from"].as_str() == Some(s.from.as_str()) && r["to"].as_str() == Some(s.to.as_str()))
    {
        rules.push(serde_json::json!({ "from": s.from, "to": s.to }));
    }
    let payload = serde_json::json!({ "rules": rules }).to_string();
    let (code, err) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.set_hotwords(&payload),
            None => (-1, "core 没起来".to_string()),
        }
    };
    if code != 0 {
        crate::message_box(&format!("加进热词失败：{err}"));
        return;
    }
    crate::observer::remove(&s);
    crate::tray::set_tip(&format!("字在：已学会「{} → {}」", s.from, s.to));
    rebuild_page();
}

fn ignore_learned() {
    if let Some(s) = crate::observer::load().into_iter().next() {
        crate::observer::remove(&s);
    }
    rebuild_page();
}

fn save_hotwords() {
    let text = with_win(|w| w.e_hotwords.map(text_of)).flatten().unwrap_or_default();
    // 已学到的「错写 -> 正确写法」映射，按正确写法建索引：单列编辑不能把它抹掉
    let mut learned: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    {
        let json = {
            let app = crate::lock(&crate::APP);
            app.as_ref().map(|a| a.core.hotwords_json()).unwrap_or_default()
        };
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) {
            for r in v["rules"].as_array().into_iter().flatten() {
                let from = r["from"].as_str().unwrap_or("").to_string();
                let to = r["to"].as_str().unwrap_or("").to_string();
                if !to.is_empty() && from != to {
                    learned.insert(to, from);
                }
            }
        }
    }
    let mut rules = Vec::new();
    for line in text.lines() {
        let word = line.trim();
        if word.is_empty() || word.starts_with('#') {
            continue;
        }
        let from = learned.get(word).cloned().unwrap_or_else(|| word.to_string());
        rules.push(serde_json::json!({ "from": from, "to": word }));
    }
    let json = serde_json::json!({ "rules": rules }).to_string();
    let (code, err) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.set_hotwords(&json),
            None => (-1, "core 没起来".to_string()),
        }
    };
    if code != 0 {
        crate::message_box(&format!("热词保存失败：{err}"));
    } else {
        crate::tray::set_tip("字在：热词已保存");
        rebuild_page();
    }
}

/// 自绘开关的当前状态（id -> 开/关）。
/// 为什么不用 BM_SETCHECK/BM_GETCHECK：owner-draw 按钮上这两个消息不可靠，
/// 实测点了不翻转、配置是「开」也画成「关」。
static TOGGLES: std::sync::Mutex<Vec<(usize, bool)>> = std::sync::Mutex::new(Vec::new());

fn toggle_on(id: usize) -> bool {
    TOGGLES
        .lock()
        .ok()
        .and_then(|g| g.iter().find(|(i, _)| *i == id).map(|(_, v)| *v))
        .unwrap_or(false)
}

fn set_toggle(id: usize, on: bool) {
    if let Ok(mut g) = TOGGLES.lock() {
        if let Some(slot) = g.iter_mut().find(|(i, _)| *i == id) {
            slot.1 = on;
        } else {
            g.push((id, on));
        }
    }
}

fn flip_toggle(id: usize) -> bool {
    let next = !toggle_on(id);
    set_toggle(id, next);
    next
}

/// 最近一条历史的全文（复制用）
static HIST_TEXTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
/// 与列表一一对应的历史 id（删除用）
static HIST_IDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn page_history(w: &mut Window) {
    let (stats, hist) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => (a.core.stats_json(), a.core.history_json(50)),
            None => (String::new(), String::new()),
        }
    };
    let sv: serde_json::Value = serde_json::from_str(&stats).unwrap_or(serde_json::Value::Null);
    let sessions = sv["sessions"].as_u64().unwrap_or(0);
    let chars = sv["chars"].as_u64().unwrap_or(0);
    let p50 = sv["latency"]["p50"].as_u64().unwrap_or(0);
    let p95 = sv["latency"]["p95"].as_u64().unwrap_or(0);
    let cn = sv["latency"]["count"].as_u64().unwrap_or(0);
    let hist_on = sv["history_enabled"].as_bool().unwrap_or(true);
    let failed_n = crate::APP
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|a| a.core.failed_segments().len()))
        .unwrap_or(0);
    section(w, "统计与历史", 24.0);
    if cn > 0 {
        label(
            w,
            &format!("累计 {sessions} 次 · {chars} 字 · 松手→出字 p50 {p50}ms / p95 {p95}ms（{cn} 个样本）"),
            196.0,
            52.0,
            664.0,
            18.0,
        );
    } else {
        label(w, &format!("累计 {sessions} 次 · {chars} 字 · 还没有延迟样本"), 196.0, 52.0, 664.0, 18.0);
    }
    let list = listbox(w, ID_HIST_LIST, 196.0, 82.0, 664.0, 288.0);
    w.lst2 = Some(list);
    let mut texts = Vec::new();
    let mut ids = Vec::new();
    if let Some(items) = serde_json::from_str::<serde_json::Value>(&hist)
        .ok()
        .and_then(|v| v.as_array().cloned())
    {
        for it in items {
            let at = it["at"].as_str().unwrap_or("");
            let when: String = at.chars().skip(5).take(11).collect::<String>().replace('T', " ");
            let status = match it["status"].as_str().unwrap_or("") {
                "inserted" => "已上屏",
                "pending" => "待复制",
                "failed" => "失败",
                other => other,
            };
            let text = it["text"].as_str().unwrap_or("").replace('\r', " ");
            let preview: String = text.chars().take(36).collect();
            let n = it["chars"].as_u64().unwrap_or(0);
            // 与 mac 一致：带上「来源应用」，方便回过头找哪句是哪个程序里说的
            let app = it["app"].as_str().unwrap_or("");
            let app = if app.is_empty() { "未知应用" } else { app };
            let line = format!("{when}  {app}  {n:>3}字  {status}  {preview}");
            unsafe {
                SendMessageW(list, LB_ADDSTRING, WPARAM(0), LPARAM(wide(&line).as_ptr() as isize));
            }
            texts.push(text);
            ids.push(it["id"].as_str().unwrap_or("").to_string());
        }
    }
    if let Ok(mut g) = HIST_TEXTS.lock() {
        *g = texts;
    }
    if let Ok(mut g) = HIST_IDS.lock() {
        *g = ids;
    }
    w.c_hist_on = Some(checkbox(w, "记录历史（关掉后不再新增）", ID_HIST_EN, hist_on, 196.0, 386.0, 360.0));
    let retry_label = if failed_n > 0 {
        format!("重试失败（{failed_n}）")
    } else {
        "没有失败的段".to_string()
    };
    // 五个按钮压到四个：刷新没了（说完一句会自动刷，切换页面也会重建），
    // 「复制选中/删除选中」改短，和清空、重试排成一行。
    button(w, "复制", ID_HIST_COPY, 196.0, 418.0, 76.0);
    button(w, "删除", ID_HIST_DEL, 280.0, 418.0, 76.0);
    button(w, "清空", ID_HIST_CLEAR, 364.0, 418.0, 76.0);
    button(w, &retry_label, ID_HIST_RETRY, 448.0, 418.0, 160.0);
}


/// 服务商预设（与 mac `ProviderPreset.all` 同一份）
const POLISH_PRESETS: [(&str, &str, &str); 6] = [
    ("DeepSeek", "https://api.deepseek.com", "deepseek-chat"),
    ("OpenAI", "https://api.openai.com/v1", "gpt-4o-mini"),
    ("Kimi", "https://api.moonshot.cn/v1", "moonshot-v1-8k"),
    ("智谱", "https://open.bigmodel.cn/api/paas/v4", "glm-4-flash"),
    ("通义", "https://dashscope.aliyuncs.com/compatible-mode/v1", "qwen-plus"),
    ("硅基", "https://api.siliconflow.cn/v1", "Qwen/Qwen2.5-7B-Instruct"),
];

fn apply_polish_preset(i: usize) {
    let Some((_name, ep, m)) = POLISH_PRESETS.get(i) else { return };
    set_config_quiet(serde_json::json!({"polish_endpoint": ep, "polish_model": m}));
    crate::report(&format!("POLISH PRESET {i} {ep} {m}"));
    rebuild_page();
}

fn page_polish(w: &mut Window) {
    let (cfg_json, prof_json) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => (a.core.config_json(), a.core.profiles_json()),
            None => (String::new(), String::new()),
        }
    };
    let cv: serde_json::Value = serde_json::from_str(&cfg_json).unwrap_or(serde_json::Value::Null);
    let enabled = cv["polish_enabled"].as_bool().unwrap_or(false);
    let has_key = cv["polish_has_key"].as_bool().unwrap_or(false);
    let endpoint = cv["polish_endpoint"].as_str().unwrap_or("");
    let model = cv["polish_model"].as_str().unwrap_or("");

    section(w, "润色服务", 24.0);
    w.c_polish = Some(checkbox(
        w,
        "启用润色",
        ID_POLISH_EN,
        enabled,
        196.0,
        52.0,
        664.0,
    ));
    // 服务商预设：点一下把「服务地址 + 模型名」填好并立刻保存（对齐 mac ProviderPreset）
    label(w, "服务商", 196.0, 84.0, 56.0, 18.0);
    for (i, (name, _ep, _m)) in POLISH_PRESETS.iter().enumerate() {
        button(w, name, ID_POLISH_PRESET + i, 258.0 + i as f32 * 100.0, 80.0, 94.0);
    }
    label(w, "服务地址", 196.0, 116.0, 664.0, 18.0);
    w.e_polish_url = Some(edit(w, endpoint, ID_EDIT_BASE + 20, 196.0, 136.0, 420.0));
    label(w, "模型名", 196.0, 168.0, 664.0, 18.0);
    w.e_polish_model = Some(edit(w, model, ID_EDIT_BASE + 21, 196.0, 188.0, 260.0));
    label(w, "API Key", 196.0, 220.0, 664.0, 18.0);
    let kh = child(
        "EDIT",
        "",
        WS_BORDER.0 | ES_PASSWORD as u32 | ES_AUTOHSCROLL as u32,
        w,
        196.0,
        240.0,
        420.0,
        26.0,
        ID_EDIT_BASE + 22,
    );
    w.ctrls.push(kh);
    w.e_polish_key = Some(kh);
    button(w, "保存", ID_POLISH_SAVE, 196.0, 300.0, 90.0);
    button(w, "测试连接", ID_POLISH_TEST, 296.0, 300.0, 100.0);
    let pres = crate::lock(&POLISH_RESULT).clone();
    if !pres.is_empty() {
        label(w, &pres, 410.0, 304.0, 450.0, 18.0);
    } else if !has_key {
        label(w, "还没填 Key：填好点「保存」，再点「测试连接」", 410.0, 304.0, 450.0, 18.0);
    }

    label(w, "润色档", 196.0, 340.0, 62.0, 20.0);
    info(w, 266.0, 339.0, "每一档对应一套服务端指令：选中后可以先编辑，再保存；常用档位能在设置页里绑快捷键。");
    let plist = listbox(w, ID_POLISH_LIST, 196.0, 364.0, 664.0, 64.0);
    w.lst3 = Some(plist);
    let pv: serde_json::Value = serde_json::from_str(&prof_json).unwrap_or(serde_json::Value::Null);
    if let Some(list) = pv["profiles"].as_array() {
        for (i, p) in list.iter().enumerate() {
            let name = p["name"].as_str().unwrap_or("");
            let level = match p["level"].as_str().unwrap_or("") {
                "light" => "轻度",
                "medium" => "中度",
                "strong" => "重度",
                other => other,
            };
            let slot = p["slot"].as_str().unwrap_or("");
            let active = p["active"].as_bool().unwrap_or(false);
            let mark = if active { "✓ " } else { "   " };
            let line = if slot.is_empty() {
                format!("{mark}{name}   —   {level}")
            } else {
                format!("{mark}{name}   —   {level}   {slot}")
            };
            unsafe {
                SendMessageW(plist, LB_ADDSTRING, WPARAM(0), LPARAM(wide(&line).as_ptr() as isize));
            }
            if active {
                unsafe {
                    SendMessageW(plist, LB_SETCURSEL, WPARAM(i), LPARAM(0));
                }
            }
        }
    }
    button(w, "用选中的档", ID_POLISH_ACT, 196.0, 436.0, 110.0);
    label(w, "档位快捷键", 314.0, 440.0, 78.0, 18.0);
    let slot_now = pv["profiles"]
        .as_array()
        .and_then(|l| l.iter().find(|p| p["active"].as_bool().unwrap_or(false)))
        .and_then(|p| p["slot"].as_str())
        .unwrap_or("")
        .to_string();
    w.e_slot = Some(edit(w, &slot_now, ID_EDIT_BASE + 28, 396.0, 434.0, 190.0));


    // 档位指令：和 mac 的 TextEditor 一样，选中哪档改哪档，点保存才写盘
    let active_prompt = pv["profiles"]
        .as_array()
        .and_then(|l| l.iter().find(|p| p["active"].as_bool().unwrap_or(false)))
        .and_then(|p| p["prompt"].as_str())
        .unwrap_or("")
        .to_string();
    label(w, "指令（选中档）", 196.0, 462.0, 140.0, 18.0);
    let ph = child(
        "EDIT",
        &active_prompt,
        WS_BORDER.0 | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | WS_VSCROLL.0,
        w,
        196.0,
        482.0,
        664.0,
        48.0,
        ID_POLISH_PROMPT,
    );
    w.ctrls.push(ph);
    w.e_prompt = Some(ph);
    button(w, "保存指令", ID_POLISH_PROMPT_SAVE, 196.0, 536.0, 96.0);
    let pres = crate::lock(&POLISH_RESULT).clone();
    label(
        w,
        if pres.is_empty() { "" } else { &pres },
        306.0,
        540.0,
        554.0,
        18.0,
    );
}


/// 第一次把文字发到服务端前问一次（对齐 mac 的「这段文字会离开本机」弹窗）
fn polish_consent() -> bool {
    if crate::ASSUME_CONSENT.load(Ordering::Relaxed) {
        return true;
    }
    crate::message_box_yesno(
        "开启润色后，每段转写文本都会发到你配置的服务端（比如 DeepSeek）。\n\n语音和模型仍然只在本机，只有整理后的文字和原文会经过网络。可以随时关掉。\n\n我明白，开启润色",
    )
}

fn polish_models_async() {
    let ptr = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.raw_handle(),
            None => return,
        }
    };
    let raw = ptr as usize;
    std::thread::spawn(move || {
        let (list, err) = unsafe { crate::core::polish_models_raw(raw as *mut std::ffi::c_void) };
        let msg = if !list.is_empty() {
            format!("找到 {} 个可用模型", list.len())
        } else if !err.is_empty() {
            format!("没拿到：{err}")
        } else {
            "服务商没返回模型列表".to_string()
        };
        *crate::lock(&POLISH_MODELS) = list;
        *crate::lock(&POLISH_RESULT) = msg;
        let hwnd = { crate::lock(&WIN).as_ref().map(|w| w.hwnd) };
        if let Some(h) = hwnd {
            unsafe {
                let _ = PostMessageW(h, WM_POLISH_MODELS_DONE, WPARAM(0), LPARAM(0));
            }
        }
    });
}

fn save_polish() {
    let (enabled, url, model, key) = with_win(|w| {
        (
            w.c_polish.map(checked).unwrap_or(false),
            w.e_polish_url.map(text_of).unwrap_or_default(),
            w.e_polish_model.map(text_of).unwrap_or_default(),
            w.e_polish_key.map(text_of).unwrap_or_default(),
        )
    })
    .unwrap_or((false, String::new(), String::new(), String::new()));
    let mut v = serde_json::json!({
        "polish_enabled": enabled,
        "polish_endpoint": url.trim(),
        "polish_model": model.trim(),
    });
    // 打开润色但还没选档（level = off）时，默认用「轻度」：
    // 否则开关是开的、文字却一点没变，用户只会觉得「润色坏了」。
    if enabled {
        let lvl = {
            let app = crate::lock(&crate::APP);
            app.as_ref()
                .and_then(|a| serde_json::from_str::<serde_json::Value>(&a.core.config_json()).ok())
                .and_then(|c| c["polish_level"].as_str().map(|s| s.to_string()))
                .unwrap_or_default()
        };
        if lvl == "off" || lvl.is_empty() {
            v["polish_level"] = serde_json::Value::String("light".to_string());
        }
    }
    if !key.trim().is_empty() {
        v["polish_api_key"] = serde_json::Value::String(key.trim().to_string());
    }
    let (code, err) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.set_config(&v.to_string()),
            None => (-1, "core 没起来".to_string()),
        }
    };
    if code != 0 {
        crate::message_box(&format!("润色设置保存失败：{err}"));
    } else {
        crate::tray::set_tip("字在：润色设置已保存");
        rebuild_page();
    }
}

/// 局域网听写（对齐 mac 的「局域网接口」页）：别的设备把 WAV POST 过来，本机识别后回文字
/// 本机在局域网里的 IPv4：用一个不会真发包的 UDP connect 让系统选出出口地址。
fn local_ipv4() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    let ip = s.local_addr().ok()?.ip();
    if ip.is_loopback() {
        return None;
    }
    Some(ip.to_string())
}

/// 生成 32 位十六进制（不用额外依赖：RandomState 由系统熵播种）
fn rand_hex(n: usize) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    while out.len() < n {
        let st = std::collections::hash_map::RandomState::new();
        let mut h = st.build_hasher();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        h.write_u128(now);
        h.write_u32(std::process::id());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out.truncate(n);
    out
}

fn save_lan() {
    let port: u32 = {
        let w = crate::lock(&WIN);
        w.as_ref()
            .and_then(|w| w.e_lan_port)
            .map(|h| {
                let s = text_of(h);
                s.trim().parse::<u32>().unwrap_or(8765)
            })
            .unwrap_or(8765)
    };
    let token = {
        let w = crate::lock(&WIN);
        w.as_ref()
            .and_then(|w| w.e_lan_token)
            .map(text_of)
            .unwrap_or_default()
    };
    let enabled = toggle_on(ID_LAN_EN);
    let allow = toggle_on(ID_LAN_ALLOW);
    let has_token = {
        let w = crate::lock(&WIN);
        w.as_ref().map(|w| w.lan_has_token).unwrap_or(false)
    };
    let token_edit = token.trim().to_string();
    if allow && token_edit.is_empty() && !has_token {
        crate::report("LAN 拒绝保存：allow_lan 打开但令牌为空（等同把识别接口暴露给整个局域网）");
        crate::message_box("允许其它设备连进来时必须填一个令牌，否则等于把识别接口敞给整个局域网。");
        return;
    }
    let mut obj = serde_json::json!({
        "lan_enabled": enabled,
        "lan_port": port,
        "lan_allow_lan": allow,
    });
    // 令牌框留空 = 不改已有令牌（mac 的 SecureField 也是这个语义）
    if !token_edit.is_empty() || !has_token {
        obj["lan_token"] = serde_json::json!(token_edit);
    }
    let v = obj;
    set_config_quiet(v);
    crate::tray::set_tip(&format!("字在 · 局域网接口：{}", if enabled { "已开启" } else { "已关闭" }));
    rebuild_page();
}

fn page_lan(w: &mut Window) {
    let c = cfg();
    let enabled = c["lan_enabled"].as_bool().unwrap_or(false);
    let allow = c["lan_allow_lan"].as_bool().unwrap_or(false);
    let port = c["lan_port"].as_u64().unwrap_or(8765);
    // 令牌是凭据：和 mac 一样用密码框，不把明文摊在屏幕上；已设置时留空＝不改。
    // 注意 config JSON 只给 lan_has_token（不暴露明文），读 lan_token 永远是空
    let has_token = c["lan_has_token"].as_bool().unwrap_or(false);
    w.lan_has_token = has_token;
    section(w, "局域网听写", 24.0);
    info(w, 772.0, 54.0, "别的设备把 16k 单声道 WAV POST 到 /transcribe，本机识别后回文字；只在本机用可以不打开。");
    w.c_lan_en = Some(checkbox(
        w,
        "开启",
        ID_LAN_EN,
        enabled,
        196.0,
        56.0,
        560.0,
    ));
    label(w, "端口", 196.0, 96.0, 60.0, 18.0);
    w.e_lan_port = Some(child(
        "EDIT",
        &port.to_string(),
        WS_BORDER.0 | ES_NUMBER as u32,
        w,
        250.0,
        92.0,
        90.0,
        26.0,
        ID_LAN_PORT,
    ));
    label(
        w,
        if has_token { "令牌（已设置）" } else { "令牌" },
        380.0,
        96.0,
        110.0,
        18.0,
    );
    info(w, 496.0, 93.0, "允许其它设备连进来时必填：请求要带 Authorization: Bearer <令牌>；只在本机用可以不填。留空保存＝不改已有令牌。");
    w.e_lan_token = Some(child(
        "EDIT",
        "",
        WS_BORDER.0 | ES_PASSWORD as u32,
        w,
        520.0,
        92.0,
        150.0,
        26.0,
        ID_LAN_TOKEN,
    ));
    // 一键生成（R-132）：用户不用自己想一个 token
    button(w, "生成", ID_LAN_GEN, 676.0, 91.0, 64.0);

    w.c_lan_allow = Some(checkbox(
        w,
        "允许其它设备连进来",
        ID_LAN_ALLOW,
        allow,
        196.0,
        158.0,
        664.0,
    ));
    // 怎么连你：地址 + 一条能照抄的 curl（R-132）
    let ip = local_ipv4().unwrap_or_else(|| "（先连上网络）".into());
    let url = format!("http://{}:{}/transcribe", ip, port);
    let hint = if allow {
        format!(
            "{}\r\ncurl -X POST {} -H \"x-eryu-token: <令牌>\" -H \"Content-Type: audio/wav\" --data-binary @a.wav",
            url, url
        )
    } else {
        "现在只监听本机 127.0.0.1；勾上「允许其它设备连进来」后这里会给出连接地址".to_string()
    };
    label(w, "怎么连", 196.0, 190.0, 664.0, 18.0);
    child(
        "STATIC",
        &hint,
        0,
        w,
        196.0,
        208.0,
        664.0,
        44.0,
        ID_LAN_URL,
    );
    button(w, "保存并重启接口", ID_LAN_SAVE, 196.0, 258.0, 170.0);
    button(w, "刷新日志", ID_LAN_REFRESH, 380.0, 258.0, 110.0);
    // 日志：核心返回的是 JSON 数组（at/src/path/code/ms/bytes），这里排成几行可读文本
    let log = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.lan_log(),
            None => String::new(),
        }
    };
    let mut text = String::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&log) {
        let items = v.as_array().cloned().unwrap_or_default();
        // 显示最近 6 条（新的在下）
        let tail: Vec<_> = items.iter().rev().take(6).collect();
        for e in tail.iter().rev() {
            let at = e["at"].as_i64().unwrap_or(0);
            // at 是毫秒时间戳；用北京时间（UTC+8）拼 HH:MM:SS，简单够用
            let secs = (at / 1000 + 8 * 3600).rem_euclid(86400);
            let path = e["path"].as_str().unwrap_or("");
            let src = e["src"].as_str().unwrap_or("");
            let code = e["code"].as_i64().unwrap_or(0);
            let ms = e["ms"].as_i64().unwrap_or(0);
            text.push_str(&format!(
                "{:02}:{:02}:{:02}  {} {}  → {} ({} ms)\r\n",
                secs / 3600,
                (secs % 3600) / 60,
                secs % 60,
                src,
                path,
                code,
                ms
            ));
        }
    }
    if text.is_empty() {
        text = "（还没有请求）".into();
    }
    w.lbl_lan = Some(child(
        "STATIC",
        &text,
        0,
        w,
        196.0,
        296.0,
        664.0,
        96.0,
        0,
    ));

}

fn page_voice(w: &mut Window) {
    let (state_json, cfg_json) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => (a.core.voice_state(), a.core.config_json()),
            None => (String::new(), String::new()),
        }
    };
    let st: serde_json::Value = serde_json::from_str(&state_json).unwrap_or(serde_json::Value::Null);
    let cv: serde_json::Value = serde_json::from_str(&cfg_json).unwrap_or(serde_json::Value::Null);
    let enrolled = st["enrolled"].as_bool().unwrap_or(false);
    let dim = st["dim"].as_i64().unwrap_or(0);
    let created = st["created"].as_str().unwrap_or("");
    let model_ready = st["model_ready"].as_bool().unwrap_or(false);
    let enabled = cv["voice_lock"].as_bool().unwrap_or(false);

    section(w, "只录我的声音", 24.0);
    w.c_voice = Some(checkbox(
        w,
        "只认我的声音",
        ID_VOICE_LOCK,
        enabled,
        196.0,
        56.0,
        560.0,
    ));
    let status = if enrolled {
        format!("已登记：{dim} 维，{created}")
    } else {
        "还没登记".to_string()
    };
    label(w, &status, 196.0, 96.0, 664.0, 18.0);
    label(
        w,
        if model_ready {
            "声纹模型已就绪。"
        } else {
            "声纹模型还没下载：第一次登记时会自动拉（约 20 MB）。"
        },
        196.0,
        120.0,
        664.0,
        18.0,
    );
    info(w, 344.0, 194.0, "登记会占 8 秒，环境安静些更准；清空声纹时「只认我的声音」会一起关掉。");
    button(w, "录 8 秒登记", ID_VOICE_ENROLL, 196.0, 156.0, 130.0);
    button(w, "清空声纹", ID_VOICE_CLEAR, 338.0, 156.0, 110.0);
    label(
        w,
        "登记时正常说一句话。",
        196.0,
        196.0,
        300.0,
        36.0,
    );
}

fn page_diag(w: &mut Window) {
    // 诊断页的第一屏必须是「哪里不对」，而不是一份 TOML。用户点进来是想知道
    // 为什么不能用：模型在不在、引擎起没起、认不认得出麦克风、上次上屏成没成。
    let diag = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.diagnose(),
            None => "核心没有起来（看下面「日志」里的最后几行）".to_string(),
        }
    };
    section(w, "状态", 24.0);
    let shown = diag.replace("\r\n", "\n").replace('\n', "\r\n");
    let h = child(
        "EDIT",
        &shown,
        WS_BORDER.0 | WS_VSCROLL.0 | ES_MULTILINE as u32 | ES_READONLY as u32,
        w,
        196.0,
        52.0,
        664.0,
        132.0,
        ID_EDIT_BASE + 8,
    );
    w.ctrls.push(h);
    w.e_diag = Some(h);
    button(w, "复制状态", ID_COPY_DIAG, 196.0, 192.0, 96.0);
    button(w, "打开日志", ID_OPENLOG, 300.0, 192.0, 96.0);
    button(w, "运行自检", ID_SELFTEST, 404.0, 192.0, 100.0);
    button(w, "麦克风权限", ID_MICPRIV, 512.0, 192.0, 104.0);
    let mic = mic_privacy_state();
    let mic_line = format!("系统麦克风权限：{mic}");
    label(w, &mic_line, 624.0, 196.0, 236.0, 18.0);
    button(w, "导出诊断包", ID_DIAG_SAVE, 196.0, 226.0, 110.0);
    button(w, "上传诊断包", ID_DIAG_UPLOAD, 314.0, 226.0, 110.0);
    let hint = {
        let r = crate::lock(&DIAG_RESULT).clone();
        if r.is_empty() {
            "诊断包不含你说话的内容：只有版本、配置（密钥打码）、错误码、日志尾部。".to_string()
        } else {
            r
        }
    };
    label(w, &hint, 432.0, 230.0, 428.0, 36.0);

    section(w, "配置全文（高级：改完点保存立刻生效）", 268.0);
    let toml = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.config_toml(),
            None => String::new(),
        }
    };
    // Win32 EDIT 只认 CRLF：直接塞 LF 会被吞成一行（读回时要再转回 LF）
    let shown = toml.replace("\r\n", "\n").replace('\n', "\r\n");
    let h2 = child(
        "EDIT",
        &shown,
        WS_BORDER.0 | WS_VSCROLL.0 | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32,
        w,
        196.0,
        294.0,
        664.0,
        128.0,
        ID_TOML,
    );
    w.ctrls.push(h2);
    w.e_toml = Some(h2);
    button(w, "保存 TOML", ID_SAVE_TOML, 196.0, 432.0, 104.0);
    button(w, "复制全文", ID_COPY, 308.0, 432.0, 90.0);
    button(w, "打开配置目录", ID_OPENDIR, 406.0, 432.0, 128.0);
}

/// 诊断页把编辑器里的整份 TOML 写回（对齐 mac 的「保存 TOML」）
fn save_toml() {
    let text = with_win(|w| w.e_toml)
        .flatten()
        .map(|h| unsafe { crate::settings::window_text(h) })
        .unwrap_or_default();
    if text.trim().is_empty() {
        crate::message_box("配置为空，没有保存。");
        return;
    }
    let text = text.replace("\r\n", "\n");
    let (code, err) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.set_config_toml(&text),
            None => (-1, "core 没起来".to_string()),
        }
    };
    crate::report(&format!("SAVETOML code={code} err={err}"));
    if code == 0 {
        crate::reload_hotkey();
        rebuild_page();
        crate::message_box("已保存 config.toml。");
    } else {
        crate::message_box(&format!("保存失败：{err}"));
    }
}

/// Windows 11 的「麦克风访问」全局开关（设置 → 隐私和安全性 → 麦克风）
/// 读 HKCU\...\CapabilityAccessManager\ConsentStore\microphone 的 Value
pub fn mic_privacy_state() -> &'static str {
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_READ, REG_SZ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
    };
    let sub = wide("Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone");
    let mut key = Default::default();
    unsafe {
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            0,
            KEY_READ,
            &mut key,
        )
        .is_err()
        {
            return "未知";
        }
        let name = wide("Value");
        let mut ty = Default::default();
        let mut buf = [0u16; 64];
        let mut len = (buf.len() * 2) as u32;
        let rc = RegQueryValueExW(
            key,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut ty),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut len),
        );
        let _ = RegCloseKey(key);
        if rc.is_err() {
            return "未知";
        }
        let _ = ty;
        let _ = REG_SZ;
        let raw: Vec<u16> = buf[..(len as usize / 2).saturating_sub(1)].to_vec();
        match String::from_utf16_lossy(&raw).as_str() {
            "Allow" => "允许",
            "Deny" => "已拒绝",
            _ => "未知",
        }
    }
}

fn save_general() {
    let (hotkey, min_ms, max_s, mode, privacy, launch) = with_win(|w| {
        let hotkey = w.e_hotkey.map(crate::keycap::text).unwrap_or_default();
        let min_ms = w.e_min.and_then(get_int).unwrap_or(400);
        let max_s = w.e_max.and_then(get_int).unwrap_or(120);
        let mode = if w.mode_paste { "paste" } else { "keystroke" };
        let privacy = w.c_privacy.map(checked).unwrap_or(false);
        let launch = w.c_launch.map(checked).unwrap_or(false);
        (hotkey, min_ms, max_s, mode.to_string(), privacy, launch)
    })
    .unwrap_or_default();
    // 保存前先确认这个组合键能被解析：否则 core 只负责原样存，壳启动时解析失败会
    // 静默退回 ALT_R（stderr 在 GUI 进程里看不到），用户以为设成了、其实没生效。
    if crate::hotkey::parse(&hotkey).is_none() {
        crate::message_box(&format!(
            "「{hotkey}」这个组合键看不懂，没有保存。\n\n可用的写法：ALT_R、CTRL+ALT+SPACE、F13，或点上面的常用键。"
        ));
        return;
    }
    let patch = serde_json::json!({
        "hotkey": hotkey,
        "min_record_ms": min_ms.clamp(200, 1500),
        "max_record_secs": max_s.clamp(5, 600),
        "insert_mode": mode,
        "privacy_mode": privacy,
        "launch_at_login": launch,
    });
    apply(patch.to_string());
    // 真实注册/注销 Run 键（写注册表失败要让人看见，静默就当没这功能）
    if let Err(e) = crate::autostart::sync(launch) {
        crate::report(&format!("AUTOSTART sync 失败：{e}"));
        crate::message_box(&format!("开机自启设置没写进注册表：{e}"));
    }
}

fn save_audio() {
    let (device, denoise, pause_media, punct, source_lock) = with_win(|w| {
        let mut device = String::new();
        if let Some(h) = w.lst {
            let sel = list_sel(h);
            if sel >= 0 {
                let line = list_item(h, sel as usize);
                device = line.split('　').next().unwrap_or("").trim().to_string();
            }
        }
        let denoise = w.c_denoise.map(checked).unwrap_or(true);
        let pause_media = w.c_pause_media.map(checked).unwrap_or(false);
        let punct = w.c_punct.map(checked).unwrap_or(true);
        let source_lock = w.c_source_lock.map(checked).unwrap_or(false);
        (device, denoise, pause_media, punct, source_lock)
    })
    .unwrap_or_default();
    let patch = serde_json::json!({
        "device": device,
        "denoise": denoise,
        "pause_media": pause_media,
        "punct": punct,
        "source_lock": source_lock,
    });
    apply(patch.to_string());
}

/// 下载进度更新（由 main.rs 的 model_progress 事件调用）
pub fn on_model_progress(recv: u64, total: u64) {
    let Ok(g) = WIN.try_lock() else { return };
    let Some(w) = g.as_ref() else { return };
    let Some(h) = w.pull_lbl else { return };
    drop(g);
    let text = if total > 0 {
        let pct = (recv.saturating_mul(100) / total).min(100);
        format!("下载进度：{pct}%（{} / {} MB）", recv / 1_048_576, total / 1_048_576)
    } else {
        format!("下载进度：已收到 {} MB", recv / 1_048_576)
    };
    unsafe {
        SetWindowTextW(h, PCWSTR(wide(&text).as_ptr()));
    }
}

/// 设置页「运行自检」：跑 core 的 diagnose，把结论用对话框列出来（对齐 mac 诊断页）
fn run_selftest() {
    let (json, model_ok, mic_ok) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => {
                let j = a.core.diagnose();
                let v: serde_json::Value = serde_json::from_str(&j).unwrap_or(serde_json::Value::Null);
                let e = v["engine"].as_str().unwrap_or("?").to_string();
                let devs = v["devices"].as_array().map(|x| x.len()).unwrap_or(0);
                (j, e, devs)
            }
            None => (String::new(), "?".to_string(), 0),
        }
    };
    if json.is_empty() {
        crate::message_box("自检失败：core 没起来。");
        return;
    }
    crate::report(&format!("SELFTEST diag={json}"));
    crate::message_box(&format!(
        "自检结果\n\n引擎：{model_ok}\n录音设备：{mic_ok} 个\n\n完整诊断写进了 eryu-cli.log（SELFTEST 行）。"
    ));
}

fn apply(json: String) {
    let (code, err) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.set_config(&json),
            None => (-1, "core 没起来".to_string()),
        }
    };
    if code != 0 {
        crate::message_box(&format!("保存失败：{err}"));
    } else {
        crate::reload_hotkey();
        crate::tray::set_tip("字在：设置已保存");
        // 重建当前页，显示刚落盘的值
        rebuild_page();
    }
}

fn copy_history() {
    let sel = with_win(|w| w.lst2.map(list_sel)).flatten().unwrap_or(-1);
    if sel < 0 {
        return;
    }
    let text = HIST_TEXTS
        .lock()
        .ok()
        .and_then(|g| g.get(sel as usize).cloned())
        .unwrap_or_default();
    if text.is_empty() {
        return;
    }
    unsafe {
        use windows::Win32::System::DataExchange::*;
        use windows::Win32::System::Memory::*;
        if OpenClipboard(HWND::default()).is_ok() {
            let _ = EmptyClipboard();
            let w: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = w.len() * 2;
            if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, bytes) {
                let p = GlobalLock(h);
                if !p.is_null() {
                    std::ptr::copy_nonoverlapping(w.as_ptr() as *const u8, p as *mut u8, bytes);
                    let _ = GlobalUnlock(h);
                    let _ = SetClipboardData(13, windows::Win32::Foundation::HANDLE(h.0));
                }
            }
            let _ = CloseClipboard();
        }
    }
    crate::tray::set_tip("字在：历史文本已复制");
}

fn clear_history() {
    if crate::message_box_yesno("清空全部历史记录？这个操作不能撤销。") {
        {
            let app = crate::lock(&crate::APP);
            if let Some(a) = app.as_ref() {
                let _ = a.core.clear_history();
            }
        }
        rebuild_page();
    }
}

/// 删除选中的那一条历史（只删一条，不是清空）
fn delete_selected_history() {
    let sel = with_win(|w| w.lst2.map(list_sel)).flatten().unwrap_or(-1);
    if sel < 0 {
        return;
    }
    let id = HIST_IDS
        .lock()
        .ok()
        .and_then(|g| g.get(sel as usize).cloned())
        .unwrap_or_default();
    if id.is_empty() {
        return;
    }
    {
        let app = crate::lock(&crate::APP);
        if let Some(a) = app.as_ref() {
            let (code, err) = a.core.delete_history(&id);
            crate::report(&format!("HIST del id={id} code={code} err={err}"));
        }
    }
    rebuild_page();
}

/// 重试上次失败的段（与托盘菜单同一条路径）
fn retry_failed_segments() {
    let n = {
        let app = crate::lock(&crate::APP);
        app.as_ref().map(|a| a.core.failed_segments().len()).unwrap_or(0)
    };
    if n == 0 {
        crate::message_box("没有失败的段要重试。");
        return;
    }
    let rc = {
        let app = crate::lock(&crate::APP);
        app.as_ref().map(|a| a.core.retry_failed()).unwrap_or(-1)
    };
    crate::report(&format!("RETRY failed rc={rc}"));
    rebuild_page();
}

/// 给选中的档写快捷键槽（空字符串=解绑）。写回整份 profiles 后重注册。
/// 自测：给第一档写一句指令，读回确认（VM 里没法手点，用它做回归）
pub fn self_test_prompt(text: &str) {
    let (hwnd_prompt, name) = {
        let g = crate::lock(&WIN);
        match g.as_ref() {
            Some(w) => {
                let name = w
                    .lst3
                    .map(|h| unsafe {
                        let n = SendMessageW(h, LB_GETCOUNT, WPARAM(0), LPARAM(0)).0;
                        if n > 0 {
                            let _ = SendMessageW(h, LB_SETCURSEL, WPARAM(0), LPARAM(0));
                            list_item(h, 0)
                        } else {
                            String::new()
                        }
                    })
                    .unwrap_or_default();
                (w.e_prompt, name)
            }
            None => (None, String::new()),
        }
    };
    let Some(ph) = hwnd_prompt else {
        crate::report("PROMPTSEL FAIL 编辑框没起来");
        return;
    };
    unsafe {
        let w: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = SetWindowTextW(ph, PCWSTR(w.as_ptr()));
    }
    if let Some(w) = crate::lock(&WIN).as_mut() {
        w.e_prompt = Some(ph);
    }
    save_prompt();
    let want_name: String = name
        .trim()
        .trim_start_matches('✓')
        .trim()
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();
    let got = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => {
                let v: serde_json::Value =
                    serde_json::from_str(&a.core.profiles_json()).unwrap_or_default();
                v["profiles"]
                    .as_array()
                    .and_then(|l| l.iter().find(|p| p["name"].as_str() == Some(want_name.as_str())))
                    .and_then(|p| p["prompt"].as_str().map(|s| s.to_string()))
                    .unwrap_or_default()
            }
            None => String::new(),
        }
    };
    crate::report(&format!("PROMPTSEL profile={} prompt={}", name, got));
    if got == text { crate::report("PROMPTSEL OK"); } else { crate::report("PROMPTSEL FAIL"); }
}

/// 把「指令」编辑框的内容写进当前选中的那一档（对齐 mac 的 prompt 编辑）
fn save_prompt() {
    let sel = with_win(|w| w.lst3.map(list_sel)).flatten().unwrap_or(-1);
    if sel < 0 {
        crate::tray::set_tip("字在：先在列表里选一档");
        return;
    }
    let line = with_win(|w| w.lst3.map(|h| list_item(h, sel as usize)))
        .flatten()
        .unwrap_or_default();
    let name: String = line
        .chars()
        .skip_while(|c| c.is_whitespace() || *c == '✓')
        .take_while(|c| !c.is_whitespace())
        .collect();
    let text = with_win(|w| w.e_prompt.map(text_of)).flatten().unwrap_or_default();
    let (code, err) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => {
                let mut v: serde_json::Value =
                    serde_json::from_str(&a.core.profiles_json()).unwrap_or_default();
                if let Some(list) = v["profiles"].as_array_mut() {
                    for p in list.iter_mut() {
                        if p["name"].as_str() == Some(name.as_str()) {
                            p["prompt"] = serde_json::Value::String(text.clone());
                        }
                    }
                }
                a.core.set_profiles(&v.to_string())
            }
            None => (-1, "core 没起来".to_string()),
        }
    };
    if code != 0 {
        crate::message_box(&format!("保存指令失败：{err}"));
    } else {
        crate::tray::set_tip(&format!("字在：已保存「{name}」的指令"));
    }
}

fn save_slot() {
    let sel = with_win(|w| w.lst3.map(list_sel)).flatten().unwrap_or(-1);
    if sel < 0 {
        return;
    }
    let line = with_win(|w| w.lst3.map(|h| list_item(h, sel as usize)))
        .flatten()
        .unwrap_or_default();
    let name: String = line
        .chars()
        .skip_while(|c| c.is_whitespace() || *c == '✓')
        .take_while(|c| !c.is_whitespace())
        .collect();
    let slot = with_win(|w| w.e_slot.map(text_of)).flatten().unwrap_or_default();
    let slot = slot.trim().to_string();
    let (profiles_json, code, err) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => {
                let mut v: serde_json::Value =
                    serde_json::from_str(&a.core.profiles_json()).unwrap_or_default();
                if let Some(list) = v["profiles"].as_array_mut() {
                    for p in list.iter_mut() {
                        if p["name"].as_str() == Some(name.as_str()) {
                            p["slot"] = serde_json::Value::String(slot.clone());
                        }
                    }
                }
                let (c, e) = a.core.set_profiles(&v.to_string());
                ((), c, e)
            }
            None => ((), -1, "core 没起来".to_string()),
        }
    };
    let _ = profiles_json;
    if code != 0 {
        crate::message_box(&format!("保存快捷键槽失败：{err}"));
        return;
    }
    crate::reload_hotkey();
    let tip = if slot.is_empty() {
        "字在：已解绑这个档的快捷键".to_string()
    } else {
        format!("字在：档「{name}」快捷键 = {slot}")
    };
    crate::tray::set_tip(&tip);
    rebuild_page();
}

fn activate_profile() {
    let sel = with_win(|w| w.lst3.map(list_sel)).flatten().unwrap_or(-1);
    if sel < 0 {
        return;
    }
    let line = with_win(|w| w.lst3.map(|h| list_item(h, sel as usize)))
        .flatten()
        .unwrap_or_default();
    let name: String = line
        .chars()
        .skip_while(|c| c.is_whitespace() || *c == '✓')
        .take_while(|c| !c.is_whitespace())
        .collect();
    if name.is_empty() {
        return;
    }
    let code = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.set_active_profile(&name),
            None => -1,
        }
    };
    if code == 0 {
        crate::tray::set_tip(&format!("字在：润色档已切到 {name}"));
        rebuild_page();
    } else {
        crate::message_box("切换润色档失败（文件里没有这一档？）");
    }
}

/// 自测：设置页输入（R-137）——在热词多行框里逐字敲 5 个字符，再走一次「往自己窗口粘贴」。
/// 两件事必须都为真：① 键入不打乱控件（页面不重建、焦点不丢）；② 注入能落进自己的输入框并被读回。
pub fn input_selftest() -> String {
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, SetWindowTextW, WM_CHAR};
    open_at(3);
    std::thread::sleep(std::time::Duration::from_millis(500));
    let h = {
        let g = crate::lock(&WIN);
        g.as_ref().and_then(|w| w.e_hotwords)
    };
    let Some(h) = h else {
        return "INPUT FAIL 热词输入框没建出来".to_string();
    };
    let before = window_text(h);
    unsafe {
        let _ = SetFocus(h);
        for ch in "abcde".chars() {
            SendMessageW(h, WM_CHAR, WPARAM(ch as usize), LPARAM(0));
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    let typed = window_text(h);
    let typed_ok = typed.ends_with("abcde");
    if let Some(w) = hwnd() {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(w);
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    let (status, detail) = crate::inject::paste("注入自测ABC", "paste");
    std::thread::sleep(std::time::Duration::from_millis(400));
    let after = window_text(h);
    let inject_ok = after.contains("注入自测ABC");
    // 还原成原内容，别把演示文字留在用户的热词框里
    let restore: Vec<u16> = before.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        SetWindowTextW(h, PCWSTR(restore.as_ptr()));
    }
    format!(
        "INPUT typed_ok={typed_ok} typed_tail={:?} inject={status} inject_ok={inject_ok} after_len={} detail={detail}",
        typed.chars().rev().take(8).collect::<String>().chars().rev().collect::<String>(),
        after.chars().count()
    )
}

/// 读一个控件里的文字（保存 TOML 用）
pub fn window_text(h: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(h);
        if n <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(h, &mut buf);
        String::from_utf16_lossy(&buf[..got.max(0) as usize])
    }
}

fn with_win<T: Default>(f: impl FnOnce(&Window) -> T) -> Option<T> {
    let g = crate::lock(&WIN);
    g.as_ref().map(f)
}

fn checked(h: HWND) -> bool {
    let id = unsafe { GetDlgCtrlID(h) } as usize;
    toggle_on(id)
}

fn combo_sel(h: HWND) -> usize {
    unsafe { SendMessageW(h, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as usize }
}

fn list_sel(h: HWND) -> i32 {
    unsafe { SendMessageW(h, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as i32 }
}

fn list_item(h: HWND, i: usize) -> String {
    unsafe {
        let mut buf = [0u16; 512];
        let n = SendMessageW(
            h,
            LB_GETTEXT,
            WPARAM(i),
            LPARAM(buf.as_mut_ptr() as isize),
        )
        .0;
        if n <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

/// 自测/自动化用：直接开/关电平测试（等价于点「开始测试」按钮）
pub fn set_meter_on(on: bool) {
    {
        let mut g = crate::lock(&WIN);
        if let Some(w) = g.as_mut() {
            w.meter_on = on;
        }
    }
    rebuild_page();
}

/// 有新的一句落盘时，如果正停在历史页就重建一次（省掉一个「刷新」按钮）
pub(crate) fn refresh_history_if_open() {
    let hist = with_win(|w| w.page == 4).unwrap_or(false);
    if !hist {
        return;
    }
    let visible = with_win(|w| w.hwnd)
        .map(|h| unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(h).as_bool() })
        .unwrap_or(false);
    if visible {
        rebuild_page();
    }
}

fn rebuild_page() {
    let hwnd = with_win(|w| w.hwnd).unwrap_or_default();
    if !hwnd.0.is_null() {
        build(hwnd);
        unsafe {
            InvalidateRect(hwnd, None, true);
        }
    }
}


/// 圆角矩形填充（设计稿的卡片/按钮/选中态都是圆角）
/// 聚焦控件外框（EDIT/COMBO 的自绘焦点框：系统那个亮色在深底上很刺眼）
/// 这个控件是不是"可以打字的输入框"（Edit / ComboBox）——焦点蓝框只给它们画
fn is_input(h: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    let name = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
    name == "Edit" || name == "ComboBox"
}

unsafe fn round_fill(hdc: windows::Win32::Graphics::Gdi::HDC, rc: &windows::Win32::Foundation::RECT, brush: HBRUSH, radius: i32) {
    let old = SelectObject(hdc, brush);
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, radius, radius);
    SelectObject(hdc, old);
}

unsafe fn text_at(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    hfont: HFONT,
    s: &str,
    rc: &windows::Win32::Foundation::RECT,
    color: u32,
    center: bool,
) {
    let old = SelectObject(hdc, hfont);
    SetTextColor(hdc, COLORREF(color));
    SetBkMode(hdc, TRANSPARENT);
    let mut buf: Vec<u16> = s.encode_utf16().collect();
    let mut r = *rc;
    let mut flags = DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS;
    if center {
        flags |= DT_CENTER;
    }
    let flags = DRAW_TEXT_FORMAT(flags.0);
    DrawTextW(hdc, &mut buf, &mut r, flags);
    SelectObject(hdc, old);
}

/// 整窗自绘：深色底 + 顶部标题 + 左侧导航（选中项蓝色圆角条）+ 内容卡片
unsafe fn paint(hwnd: HWND) {
    // build() 持锁创建控件时会同步触发 WM_PAINT，这里阻塞等锁就是死锁 → 用 try_lock。
    // 拿不到锁时不能整帧跳过：至少把深色底和卡片画出来（否则卡片底色缺失，I-105）。
    let chrome = {
        match WIN.try_lock() {
            Ok(g) => g
                .as_ref()
                .map(|w| {
                    (
                        w.k,
                        Some((
                            w.page,
                            w.font,
                            w.font_title,
                            w.brush_bg,
                            w.brush_card,
                            w.hover.clone(),
                        )),
                    )
                }),
            Err(_) => None,
        }
    };
    let (k, page, font, font_title, bg, card, hover) = match chrome {
        Some((k, Some((page, font, font_title, bg, card, hover)))) => {
            (k, page, font, font_title, bg, card, hover)
        }

        _ => {
            // 没有 Window 上下文：临时画刷画底与卡片，文本留给下一次真正的 WM_PAINT
            let k = GetDpiForWindow(hwnd) as f32 / 96.0;
            let bg = CreateSolidBrush(COLORREF(C_BG));
            let card = CreateSolidBrush(COLORREF(C_CARD));
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut client = windows::Win32::Foundation::RECT::default();
            GetClientRect(hwnd, &mut client).ok();
            FillRect(hdc, &client, bg);
            let card_rc = windows::Win32::Foundation::RECT {
                left: px(k, 168.0),
                top: px(k, 12.0),
                right: client.right - px(k, 16.0),
                bottom: client.bottom - px(k, 18.0),
            };
            round_fill(hdc, &card_rc, card, px(k, 12.0));
            EndPaint(hwnd, &ps);
            let _ = DeleteObject(bg);
            let _ = DeleteObject(card);
            return;
        }
    };
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut client = windows::Win32::Foundation::RECT::default();
    GetClientRect(hwnd, &mut client).ok();
    FillRect(hdc, &client, bg);
    // 顶栏：底色 + 标题（关闭按钮是子控件，见 add_header_close）
    {
        let bar = windows::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: client.right,
            bottom: px(k, HEADER),
        };
        let bb = CreateSolidBrush(COLORREF(C_BG));
        FillRect(hdc, &bar, bb);
        let _ = DeleteObject(bb);
        let line = windows::Win32::Foundation::RECT {
            top: px(k, HEADER) - 1,
            bottom: px(k, HEADER),
            ..bar
        };
        let lb = CreateSolidBrush(COLORREF(C_LINE));
        FillRect(hdc, &line, lb);
        let _ = DeleteObject(lb);
        // 顶栏只留 ✕ 和一条分隔线；标题在侧栏里（重复两次显得啰嗦）
    }
    let _ = windows::Win32::Graphics::Gdi::SetViewportOrgEx(hdc, 0, px(k, HEADER), None);
    // 内容区坐标从顶栏下方算起
    client.bottom -= px(k, HEADER);
    // 内容卡片
    let card_rc = windows::Win32::Foundation::RECT {
        left: px(k, 168.0),
        top: px(k, 12.0),
        right: client.right - px(k, 16.0),
        bottom: client.bottom - px(k, 18.0),
    };
    round_fill(hdc, &card_rc, card, px(k, 12.0));
    // 标题
    let title_font = font_title;
    let t_rc = windows::Win32::Foundation::RECT {
        left: px(k, 20.0),
        top: px(k, 16.0),
        right: px(k, 150.0),
        bottom: px(k, 40.0),
    };
    text_at(hdc, title_font, "字在 设置", &t_rc, C_TEXT, false);
    let s_rc = windows::Win32::Foundation::RECT {
        top: px(k, 38.0),
        bottom: px(k, 56.0),
        ..t_rc
    };
    text_at(hdc, font, "本机识别，不联网", &s_rc, C_MUTED, false);
    // 导航项
    let mut nav = Vec::new();
    for (i, name) in PAGES.iter().enumerate() {
        let top = px(k, 76.0) + (px(k, 44.0) * i as i32);
        let rc = windows::Win32::Foundation::RECT {
            left: px(k, 12.0),
            top,
            right: px(k, 156.0),
            bottom: top + px(k, 36.0),
        };
        // 注意：paint 里已经 SetViewportOrgEx 下移了 HEADER，rc 是绘制坐标；
        // 点击命中用的是客户区坐标，所以要补回 HEADER，否则整列都点不准。
        nav.push((
            rc.left,
            rc.top + px(k, HEADER),
            rc.right,
            rc.bottom + px(k, HEADER),
        ));
        if i == page {
            let b = CreateSolidBrush(COLORREF(C_ACCENT));
            round_fill(hdc, &rc, b, px(k, 9.0));
            let _ = DeleteObject(b);
        }
        let dot = format!("{}  {}", PAGE_ICONS[i], name);
        let mut tr = rc;
        tr.left += px(k, 14.0);
        text_at(
            hdc,
            font,
            &dot,
            &tr,
            if i == page { 0x00FFFFFF } else { C_MUTED },
            false,
        );
    }
    if let Ok(t) = TIPS.lock() {
        let f = ui_font(k, 10.0, 600);
        for (x, y, w0, h0, _) in t.iter() {
            let cx = x + w0 / 2;
            let cy = y - px(k, HEADER) + h0 / 2;
            let pen = windows::Win32::Graphics::Gdi::CreatePen(
                windows::Win32::Graphics::Gdi::PS_SOLID,
                1,
                COLORREF(C_MUTED),
            );
            let ob = windows::Win32::Graphics::Gdi::SelectObject(
                hdc,
                windows::Win32::Graphics::Gdi::GetStockObject(
                    windows::Win32::Graphics::Gdi::NULL_BRUSH,
                ),
            );
            let op = windows::Win32::Graphics::Gdi::SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(pen.0));
            let _ = windows::Win32::Graphics::Gdi::Ellipse(hdc, cx - px(k, 7.0), cy - px(k, 7.0), cx + px(k, 7.0), cy + px(k, 7.0));
            let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, op);
            let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, ob);
            let _ = DeleteObject(pen);
            let mut r = windows::Win32::Foundation::RECT {
                left: cx - px(k, 7.0),
                top: cy - px(k, 7.0),
                right: cx + px(k, 7.0),
                bottom: cy + px(k, 7.0),
            };
            let _ = windows::Win32::Graphics::Gdi::SetTextColor(hdc, COLORREF(C_MUTED));
            let _ = windows::Win32::Graphics::Gdi::SetBkMode(hdc, windows::Win32::Graphics::Gdi::TRANSPARENT);
            let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(f.0));
            DrawTextW(
                hdc,
                &mut wide("i"),
                &mut r,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
        let _ = DeleteObject(f);
    }
    // 悬停气泡（同一个窗口内画，避免独立窗口收不到鼠标/被裁）
    if let Some((text, hx, hy)) = hover.clone() {
        let f = ui_font(k, 12.0, 400);
        let maxw = px(k, 320.0);
        let mut size = windows::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: maxw,
            bottom: 0,
        };
        // 先按最大宽度量高度（DT_CALCRECT + DT_WORDBREAK），再决定气泡尺寸
        DrawTextW(
            hdc,
            &mut wide(&text),
            &mut size,
            DT_LEFT | DT_WORDBREAK | DRAW_TEXT_FORMAT(0x0400),
        );
        let text_w = size.right.max(px(k, 60.0)).min(px(k, 240.0));
        let bw = text_w + px(k, 20.0);
        let bh = size.bottom + px(k, 14.0);
        let mut bx = hx + px(k, 4.0);
        // 气泡优先放在图标右侧（右侧通常是空白），下方是子控件，会被它们盖住
        let mut by = hy - px(k, HEADER) - bh / 2;
        if by < px(k, 6.0) {
            by = px(k, 6.0);
        }
        if by + bh > client.bottom - px(k, 8.0) {
            by = hy - px(k, HEADER) - bh - px(k, 8.0);
        }
        let bubble = windows::Win32::Foundation::RECT { left: bx, top: by, right: bx + bw, bottom: by + bh };
        let bb = CreateSolidBrush(COLORREF(0x002A2A2E));
        round_fill(hdc, &bubble, bb, px(k, 8.0));
        let _ = DeleteObject(bb);
        let pen = windows::Win32::Graphics::Gdi::CreatePen(
            windows::Win32::Graphics::Gdi::PS_SOLID,
            1,
            COLORREF(C_LINE),
        );
        let op = windows::Win32::Graphics::Gdi::SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(pen.0));
        let ob = windows::Win32::Graphics::Gdi::SelectObject(
            hdc,
            windows::Win32::Graphics::Gdi::GetStockObject(windows::Win32::Graphics::Gdi::NULL_BRUSH),
        );
        let _ = RoundRect(hdc, bx, by, bx + bw, by + bh, px(k, 8.0), px(k, 8.0));
        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, ob);
        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, op);
        let _ = DeleteObject(pen);
        let mut tr = bubble;
        tr.left += px(k, 10.0);
        tr.right -= px(k, 10.0);
        tr.top += px(k, 6.0);
        tr.bottom -= px(k, 6.0);
        let _ = windows::Win32::Graphics::Gdi::SetTextColor(hdc, COLORREF(C_TEXT));
        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(f.0));
        DrawTextW(hdc, &mut wide(&text), &mut tr, DT_LEFT | DT_WORDBREAK);
        let _ = DeleteObject(f);
    }
    if let Ok(mut g) = NAV.lock() {
        *g = nav;
    }
    EndPaint(hwnd, &ps);
}

/// 自绘按钮
fn is_toggle(id: usize) -> bool {
    matches!(id, ID_DENOISE | ID_POLISH_EN | ID_SAVE if false)
        || id == ID_EDIT_BASE + 4
        || id == ID_EDIT_BASE + 5
        || id == ID_DENOISE
        || id == ID_POLISH_EN
        || id == ID_PAUSE_MEDIA
        || id == ID_HIST_EN
        || id == ID_PUNCT
        || id == ID_SOURCE_LOCK
        || id == ID_VOICE_LOCK
        || id == ID_LAN_EN
        || id == ID_LAN_ALLOW
}

unsafe fn draw_toggle(dis: &DRAWITEMSTRUCT) {
    let on = toggle_on(dis.CtlID as usize);
    let k = GetDpiForWindow(dis.hwndItem) as f32 / 96.0;
    // 焦点描边：Tab 到这里时能看见
    let focused = dis.itemState.0 & windows::Win32::UI::Controls::ODS_FOCUS.0 != 0
        || GetFocus() == dis.hwndItem;
    if focused {
        let pen = windows::Win32::Graphics::Gdi::CreatePen(
            windows::Win32::Graphics::Gdi::PS_SOLID,
            2,
            COLORREF(C_ACCENT),
        );
        let old_pen = SelectObject(dis.hDC, pen);
        let old_brush = SelectObject(
            dis.hDC,
            windows::Win32::Graphics::Gdi::GetStockObject(windows::Win32::Graphics::Gdi::NULL_BRUSH),
        );
        let _ = RoundRect(
            dis.hDC,
            dis.rcItem.left + px(k, 2.0),
            dis.rcItem.top + px(k, 2.0),
            dis.rcItem.right - px(k, 2.0),
            dis.rcItem.bottom - px(k, 2.0),
            px(k, 6.0),
            px(k, 6.0),
        );
        SelectObject(dis.hDC, old_brush);
        SelectObject(dis.hDC, old_pen);
        let _ = DeleteObject(pen);
    }
    let h = dis.rcItem.bottom - dis.rcItem.top;
    let track_w = px(k, 44.0);
    let track_h = px(k, 24.0);
    let top = dis.rcItem.top + (h - track_h) / 2;
    let track = windows::Win32::Foundation::RECT {
        left: dis.rcItem.right - track_w,
        top,
        right: dis.rcItem.right,
        bottom: top + track_h,
    };
    let track_brush = CreateSolidBrush(COLORREF(if on { C_ACCENT } else { 0x00606060 }));
    round_fill(dis.hDC, &track, track_brush, track_h);
    let _ = DeleteObject(track_brush);
    let knob = px(k, 20.0);
    let kx = if on { track.right - knob - px(k, 2.0) } else { track.left + px(k, 2.0) };
    let knob_rc = windows::Win32::Foundation::RECT {
        left: kx,
        top: top + px(k, 2.0),
        right: kx + knob,
        bottom: top + px(k, 2.0) + knob,
    };
    let knob_brush = CreateSolidBrush(COLORREF(0x00FFFFFF));
    round_fill(dis.hDC, &knob_rc, knob_brush, knob);
    let _ = DeleteObject(knob_brush);
    // 文案
    let mut buf = [0u16; 128];
    let n = GetWindowTextW(dis.hwndItem, &mut buf);
    let caption = String::from_utf16_lossy(&buf[..n as usize]);
    let font = font(-px(k, 13.0), 400);
    let mut tr = windows::Win32::Foundation::RECT {
        left: dis.rcItem.left,
        top: dis.rcItem.top,
        right: track.left - px(k, 12.0),
        bottom: dis.rcItem.bottom,
    };
    text_at(dis.hDC, font, &caption, &mut tr, C_TEXT, false);
    let _ = DeleteObject(font);
}

fn is_list(id: usize) -> bool {
    matches!(id, ID_LIST | ID_HIST_LIST | ID_POLISH_LIST)
}

unsafe fn draw_list_item(dis: &DRAWITEMSTRUCT) {
    let k = GetDpiForWindow(dis.hwndItem) as f32 / 96.0;
    let (bg, fg) = if dis.itemState.0 & windows::Win32::UI::Controls::ODS_SELECTED.0 != 0 {
        (0x00553A2B, 0x00FFFFFF)
    } else {
        (C_EDIT, C_TEXT)
    };
    let bg_brush = CreateSolidBrush(COLORREF(bg));
    FillRect(dis.hDC, &dis.rcItem, bg_brush);
    let _ = DeleteObject(bg_brush);
    let mut buf = [0u16; 512];
    let n = SendMessageW(
        dis.hwndItem,
        LB_GETTEXT,
        WPARAM(dis.itemID as usize),
        LPARAM(buf.as_mut_ptr() as isize),
    )
    .0;
    if n > 0 {
        let text = String::from_utf16_lossy(&buf[..n as usize]);
        let font = font(-px(k, 13.0), 400);
        let mut tr = dis.rcItem;
        tr.left += px(k, 10.0);
        tr.right -= px(k, 6.0);
        text_at(dis.hDC, font, &text, &mut tr, fg, false);
        let _ = DeleteObject(font);
    }
}

unsafe fn draw_button(dis: &DRAWITEMSTRUCT) {
    let id = dis.CtlID as usize;
    let primary = is_primary(id) || segment_on(id).unwrap_or(false);
    let mut buf = [0u16; 128];
    let n = GetWindowTextW(dis.hwndItem, &mut buf);
    let caption = String::from_utf16_lossy(&buf[..n as usize]);
    let k = GetDpiForWindow(dis.hwndItem) as f32 / 96.0;
    let rc = dis.rcItem;
    let fill = if primary { C_ACCENT } else { C_EDIT };
    let btn_brush = CreateSolidBrush(COLORREF(fill));
    round_fill(dis.hDC, &rc, btn_brush, px(k, 8.0));
    let _ = DeleteObject(btn_brush);
    if !primary {
        // 次要按钮：细边框
        let pen = windows::Win32::Graphics::Gdi::CreatePen(
            windows::Win32::Graphics::Gdi::PS_SOLID,
            1,
            COLORREF(C_LINE),
        );
        let old_pen = SelectObject(dis.hDC, pen);
        let old_brush = SelectObject(
            dis.hDC,
            windows::Win32::Graphics::Gdi::GetStockObject(windows::Win32::Graphics::Gdi::NULL_BRUSH),
        );
        let _ = RoundRect(dis.hDC, rc.left, rc.top, rc.right, rc.bottom, px(k, 8.0), px(k, 8.0));
        SelectObject(dis.hDC, old_brush);
        SelectObject(dis.hDC, old_pen);
        let _ = DeleteObject(pen);
    }
    // 键盘焦点：细描边（去掉系统默认虚线框后，这是唯一能看出焦点在哪的地方）。
    // 主操作本身是蓝底，焦点描边得用白才看得见。
    let focused = dis.itemState.0 & windows::Win32::UI::Controls::ODS_FOCUS.0 != 0
        || GetFocus() == dis.hwndItem;
    if focused {
        let pen = windows::Win32::Graphics::Gdi::CreatePen(
            windows::Win32::Graphics::Gdi::PS_SOLID,
            2,
            COLORREF(if primary { 0x00FFFFFF } else { C_ACCENT }),
        );
        let old_pen = SelectObject(dis.hDC, pen);
        let old_brush = SelectObject(
            dis.hDC,
            windows::Win32::Graphics::Gdi::GetStockObject(windows::Win32::Graphics::Gdi::NULL_BRUSH),
        );
        let _ = RoundRect(
            dis.hDC,
            rc.left + px(k, 2.0),
            rc.top + px(k, 2.0),
            rc.right - px(k, 2.0),
            rc.bottom - px(k, 2.0),
            px(k, 7.0),
            px(k, 7.0),
        );
        SelectObject(dis.hDC, old_brush);
        SelectObject(dis.hDC, old_pen);
        let _ = DeleteObject(pen);
    }
    let font = font(-px(k, 13.0), 500);
    let mut tr = rc;
    text_at(
        dis.hDC,
        font,
        &caption,
        &mut tr,
        if primary { 0x00FFFFFF } else { C_TEXT },
        true,
    );
    let _ = DeleteObject(font);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_POLISH_MODELS_DONE => {
            rebuild_page();
            LRESULT(0)
        }
        WM_POLISH_DONE => {
            rebuild_page();
            LRESULT(0)
        }
        WM_DIAG_DONE => {
            rebuild_page();
            LRESULT(0)
        }
        WM_VOICE_DONE => {
            let res = crate::lock(&VOICE_RESULT).take();
            match res {
                Some((0, _)) => {
                    set_config_quiet(serde_json::json!({"voice_lock": true}));
                    crate::report("VOICE 登记成功，voice_lock=true");
                    crate::tray::set_tip("字在：声纹已登记");
                }
                Some((_, e)) => {
                    crate::report(&format!("VOICE 登记失败 {e}"));
                    crate::message_box(&format!("声纹登记失败：{e}"));
                }
                None => {}
            }
            rebuild_page();
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let hit = TIPS.lock().ok().and_then(|t| {
                t.iter()
                    .find(|(lx, ly, lw, lh, _)| x >= *lx && x <= lx + lw && y >= *ly && y <= ly + lh)
                    .map(|(lx, ly, _, _, text)| (text.clone(), *lx, *ly))
            });
            let mut need = false;
            if let Ok(mut g) = WIN.lock() {
                if let Some(w) = g.as_mut() {
                    let same = match (&w.hover, &hit) {
                        (Some((a, _, _)), Some((b, _, _))) => a == b,
                        (None, None) => true,
                        _ => false,
                    };
                    if !same {
                        w.hover = hit.clone();
                        need = true;
                    }
                }
            }
            if need {
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as usize;
            let note = ((wparam.0 >> 16) & 0xFFFF) as u32;
            // 输入框/下拉框拿到或失去焦点：重画那一圈强调色外框
            if matches!(note, 0x0100 | 0x0200 | 0x0003 | 0x0004) {
                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hwnd, None, false);
            }
            let _t0 = std::time::Instant::now();
            // 自绘开关的翻转必须发生在具体分支之前：分支里（如「记录历史」「只认我的声音」）
            // 要读翻转后的状态再落盘，否则写回去的永远是旧值。
            if is_toggle(id) && note == 0 {
                // 打开润色前先问一次（对齐 mac 的「这段文字会离开本机」）
                if id == ID_POLISH_EN && !toggle_on(ID_POLISH_EN) && !polish_consent() {
                    return LRESULT(0);
                }
                let _ = flip_toggle(id);
                // 打开「标点 / 降噪 / 只认我的声音」时，模型没装就顺手下载：
                // 用户点开关的意图就是「我要这个效果」，不该再让他去引擎页手动找。
                if toggle_on(id) {
                    match id {
                        ID_PUNCT => auto_pull_if_missing("punct", "标点模型"),
                        ID_DENOISE => auto_pull_if_missing("gtcrn", "降噪模型"),
                        ID_VOICE_LOCK => auto_pull_if_missing("speaker", "声纹模型"),
                        _ => {}
                    }
                }
            }
            match id {
                ID_NAV if note == LBN_SELCHANGE as u32 => {
                    let sel = with_win(|w| {
                        w.ctrls
                            .first()
                            .map(|h| unsafe { SendMessageW(*h, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as usize })
                    })
                    .flatten();
                    if let Some(p) = sel {
                        let leave_mic = {
                            let mut g = crate::lock(&WIN);
                            match g.as_mut() {
                                Some(w) => {
                                    let leave = w.page == 2 && p != 2 && w.meter_on;
                                    if leave {
                                        w.meter_on = false;
                                    }
                                    if w.page != p {
                                        w.page = p;
                                    }
                                    leave
                                }
                                None => false,
                            }
                        };
                        if leave_mic {
                            if let Some(a) = crate::lock(&crate::APP).as_ref() {
                                a.core.meter_stop();
                            }
                        }
                        if p == 3 {
                            refresh_suggestions();
                        }
                        rebuild_page();
                    }
                }
                ID_CLOSE_HEADER => {
                    ShowWindow(hwnd, SW_HIDE);
                }
                ID_SAVE => {
                    let page = with_win(|w| w.page).unwrap_or(0);
                    if page == 0 {
                        // 先把「按应用覆盖」的文本抓在手里：save_general() 末尾会 rebuild_page()，
                        // 那一句会把 e_perapp 换成新建的空编辑框，之后再读就是空的（实测会把用户的
                        // per_app 覆盖成 {}）。
                        let per_app = with_win(|w| w.e_perapp.map(text_of)).flatten().unwrap_or_default();
                        save_general();
                        save_per_app_text(&per_app);
                    } else if page == 2 {
                        save_audio();
                    }
                }
                ID_USE => {
                    let page = with_win(|w| w.page).unwrap_or(0);
                    if page == 1 {
                        if engine_selected_is_aux() {
                            crate::tray::set_tip("字在：这是附加模型，用「下载选中模型」装它");
                        } else {
                            switch_engine();
                        }
                    } else if page == 2 {
                        save_audio();
                    }
                }
                n if (ID_POLISH_PRESET..ID_POLISH_PRESET + 6).contains(&n) => {
                    apply_polish_preset(n - ID_POLISH_PRESET)
                }
                ID_SAVE_TOML => save_toml(),
                ID_SAVE_HW => save_hotwords(),
                ID_HW_ADD => add_suggested_hotword(),
                ID_OBS_ADOPT => adopt_learned(),
                ID_OBS_IGNORE => ignore_learned(),
                ID_HW_REFRESH => {
            refresh_suggestions();
            rebuild_page();
        }
                ID_PROXY_SAVE => save_text_field(401, "model_proxy", "下载代理已保存"),
                ID_ENGINE_SAVE => {
                    save_text_field(400, "model_mirror", "下载地址已保存");
                    save_text_field(401, "model_proxy", "下载代理已保存");
                }
                ID_MIRROR_SAVE => save_text_field(400, "model_mirror", "下载地址已保存"),
                ID_HIST_REFRESH => rebuild_page(),
                ID_HIST_COPY => copy_history(),
                ID_HIST_CLEAR => clear_history(),
                ID_HIST_EN => {
                    let on = with_win(|w| w.c_hist_on.map(checked)).flatten().unwrap_or(true);
                    let json = serde_json::json!({ "history_enabled": on }).to_string();
                    {
                        let app = crate::lock(&crate::APP);
                        if let Some(a) = app.as_ref() {
                            let (c, e) = a.core.set_config(&json);
                            crate::report(&format!("HIST enabled={on} code={c} err={e}"));
                        }
                    }
                    rebuild_page();
                }
                ID_HIST_DEL => delete_selected_history(),
                ID_HIST_RETRY => retry_failed_segments(),
                ID_POLISH_SAVE => save_polish(),
                ID_POLISH_ACT => {
                    if polish_consent() {
                        activate_profile();
                        // 槽位和「用」合并成一个动作：填了就一起绑上，省掉一个「保存槽」按钮
                        let slot = with_win(|w| w.e_slot.map(text_of)).flatten().unwrap_or_default();
                        if !slot.trim().is_empty() {
                            save_slot();
                        }
                    }
                }
                ID_POLISH_SLOT => save_slot(),
                ID_POLISH_PROMPT_SAVE => save_prompt(),
                ID_MIC_TEST => {
                    let on = with_win(|w| w.meter_on).unwrap_or(false);
                    let next = !on;
                    if let Some(w) = crate::lock(&WIN).as_mut() {
                        w.meter_on = next;
                    }
                    {
                        let app = crate::lock(&crate::APP);
                        if let Some(a) = app.as_ref() {
                            if next {
                                a.core.meter_start();
                            } else {
                                a.core.meter_stop();
                            }
                        }
                    }
                    rebuild_page();
                }
                ID_PULL => pull_model(),
                ID_COPY => {
                    let text = with_win(|w| w.e_toml.map(text_of)).flatten().unwrap_or_default();
                    crate::inject_copy(&text);
                    crate::tray::set_tip("字在：配置已复制");
                }
                ID_DIAG_SAVE => {
                    let snap = crate::diag::snapshot();
                    let msg = match snap.save() {
                        Ok(p) => {
                            crate::report(&format!("DIAG 已保存 {}", p.display()));
                            format!("已保存：{}", p.display())
                        }
                        Err(e) => {
                            crate::report(&format!("DIAG 保存失败：{e}"));
                            e
                        }
                    };
                    *crate::lock(&DIAG_RESULT) = msg;
                    rebuild_page();
                }
                ID_DIAG_UPLOAD => {
                    let snap = crate::diag::snapshot();
                    let body = snap.bundle();
                    let saved = match snap.save() {
                        Ok(p) => format!("已保存：{}", p.display()),
                        Err(e) => format!("本地保存失败：{e}"),
                    };
                    crate::report(&format!("DIAG 准备上传：{saved}"));
                    let up = {
                        let app = crate::lock(&crate::APP);
                        app.as_ref().map(|a| a.core.uploader())
                    };
                    let url = snap.upload_url.clone();
                    match up {
                        Some(up) => {
                            std::thread::spawn(move || {
                                let msg = match crate::diag::upload_body(up, url, body) {
                                    Ok(m) => format!("{saved} · {m}"),
                                    Err(e) => format!("{saved} · 上传失败：{e}"),
                                };
                                crate::report(&format!("DIAG {msg}"));
                                *crate::lock(&DIAG_RESULT) = msg;
                                let hwnd = crate::lock(&WIN).as_ref().map(|w| w.hwnd);
                                if let Some(h) = hwnd {
                                    unsafe {
                                        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                                            h,
                                            WM_DIAG_DONE,
                                            WPARAM(0),
                                            LPARAM(0),
                                        );
                                    }
                                }
                            });
                        }
                        None => {
                            *crate::lock(&DIAG_RESULT) = format!("{saved} · 核心没起来，这次没法上传");
                            rebuild_page();
                        }
                    }
                }
                ID_COPY_DIAG => {
                    // 用户报「不能用」时，这一段就是他要发出来的东西
                    let text = with_win(|w| w.e_diag.map(text_of)).flatten().unwrap_or_default();
                    crate::inject_copy(&text);
                    crate::tray::set_tip("字在：状态已复制，可以直接发给我");
                }
                ID_OPENLOG => {
                    let path = format!("{}/eryu-cli.log", crate::config_dir());
                    let _ = std::fs::OpenOptions::new().create(true).append(true).open(&path);
                    unsafe {
                        let p = wide(&path);
                        windows::Win32::UI::Shell::ShellExecuteW(
                            None,
                            windows::core::w!("open"),
                            PCWSTR(p.as_ptr()),
                            None,
                            None,
                            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                        );
                    }
                }
                ID_PERAPP_SAVE => save_per_app(),
                ID_HOTKEY_PRESET_ALT => save_hotkey_preset("ALT_R"),
                ID_HOTKEY_PRESET_CAS => save_hotkey_preset("CTRL+ALT+SPACE"),
                ID_HOTKEY_PRESET_F13 => save_hotkey_preset("F13"),
                ID_HOTKEY_CLEAR => save_hotkey_preset(""),
                ID_MODE_PASTE | ID_MODE_TYPE => {
                    if let Ok(mut g) = WIN.lock() {
                        if let Some(w) = g.as_mut() {
                            w.mode_paste = id == ID_MODE_PASTE;
                        }
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                }
                ID_POLISH_TEST => polish_test_async(),
                ID_POLISH_MODELS => polish_models_async(),
                ID_MIRROR_OPEN => {
                    let url = "https://officeace.harmonyos.fun/eryu-models/";
                    unsafe {
                        use windows::Win32::UI::Shell::ShellExecuteW;
                        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
                        let op = wide("open");
                        let u = wide(url);
                        ShellExecuteW(None, PCWSTR(op.as_ptr()), PCWSTR(u.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
                    }
                    crate::report("ENGINE 打开模型来源");
                }
                ID_LAN_GEN => {
                    // 一键生成 token（R-132）：写进令牌框并立刻保存，省得用户自己想
                    let t = rand_hex(32);
                    let h = with_win(|w| w.e_lan_token).flatten();
                    if let Some(h) = h {
                        unsafe {
                            let _ = SetWindowTextW(h, PCWSTR(wide(&t).as_ptr()));
                        }
                    }
                    save_lan();
                    crate::report("LAN 已生成令牌并保存");
                }
                ID_LAN_SAVE => save_lan(),
                ID_LAN_REFRESH => rebuild_page(),
                ID_VOICE_ENROLL => enroll_voice_async(),
                ID_VOICE_CLEAR => {
                    let rc = {
                        let app = crate::lock(&crate::APP);
                        app.as_ref().map(|a| a.core.voice_clear())
                    };
                    match rc {
                        Some((0, _)) => {
                            set_config_quiet(serde_json::json!({"voice_lock": false}));
                            crate::report("VOICE 已清空声纹，voice_lock=false");
                            rebuild_page();
                        }
                        Some((c, e)) => crate::message_box(&format!("清空失败（{c}）：{e}")),
                        None => {}
                    }
                }
                ID_VOICE_LOCK => {
                    let on = toggle_on(ID_VOICE_LOCK);
                    set_config_quiet(serde_json::json!({ "voice_lock": on }));
                }
                ID_SELFTEST => run_selftest(),
                ID_MICPRIV => {
                    let _ = std::process::Command::new("cmd")
                        .args(["/c", "start", "", "ms-settings:privacy-microphone"])
                        .spawn();
                }
                ID_OPENDIR => {
                    let dir = crate::config_dir();
                    let _ = std::process::Command::new("explorer").arg(dir).spawn();
                }
                _ => {}
            }
            if is_toggle(id) && note == 0 {
                // 开关状态已在 match 之前翻转，这里只做「立刻生效」和重画
                if id == ID_POLISH_EN {
                    save_polish();
                }
                if let Some(h) = with_win(|w| {
                    w.ctrls
                        .iter()
                        .copied()
                        .find(|h| unsafe { GetDlgCtrlID(*h) } as usize == id)
                })
                .flatten()
                {
                    unsafe {
                        InvalidateRect(h, None, true);
                    }
                }
            }
            let _ms = _t0.elapsed().as_millis();
            if _ms > 50 || std::env::var("ERYU_CMD_DEBUG").is_ok() {
                crate::report(&format!("CMD id={id} note={note} ms={_ms}"));
            }
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => {
            // 静态文本：卡片底色 + 浅色字
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut core::ffi::c_void);
            SetTextColor(hdc, COLORREF(C_TEXT));
            SetBkMode(hdc, OPAQUE);
            SetBkColor(hdc, COLORREF(C_CARD));
            let b = cached_brush(1);
            LRESULT(b.0 as isize)
        }
        WM_PAINT => {
            // 只画底色/卡片/导航；不再额外描「焦点环」——它是在窗口 DC 上按控件屏幕
            // 坐标反推矩形画出来的，跨页/重建后容易留下一圈对不上控件的蓝线（I-105）。
            paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        // 用 GetAsyncKeyState：全局物理键状态，真实键盘和自测注入都能看到
        WM_KEYDOWN if wparam.0 as u32 == 0x09 && unsafe { GetAsyncKeyState(0x11) < 0 } => {
            // Ctrl+Tab / Ctrl+Shift+Tab：切设置页（左侧导航是自绘的，没法用 Tab 走到）
            let back = unsafe { GetAsyncKeyState(0x10) < 0 };
            let cur = with_win(|w| w.page).unwrap_or(0);
            let n = PAGES.len();
            let next = if back { (cur + n - 1) % n } else { (cur + 1) % n };
            switch_page(next);
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 as u32 == 0x09 => {
            // Tab / Shift+Tab：无边框普通窗口不会自动做焦点遍历，这里自己走一遍
            unsafe {
                let back = GetKeyState(0x10) < 0; // VK_SHIFT
                if let Ok(next) =
                    GetNextDlgTabItem(hwnd, GetFocus(), windows::Win32::Foundation::BOOL::from(back))
                {
                    let _ = SetFocus(next);
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 as u32 == 0x0D => {
            // Enter：按下当前有焦点的按钮（普通窗口没有「默认按钮」概念，这里补上）
            unsafe {
                let f = GetFocus();
                let mut buf = [0u16; 16];
                let n = GetClassNameW(f, &mut buf);
                if String::from_utf16_lossy(&buf[..n as usize]) == "Button" {
                    let _ = PostMessageW(f, 0x00F5, WPARAM(0), LPARAM(0));
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN
            if (0x31..=0x39).contains(&(wparam.0 as u32))
                && unsafe { GetAsyncKeyState(0x11) < 0 } =>
        {
            // Ctrl+1..9：直接跳到第 N 页
            switch_page((wparam.0 as usize) - 0x31);
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 as u32 == 0x1B => {
            // Windows 习惯：Esc 关对话框（无边框窗口不是标准 dialog，得自己接）
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_SYSCOMMAND if (wparam.0 & 0xFFF0) == 0xF060 => {
            // Alt+F4 也走隐藏，别把窗口真销毁（否则下次要从头 build）
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_NCHITTEST => {
            let mut pt = windows::Win32::Foundation::POINT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt);
            let mut origin = windows::Win32::Foundation::POINT::default();
            let _ = crate::ui::client_to_screen(hwnd, &mut origin);
            let k = WIN.lock().ok().and_then(|g| g.as_ref().map(|w| w.k)).unwrap_or(1.0);
            if pt.y - origin.y < crate::ui::px(k, HEADER) {
                return LRESULT(windows::Win32::UI::WindowsAndMessaging::HTCAPTION as isize);
            }
            LRESULT(windows::Win32::UI::WindowsAndMessaging::HTCLIENT as isize)
        }
        WM_DRAWITEM => {
            let dis = &*(lparam.0 as *const DRAWITEMSTRUCT);
            if is_toggle(dis.CtlID as usize) {
                draw_toggle(dis);
            } else if is_list(dis.CtlID as usize) {
                draw_list_item(dis);
            } else {
                draw_button(dis);
            }
            LRESULT(1)
        }
        WM_MEASUREITEM => {
            let mis = &mut *(lparam.0 as *mut windows::Win32::UI::Controls::MEASUREITEMSTRUCT);
            let k = GetDpiForWindow(hwnd) as f32 / 96.0;
            mis.itemHeight = px(k, 26.0).max(18) as u32;
            LRESULT(1)
        }
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let hit = NAV
                .lock()
                .ok()
                .and_then(|g| {
                    g.iter()
                        .position(|(l, t, r, b)| x >= *l && x <= *r && y >= *t && y <= *b)
                });
            if let Some(p) = hit {
                switch_page(p);
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            // 换到不同缩放的屏幕：重建字体与布局（原来只算一次，用户看到的就是「字破碎」）
            let dpi = (wparam.0 & 0xFFFF) as u32;
            let k = dpi as f32 / 96.0;
            if let Ok(mut g) = WIN.lock() {
                if let Some(w) = g.as_mut() {
                    let old_font = w.font;
                    let old_section = w.font_section;
                    let old_title = w.font_title;
                    w.k = k.max(1.0);
                    w.font = ui_font(k, crate::ui::BODY, 400);
                    w.font_section = ui_font(k, crate::ui::SECTION, 600);
                    w.font_title = ui_font(k, crate::ui::TITLE, 600);
                    unsafe {
                        let _ = DeleteObject(old_font);
                        let _ = DeleteObject(old_section);
                        let _ = DeleteObject(old_title);
                    }
                }
            }
            let hint = &*(lparam.0 as *const windows::Win32::Foundation::RECT);
            SetWindowPos(
                hwnd,
                HWND::default(),
                hint.left,
                hint.top,
                hint.right - hint.left,
                hint.bottom - hint.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            build(hwnd);
            InvalidateRect(hwnd, None, true);
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut core::ffi::c_void);
            SetTextColor(hdc, COLORREF(C_TEXT));
            SetBkColor(hdc, COLORREF(C_EDIT));
            let b = cached_brush(0);
            LRESULT(b.0 as isize)
        }
        WM_CTLCOLORLISTBOX => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut core::ffi::c_void);
            SetTextColor(hdc, COLORREF(C_TEXT));
            SetBkColor(hdc, COLORREF(C_EDIT));
            let b = cached_brush(0);
            LRESULT(b.0 as isize)
        }
        WM_CTLCOLORBTN => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut core::ffi::c_void);
            SetTextColor(hdc, COLORREF(C_TEXT));
            SetBkMode(hdc, TRANSPARENT);
            let b = cached_brush(1);
            LRESULT(b.0 as isize)
        }
        WM_CLOSE => {
            ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_DESTROY => {
            // 窗口销毁前把电平测试停掉，别把麦克风一直占着
            {
                let was_on = WIN
                    .lock()
                    .ok()
                    .and_then(|g| g.as_ref().map(|w| w.meter_on))
                    .unwrap_or(false);
                if was_on {
                    if let Ok(app) = crate::APP.lock() {
                        if let Some(a) = app.as_ref() {
                            a.core.meter_stop();
                        }
                    }
                }
            }
            let mut g = crate::lock(&WIN);
            if let Some(w) = g.take() {
                unsafe {
                    DeleteObject(w.font);
                    DeleteObject(w.font_title);
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 自测：逐个量「打开某个分页要多久」以及各 FFI 读取耗时。
/// 用户反馈「按钮点下去卡死」，这里把每条读路径的毫秒数打出来，定位是谁在阻塞。
pub fn self_test_perf() -> String {
    let mut out = String::new();
    for page in 0..PAGES.len() {
        let t = std::time::Instant::now();
        open_at(page);
        std::thread::sleep(std::time::Duration::from_millis(120));
        let rebuild = std::time::Instant::now();
        rebuild_page();
        let rebuild_ms = rebuild.elapsed().as_millis();
        out.push_str(&format!(
            "PERF page={} {} 开窗={}ms 重建={}ms\n",
            page,
            PAGES[page],
            t.elapsed().as_millis() - rebuild_ms,
            rebuild_ms
        ));
    }
    let app_json = |f: &dyn Fn(&crate::App) -> String| -> (u128, usize) {
        let t = std::time::Instant::now();
        let n = {
            let app = crate::lock(&crate::APP);
            match app.as_ref() {
                Some(a) => f(a).len(),
                None => 0,
            }
        };
        (t.elapsed().as_millis(), n)
    };
    for (name, f) in [
        ("models_json", (&|a: &crate::App| a.core.models_json()) as &dyn Fn(&crate::App) -> String),
        ("devices_json", &|a: &crate::App| a.core.devices_json()),
        ("stats_json", &|a: &crate::App| a.core.stats_json()),
        ("history_json", &|a: &crate::App| a.core.history_json(50)),
        ("profiles_json", &|a: &crate::App| a.core.profiles_json()),
        ("hotwords_json", &|a: &crate::App| a.core.hotwords_json()),
        ("config_toml", &|a: &crate::App| a.core.config_toml()),
    ] {
        let (ms, n) = app_json(f);
        out.push_str(&format!("PERF ffi {name}={ms}ms len={n}\n"));
    }
    out
}

/// 打开设置窗口并切到指定分页（截图核对用）
pub fn open_page(page: usize) {
    open_at(page);
}

/// 自测：打开设置窗口 → 把「最短录音」写成 ms → 走真实的保存路径 → 读回配置校验。
/// 用于真机上验证「点保存真的落盘」（跨进程点按钮受 UIPI 限制，进程内直接走同一函数）。
pub fn self_test_save(ms: i64) -> String {
    open();
    std::thread::sleep(std::time::Duration::from_millis(300));
    {
        let g = crate::lock(&WIN);
        let Some(w) = g.as_ref() else {
            return "SETTINGS-TEST FAIL 窗口没起来".to_string();
        };
        if let Some(h) = w.e_min {
            set_text(h, &ms.to_string());
        }
    }
    save_general();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let c = cfg();
    let got = c["min_record_ms"].as_i64().unwrap_or(-1);
    if got == ms {
        format!("SETTINGS-TEST OK min_record_ms={got}")
    } else {
        format!("SETTINGS-TEST FAIL 期望 {ms} 实际 {got}")
    }
}

fn switch_engine() {
    let (items, sel) = with_win(|w| {
        let h = w.lst;
        let sel = h.map(|h| list_sel(h)).unwrap_or(-1);
        (h, sel)
    })
    .unwrap_or((None, -1));
    let Some(list) = items else { return };
    if sel < 0 {
        crate::message_box("先在上面选一个模型");
        return;
    }
    let models: serde_json::Value = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => serde_json::from_str(&a.core.models_json()).unwrap_or(serde_json::Value::Null),
            None => serde_json::Value::Null,
        }
    };
    let Some(arr) = models.as_array() else { return };
    let Some(m) = arr.get(sel as usize) else {
        let _ = list;
        return;
    };
    let id = m["id"].as_str().unwrap_or("").to_string();
    let installed = m["installed"].as_bool().unwrap_or(false);
    if !installed {
        crate::message_box("这个模型还没下载，先点「下载选中模型」");
        return;
    }
    apply(serde_json::json!({ "engine": id }).to_string());
}

/// 自测：反复重建设置页，看 GDI / USER 对象有没有涨（0.8.1 审计里的画刷/字体泄漏）。
pub fn self_test_gdi_cycles(n: usize) -> String {
    use windows::Win32::System::Threading::{GetCurrentProcess, GetGuiResources};
    let counts = || unsafe {
        let p = GetCurrentProcess();
        (
            GetGuiResources(p, windows::Win32::System::Threading::GR_GDIOBJECTS),
            GetGuiResources(p, windows::Win32::System::Threading::GR_USEROBJECTS),
        )
    };
    open_at(0);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let (g0, u0) = counts();
    for i in 0..n {
        {
            let mut g = crate::lock(&WIN);
            if let Some(w) = g.as_mut() {
                w.page = i % PAGES.len();
            }
        }
        rebuild_page();
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    let (g1, u1) = counts();
    format!(
        "GDI before={g0} after={g1} delta={} USER before={u0} after={u1} delta={}",
        g1 as i64 - g0 as i64,
        u1 as i64 - u0 as i64
    )
}

pub fn is_open() -> bool {
    WIN.lock().map(|g| g.is_some()).unwrap_or(false)
}

fn pull_model() {
    let (id, sel) = with_win(|w| {
        let sel = w.lst.map(|h| list_sel(h)).unwrap_or(-1);
        let id = if sel >= 0 {
            w.model_ids.get(sel as usize).cloned().unwrap_or_default()
        } else {
            String::new()
        };
        (id, sel)
    })
    .unwrap_or((String::new(), -1));
    if sel < 0 || id.is_empty() {
        crate::message_box("先在上面选一个模型");
        return;
    }
    let code = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.pull_model(&id).0,
            None => -1,
        }
    };
    let msg = match code {
        0 => "开始下载，进度在托盘提示里看".to_string(),
        -3 => "未知模型".to_string(),
        -4 => "已经有一个下载在跑了".to_string(),
        -5 => "这个模型已经装好了，直接「使用选中模型」".to_string(),
        _ => format!("下载没能开始（{code}）"),
    };
    crate::message_box(&msg);
}

/// 打开一个「需要模型」的开关时，模型没装就顺手开始下载。
/// 之前是提示「去引擎页选中再点下载」——对普通用户等于让他别用（用户反馈 2026-11-21）。
fn auto_pull_if_missing(model_id: &str, what: &str) {
    let installed = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => serde_json::from_str::<serde_json::Value>(&a.core.models_json())
                .ok()
                .and_then(|v| {
                    v.as_array().and_then(|l| {
                        l.iter()
                            .find(|m| m["id"].as_str() == Some(model_id))
                            .map(|m| m["installed"].as_bool().unwrap_or(false))
                    })
                })
                .unwrap_or(true),
            None => true,
        }
    };
    if installed {
        return;
    }
    let code = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.pull_model(model_id).0,
            None => -1,
        }
    };
    match code {
        0 => crate::tray::set_tip(&format!("字在：正在后台下载{what}，装好会提示")),
        -4 => crate::tray::set_tip("字在：已经有一个下载在跑，等它完成"),
        _ => {}
    }
}

/// 全量自测：把设置页会写的每个域都走一遍「写 → 读回 → 校验」，最后恢复原值。
/// 目的不是测 UI 布局（那是截图的事），是测「点保存之后配置真的进去了」。
fn check_cfg(
    out: &mut Vec<String>,
    name: &str,
    patch: serde_json::Value,
    key: &str,
    want: serde_json::Value,
) -> bool {
    let (code, err) = {
        let app = crate::lock(&crate::APP);
        match app.as_ref() {
            Some(a) => a.core.set_config(&patch.to_string()),
            None => (-1, "core 没起来".to_string()),
        }
    };
    if code != 0 {
        out.push(format!("{name}=FAIL 写入被拒({err})"));
        return false;
    }
    let now = cfg();
    let got = now.get(key).cloned().unwrap_or(serde_json::Value::Null);
    if got == want {
        out.push(format!("{name}=OK"));
        true
    } else {
        out.push(format!("{name}=FAIL 期望 {want} 实际 {got}"));
        false
    }
}

pub fn self_test_all() -> String {
    let before = cfg();
    let mut out: Vec<String> = Vec::new();
    let mut ok = true;

    // 通用：最短录音 + 上屏方式
    if !check_cfg(&mut out, "min_record_ms", serde_json::json!({"min_record_ms": 450}), "min_record_ms", serde_json::json!(450)) { ok = false; }
    if !check_cfg(&mut out, "insert_mode", serde_json::json!({"insert_mode": "keystroke"}), "insert_mode", serde_json::json!("keystroke")) { ok = false; }
    if !check_cfg(&mut out, "insert_mode_back", serde_json::json!({"insert_mode": "paste"}), "insert_mode", serde_json::json!("paste")) { ok = false; }
    // 隐私开关（最容易只改了 UI 没落盘的）
    if !check_cfg(&mut out, "privacy_on", serde_json::json!({"privacy_mode": true}), "privacy_mode", serde_json::json!(true)) { ok = false; }
    if !check_cfg(&mut out, "privacy_off", serde_json::json!({"privacy_mode": false}), "privacy_mode", serde_json::json!(false)) { ok = false; }
    // 音频
    if !check_cfg(&mut out, "denoise_off", serde_json::json!({"denoise": false}), "denoise", serde_json::json!(false)) { ok = false; }
    if !check_cfg(&mut out, "denoise_on", serde_json::json!({"denoise": true}), "denoise", serde_json::json!(true)) { ok = false; }
    // 热词：写两条 → 读回条数 → 清空
    {
        let app = crate::lock(&crate::APP);
        if let Some(a) = app.as_ref() {
            let r = a.core.set_hotwords(r#"{"rules":[{"from":"测试错字","to":"测试正字"}]}"#);
            let n = a.core.hotwords_json().matches("测试错字").count();
            if r.0 == 0 && n == 1 {
                out.push("hotwords=OK".into());
            } else {
                ok = false;
                out.push(format!("hotwords=FAIL code={} 命中={n}", r.0));
            }
            let _ = a.core.set_hotwords(r#"{"rules":[]}"#);
        }
    }
    // 润色：只写端点/模型（不碰 Key），再读回
    if !check_cfg(&mut out, "polish_endpoint", serde_json::json!({"polish_endpoint": "https://api.deepseek.com"}), "polish_endpoint", serde_json::json!("https://api.deepseek.com")) { ok = false; }
    // 引擎与设备：写回原值，确认这两个域接受字符串
    let eng = before["engine"].as_str().unwrap_or("sense").to_string();
    let dev = before["device"].as_str().unwrap_or("").to_string();
    if !check_cfg(&mut out, "engine", serde_json::json!({"engine": eng.clone()}), "engine", serde_json::json!(eng)) { ok = false; }
    if !check_cfg(&mut out, "device", serde_json::json!({"device": dev.clone()}), "device", serde_json::json!(dev)) { ok = false; }

    // 还原：把最可能被改动的几项恢复
    let patch = serde_json::json!({
        "min_record_ms": before["min_record_ms"],
        "privacy_mode": before["privacy_mode"],
        "denoise": before["denoise"],
        "insert_mode": before["insert_mode"],
    });
    {
        let app = crate::lock(&crate::APP);
        if let Some(a) = app.as_ref() {
            let _ = a.core.set_config(&patch.to_string());
        }
    }
    format!(
        "SETTINGSLIVE {} {}",
        if ok { "OK" } else { "FAIL" },
        out.join(" ")
    )
}

/// 主窗口收到 meter 事件时转到设置页：只有音频页开着测试才刷新那行电平
pub fn on_meter(rms: f32) {
    let target = {
        let g = match WIN.try_lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match g.as_ref() {
            Some(w) if w.meter_on => w.meter_lbl,
            _ => None,
        }
    };
    let Some(h) = target else { return };
    const CELLS: usize = 14;
    let filled = ((rms * 1.6).clamp(0.0, 1.0) * CELLS as f32).round() as usize;
    let filled = filled.min(CELLS);
    let db = if rms > 0.000_01 { 20.0 * rms.log10() } else { -60.0 };
    let text = format!(
        "{}{}  约 {:.0} dB",
        "█".repeat(filled),
        "░".repeat(CELLS - filled),
        db
    );
    unsafe {
        let _ = SetWindowTextW(h, PCWSTR(wide(&text).as_ptr()));
    }
}

/// 自测用：把当前的建议列表打出来（走真实的历史解析路径）
pub fn debug_suggested_words() -> Vec<(String, usize)> {
    suggested_words()
}

/// 自测用：纯逻辑跑一遍（不依赖历史文件）
pub fn debug_suggest_selftest() -> Vec<(String, usize)> {
    let texts = vec![
        "我们下周把设计文档发给你".to_string(),
        "先看设计文档再聊细节".to_string(),
        "设计文档周末改完".to_string(),
        "好的收到".to_string(),
    ];
    suggest_from(&texts, &["设计".to_string()])
}
