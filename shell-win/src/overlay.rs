//! 浮层：分层窗口（per-pixel alpha）+ GDI 绘制。
//!
//! 视觉参数与 macOS 版对齐，并按屏幕 DPI 缩放：
//! 胶囊高 36、提示行 16 + 间距 4（总高 56）、圆角 = 胶囊高/2（真胶囊）、
//! 电平条 4 宽 2.5 距、左侧留 10、图标 22、正文 17.5、下划线 19、提示 12.5、
//! 宽度 190 起、420 封顶、按 24 台阶增长且只增不减。
//!
//! 正文比 mac 的 14pt 略大（mac 14pt ≈ 18.7px，这里取 17.5px 是因为胶囊本身
//! 比 mac 扁、留白更紧）；早先正文写 14px，在真机上明显比 mac 小一号。
//!
//! 为什么要自己做缩放：macOS 的「点」是逻辑单位，Windows 的像素在高 DPI 屏上
//! 是物理像素。全部按 `dpi/96` 乘一遍，才能在 100% / 150% / 200% 下看起来一样大。

use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

/// 每次会话的序号：延迟隐藏时用它判断「这期间有没有新会话开始」，
/// 否则上一次的延迟隐藏会把新会话的浮层提前收掉。
static HIDE_GEN: AtomicU64 = AtomicU64::new(0);
/// 「错误提示正在显示」到什么时候（I-157）：错误之后核心还会补一个 idle 状态事件，
/// 不挡住它，用户就只会看到一闪而过的普通胶囊。
static ERR_UNTIL: Mutex<Option<std::time::Instant>> = Mutex::new(None);
use std::sync::{Arc, Mutex};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::*;

unsafe extern "system" fn overlay_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

// ---- 与 macOS 对齐的基准尺寸（pt，96dpi 下等于像素）----
const CAPSULE_H: f32 = 36.0;
const HINT_H: f32 = 16.0;
const HINT_GAP: f32 = 4.0;
const TOTAL_H: f32 = CAPSULE_H + HINT_GAP + HINT_H;
const LEFT: f32 = 10.0;
const BARS: usize = 5;
const BAR_W: f32 = 4.0;
const BAR_GAP: f32 = 2.5;
const ICON: f32 = 22.0;
const RIGHT: f32 = 12.0;
const TEXT_PT: f32 = 17.5;
const DOTS_PT: f32 = 19.0;
const HINT_PT: f32 = 12.5;
const MIN_W: f32 = 190.0;
const MAX_W: f32 = 420.0;
const STEP: f32 = 24.0;

/// 一个会话内的浮层状态（渲染线程与 UI 线程共享）
struct State {
    text: String,
    phase: String,
    level: f32,
    /// 已录秒数（提示行显示「在听… 12s」）
    secs: f64,
    anchor: Option<(i32, i32)>,
    /// 本会话已长到的宽度（pt）：只增不减，新会话重置
    width_pt: f32,
    /// 提示行 / 图标用的档位名（取首字作为徽标）
    profile: String,
    profile_icon: String,
    dots_on: bool,
    tick: f32,
}

pub struct Overlay {
    hwnd: HWND,
    st: Arc<Mutex<State>>,
    /// 调试用：强制缩放（0 = 按屏幕真实 DPI）
    forced_scale: Arc<AtomicI32>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 当前锚点所在显示器的缩放比例（1.0 / 1.25 / 1.5 / 2.0 …）
fn scale_at(anchor: Option<(i32, i32)>, forced: i32) -> f32 {
    if forced > 0 {
        return forced as f32 / 100.0;
    }
    unsafe {
        let p = match anchor {
            Some((x, y)) => POINT { x, y },
            None => {
                // 没有锚点（没读到 caret/前台窗口）时按鼠标所在屏居中，与 mac 一致
                let mut mp = POINT::default();
                if windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut mp).is_ok() {
                    mp
                } else {
                    POINT { x: 0, y: 0 }
                }
            }
        };
        let mon = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_ok() && dx > 0 {
            return dx as f32 / 96.0;
        }
        1.0
    }
}

/// 锚点所在显示器的可用区域（不含任务栏），用于夹紧位置
pub(crate) fn work_area_at(anchor: Option<(i32, i32)>) -> RECT {
    unsafe {
        let p = match anchor {
            Some((x, y)) => POINT { x, y },
            None => {
                // 没有锚点（没读到 caret/前台窗口）时按鼠标所在屏居中，与 mac 一致
                let mut mp = POINT::default();
                if windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut mp).is_ok() {
                    mp
                } else {
                    POINT { x: 0, y: 0 }
                }
            }
        };
        let mon = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(mon, &mut mi).as_bool() {
            mi.rcWork
        } else {
            RECT { left: 0, top: 0, right: GetSystemMetrics(SM_CXSCREEN), bottom: GetSystemMetrics(SM_CYSCREEN) }
        }
    }
}

/// macOS 那边档位图标是 SF Symbol 名字；Windows 用 Segoe MDL2 Assets 的「编辑」图标。
/// 早先是取档名首字，结果「日常」这个档画出来就是一个汉字「日」，用户看到一脸问号——
/// 档位是「润色」的意思，铅笔图标比汉字直观。
fn icon_glyph(_symbol: &str, name: &str) -> String {
    if name.is_empty() { return String::new(); }
    "\u{E70F}".to_string()
}

/// Emoji 字形要用专门的字面，否则会画成豆腐块
/// 系统是浅色主题吗（默认按浅色处理）。只查一次，缓存在静态里。
/// 胶囊下方的提示行没有底色，需要按主题选字色。
/// 采样窗口下方一条桌面区域的亮度：提示行没有底色，靠它决定用深色还是浅色字。
/// 只看系统主题在「浅色主题 + 深色壁纸」（或反过来）时会误判，实测那块字会糊在壁纸上。
fn bg_luma(x: i32, y: i32, w: i32) -> Option<u8> {
    unsafe {
        use windows::Win32::Foundation::COLORREF;
        use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC, CLR_INVALID};
        let dc = GetDC(None);
        if dc.is_invalid() { return None; }
        let mut sum: u32 = 0;
        let mut n: u32 = 0;
        for i in 0..9 {
            let px = x + (w as f32 * (i as f32 + 0.5) / 9.0) as i32;
            let c: COLORREF = GetPixel(dc, px, y);
            if c.0 != CLR_INVALID {
                let r = (c.0 & 0xFF) as u32;
                let g = ((c.0 >> 8) & 0xFF) as u32;
                let b = ((c.0 >> 16) & 0xFF) as u32;
                // 感知亮度（近似 601 权重）
                sum += (r * 299 + g * 587 + b * 114) / 1000;
                n += 1;
            }
        }
        ReleaseDC(None, dc);
        if n == 0 { None } else { Some((sum / n) as u8) }
    }
}

fn system_is_light() -> bool {
    static CACHED: AtomicI32 = AtomicI32::new(-1);
    let v = CACHED.load(Ordering::Relaxed);
    if v >= 0 { return v == 1; }
    let light = unsafe {
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
        let sub = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        let name = wide("AppsUseLightTheme");
        let mut data: u32 = 1;
        let mut size = std::mem::size_of::<u32>() as u32;
        let rc = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut _),
            Some(&mut size),
        );
        // 键不存在时按浅色（绝大多数机器默认浅色）
        if rc.0 == 0 { data != 0 } else { true }
    };
    CACHED.store(if light { 1 } else { 0 }, Ordering::Relaxed);
    light
}

fn emoji_font(px: i32) -> HFONT {
    let face = wide("Segoe UI Emoji");
    unsafe {
        CreateFontW(
            px, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
            ANTIALIASED_QUALITY.0 as u32, DEFAULT_PITCH.0 as u32, PCWSTR(face.as_ptr()),
        )
    }
}

fn font(px: i32, weight: i32) -> HFONT {
    // 与设置页/迷你窗同一套字体：中文用雅黑 UI，西文也正常；权重交给调用方（胶囊正文用 500，
    // 不要 600——600 在屏幕上看就是「黑体加粗」，跟系统界面对不上）。
    let face = wide("Microsoft YaHei UI");
    unsafe {
        CreateFontW(
            px, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
            ANTIALIASED_QUALITY.0 as u32, DEFAULT_PITCH.0 as u32, PCWSTR(face.as_ptr()),
        )
    }
}

fn measure(s: &str, px: i32, weight: i32) -> i32 {
    if s.is_empty() {
        return 0;
    }
    unsafe {
        let dc = GetDC(None);
        let f = font(px, weight);
        let old = SelectObject(dc, f);
        let w = wide(s);
        let mut size = SIZE::default();
        let ok = GetTextExtentPoint32W(dc, &w[..w.len() - 1], &mut size);
        SelectObject(dc, old);
        let _ = DeleteObject(f);
        ReleaseDC(None, dc);
        if ok.as_bool() { size.cx } else { 0 }
    }
}

/// 从尾部保留能放进 `max_px` 的最长片段（保证最新内容永远可见）
/// 错误提示要给人看的是「开头那句」：太长时留前段加省略号，而不是像识别中间结果那样留尾巴。
fn fit_head(s: &str, max_px: i32, px: i32, weight: i32) -> String {
    if measure(s, px, weight) <= max_px {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let head: String = chars[..mid].iter().collect();
        if measure(&head, px, weight) + measure("…", px, weight) <= max_px { lo = mid } else { hi = mid - 1 }
    }
    let head: String = chars[..lo].iter().collect();
    format!("{head}…")
}

fn fit_tail(s: &str, max_px: i32, px: i32, weight: i32) -> String {
    if measure(s, px, weight) <= max_px {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        let tail: String = chars[mid..].iter().collect();
        if measure(&tail, px, weight) <= max_px { hi = mid } else { lo = mid + 1 }
    }
    chars[lo..].iter().collect()
}

/// 纯几何：把浮层放到「光标附近、但不越出工作区」的位置。
/// `work` 是该显示器的工作区（可含负坐标，多屏时左屏 left<0）。
fn place(anchor: Option<(i32, i32)>, w: i32, h: i32, work: RECT, k: f32) -> (i32, i32) {
    let px = |v: f32| -> i32 { (v * k).round() as i32 };
    match anchor {
        Some((ax, ay)) => {
            let mut cx = ax - w / 2;
            let below = ay + px(6.0);
            let flip = below + h > work.bottom - px(4.0);
            let cy = if flip { ay - h - px(28.0) } else { below };
            let min_x = work.left + px(8.0);
            let max_x = (work.right - w - px(8.0)).max(min_x);
            if cx < min_x { cx = min_x }
            if cx > max_x { cx = max_x }
            let min_y = work.top + px(4.0);
            let max_y = (work.bottom - h - px(4.0)).max(min_y);
            (cx, cy.clamp(min_y, max_y))
        }
        None => {
            let x = work.left + (work.right - work.left - w) / 2;
            let y = work.bottom - px(96.0) - h;
            (x, y.clamp(work.top + px(4.0), (work.bottom - h - px(4.0)).max(work.top)))
        }
    }
}

impl Overlay {
    pub fn new() -> Self {
        let hwnd = unsafe {
            let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
            let class = wide("EryuOverlay");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(overlay_wndproc),
                hInstance: windows::Win32::Foundation::HINSTANCE(hinst.0 as *mut core::ffi::c_void),
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let h = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TRANSPARENT,
                PCWSTR(class.as_ptr()),
                PCWSTR(wide("字在").as_ptr()),
                WS_POPUP,
                0, 0, 190, 56,
                None, None, hinst, None,
            );
            h.unwrap_or_default()
        };
        let st = Arc::new(Mutex::new(State {
            text: String::new(),
            phase: "idle".into(),
            secs: 0.0,
            level: 0.0,
            anchor: None,
            width_pt: MIN_W,
            profile: String::new(),
            profile_icon: String::new(),
            dots_on: true,
            tick: 0.0,
        }));
        let forced = Arc::new(AtomicI32::new(0));
        let ov = Self { hwnd, st: st.clone(), forced_scale: forced.clone() };
        // 30Hz 动画：电平条的波浪、下划线的闪动都在这里推进
        let hwnd_raw = hwnd.0 as isize;
        std::thread::spawn(move || {
            let mut blink = 0.0f32;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(33));
                let (phase, visible) = {
                    let s = crate::lock(&st);
                    (s.phase.clone(), !matches!(s.phase.as_str(), "idle"))
                };
                if !visible {
                    continue;
                }
                {
                    let mut s = crate::lock(&st);
                    s.tick += 0.033;
                    blink += 0.033;
                    if blink >= 0.5 {
                        blink = 0.0;
                        s.dots_on = !s.dots_on;
                    }
                    if !matches!(phase.as_str(), "listening" | "starting") {
                        continue;
                    }
                }
                render(HWND(hwnd_raw as _), &st, forced.load(Ordering::Relaxed));
            }
        });
        ov
    }

    /// 演示：只画浮层，不碰热键/注入（截图自测用）
    pub fn demo(&self, text: &str) {
        self.demo_phase(text, "listening");
    }

    /// 给截图/自查用的阶段样例：listening / processing / inserting / done
    pub fn demo_phase(&self, text: &str, phase: &str) {
        {
            let mut s = crate::lock(&self.st);
            s.phase = phase.into();
            s.anchor = crate::caret::caret_anchor().map(|a| (a.x, a.y));
            s.text = text.to_string();
            s.level = 0.7;
            s.width_pt = MIN_W;
            if s.profile.is_empty() {
                s.profile = "日常".into();
            }
        }
        render(self.hwnd, &self.st, self.forced_scale.load(Ordering::Relaxed));
        // 演示窗口留 9 秒给截图，然后自己退出（别留孤儿进程抢热键，I-060）
        std::thread::sleep(std::time::Duration::from_millis(9000));
    }

    pub fn set_forced_scale(&self, pct: i32) {
        self.forced_scale.store(pct, Ordering::Relaxed);
    }

    pub fn show(&self, phase: &str) {
        {
            // 用户重新开始一段（starting/listening）时，旧提示必须让位：
            // 否则「启动时提示上次没上屏」会在 3.2 秒内把第一段录音的胶囊吞掉（实测）。
            if matches!(phase, "starting" | "listening") {
                if let Ok(mut t) = ERR_UNTIL.lock() {
                    if t.map(|x| x > std::time::Instant::now()).unwrap_or(false) {
                        crate::report("OVERLAY 开始新一段，清掉旧提示");
                        *t = None;
                    }
                }
            }
            // 错误提示还在时效内：核心紧接着的 idle 状态不该把错误顶掉
            if phase != "error" {
                let hold = crate::lock(&ERR_UNTIL).map(|t| t > std::time::Instant::now()).unwrap_or(false);
                if hold {
                    crate::report(&format!("OVERLAY 保留错误提示，忽略状态 {phase}"));
                    return;
                }
            }
            let mut s = crate::lock(&self.st);
            s.phase = phase.to_string();
            if phase == "idle" {
                // 与 macOS 一致：最后一句先留 0.9 秒再隐藏（不然上屏后一闪就没）；
                // 期间若来了新会话，这代隐藏自动失效。
                let hwnd_raw = self.hwnd.0 as isize;
                let my_gen = HIDE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
                s.level = 0.0;
                // 打字/粘贴已经发生，提示行没意义了，只留最后一句
                s.phase = "done".into();
                drop(s);
                render(self.hwnd, &self.st, self.forced_scale.load(Ordering::Relaxed));
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(900));
                    if HIDE_GEN.load(Ordering::SeqCst) == my_gen {
                        unsafe { let _ = ShowWindow(HWND(hwnd_raw as _), SW_HIDE); }
                    }
                });
                return;
            }
            HIDE_GEN.fetch_add(1, Ordering::SeqCst);
            if phase == "starting" || phase == "listening" {
                s.text.clear();
                s.width_pt = MIN_W;
                // 每个新会话只算一次位置，录音过程中不跳
                s.anchor = crate::caret::caret_anchor().map(|a| (a.x, a.y));
            }
        }
        render(self.hwnd, &self.st, self.forced_scale.load(Ordering::Relaxed));
    }

    pub fn set_profile(&self, icon: &str, name: &str) {
        let mut st = crate::lock(&self.st);
        st.profile = name.to_string();
        st.profile_icon = icon.to_string();
    }

    pub fn set_partial(&self, text: &str) {
        crate::lock(&self.st).text = text.to_string();
        render(self.hwnd, &self.st, self.forced_scale.load(Ordering::Relaxed));
    }

    pub fn set_final(&self, text: &str) {
        {
            let mut s = crate::lock(&self.st);
            s.text = text.to_string();
            s.phase = "inserting".into();
        }
        render(self.hwnd, &self.st, self.forced_scale.load(Ordering::Relaxed));
        let hwnd_raw = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1200));
            unsafe { let _ = ShowWindow(HWND(hwnd_raw as _), SW_HIDE); }
        });
    }

    /// 引导期间用：立刻收起（不等 0.9 秒那套延迟隐藏）
    pub fn hide_now(&self) {
        HIDE_GEN.fetch_add(1, Ordering::SeqCst);
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// 电平 + 已录秒数一起更新（秒数只影响提示行，不单独触发重画）
    pub fn set_level_secs(&self, rms: f32, secs: f64) {
        {
            let mut s = crate::lock(&self.st);
            s.level = (s.level * 0.6 + rms * 0.4).clamp(0.0, 1.0);
            s.secs = secs;
        }
        render(self.hwnd, &self.st, self.forced_scale.load(Ordering::Relaxed));
    }

    pub fn set_level(&self, rms: f32) {
        {
            let mut s = crate::lock(&self.st);
            s.level = (s.level * 0.6 + rms * 0.4).clamp(0.0, 1.0);
        }
        render(self.hwnd, &self.st, self.forced_scale.load(Ordering::Relaxed));
    }

    pub fn error(&self, msg: &str) {
        HIDE_GEN.fetch_add(1, Ordering::SeqCst);
        {
            let mut s = crate::lock(&self.st);
            s.phase = "error".into();
            s.text = msg.to_string();
            // 第一帧就要够宽：否则用户看到的是一颗按最短宽度画出来、被截断的胶囊（I-157）。
            // 误差由 render 里的逐步加宽兜底；MAX_W 保证再长的错误也放得下开头那句。
            let est = 8.0 + msg.chars().count() as f32 * 13.0;
            s.width_pt = est.clamp(MIN_W, MAX_W);
            s.anchor = crate::caret::caret_anchor().map(|a| (a.x, a.y));
        }
        crate::report(&format!("OVERLAY 错误提示：{msg}"));
        *crate::lock(&ERR_UNTIL) = Some(std::time::Instant::now() + std::time::Duration::from_millis(3200));
        // 麦克风打不开这类错误发生在「浮层还没显示」之前：不先 Show 出来，用户按了热键只会看到
        // 什么都没发生（I-157）。SHOWNA 不抢焦点，不会打断用户正在打字的窗口。
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNA);
        }
        render(self.hwnd, &self.st, self.forced_scale.load(Ordering::Relaxed));
        let hwnd_raw = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            *crate::lock(&ERR_UNTIL) = None;
            unsafe { let _ = ShowWindow(HWND(hwnd_raw as _), SW_HIDE); }
        });
    }
}

/// 画一帧并推到屏幕上。所有尺寸先算 pt，再乘 DPI 缩放。
fn render(hwnd: HWND, st: &Arc<Mutex<State>>, forced: i32) {
    let (text, phase, level, secs, anchor, mut width_pt, profile, profile_icon, dots_on, tick) = {
        let s = crate::lock(&st);
        (s.text.clone(), s.phase.clone(), s.level, s.secs, s.anchor, s.width_pt, s.profile.clone(), s.profile_icon.clone(), s.dots_on, s.tick)
    };
    let k = scale_at(anchor, forced);
    let px = |v: f32| -> i32 { (v * k).round() as i32 };

    let text_px = px(TEXT_PT).max(9);
    let hint_px = px(HINT_PT).max(7);
    let total_h = px(TOTAL_H);
    let capsule_h = px(CAPSULE_H);
    let radius = capsule_h / 2;
    let left = px(LEFT);
    let right = px(RIGHT);
    let icon = px(ICON);
    let dots_w = measure("_", px(DOTS_PT).max(9), 600) + px(3.0);

    // ---- 宽度：只增不减、24pt 台阶、420 封顶 ----
    let text_x = left + px(BARS as f32 * (BAR_W + BAR_GAP) + 7.0) + icon + px(8.0);
    let need_pt = text_x as f32 / k + right as f32 / k + dots_w as f32 / k
        + measure(&text, text_px, 600) as f32 / k
        + px(2.0) as f32 / k;
    let stepped = ((need_pt / STEP).ceil() * STEP).clamp(MIN_W, MAX_W);
    if stepped > width_pt {
        width_pt = stepped;
        if let Ok(mut s) = st.lock() {
            s.width_pt = width_pt;
        }
    }
    let w = px(width_pt);

    // ---- 位置：跟光标，夹到该屏工作区，下方放不下翻上面 ----
    let work = work_area_at(anchor);
    let (x, y) = place(anchor, w, total_h, work, k);
    if std::env::var("ERYU_TRACE").is_ok() {
        crate::report(&format!(
            "OVERLAY 位置 anchor={anchor:?} x={x} y={y} w={w} h={total_h} 工作区={},{}-{},{}",
            work.left, work.top, work.right, work.bottom
        ));
    }

    unsafe {
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -total_h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let dib = match CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(d) => d,
            Err(_) => return,
        };
        let old = SelectObject(mem_dc, dib);
        let buf = bits as *mut u8;
        let n = (w * total_h) as usize;
        // 全黑起点：胶囊区稍后填色，提示行留黑以便从文字亮度算 alpha
        for i in 0..n {
            let p = buf.add(i * 4);
            *p = 0; *p.add(1) = 0; *p.add(2) = 0; *p.add(3) = 255;
        }

        // 1) 胶囊底色（macOS: srgb 0.22/0.22/0.24）
        for yy in 0..capsule_h {
            for xx in 0..w {
                let p = buf.add(((yy * w + xx) * 4) as usize);
                *p = 61; *p.add(1) = 56; *p.add(2) = 56; // BGR = 0.24,0.22,0.22
            }
        }

        // 2) 电平条：5 根，波浪高度与 macOS 一致（14–24pt 区间）
        let active = matches!(phase.as_str(), "listening" | "starting");
        // 声音大小 → 幅度：按 dB 映射（-60 dB 静音 → 0，-20 dB 以上 → 1），与 macOS 侧同口径。
        // 用 level*9 时小声说话几乎看不出差别，用户看不出「到底有没有收到声音」。
        let db = 20.0 * (level.max(1e-5)).log10();
        let amp = ((db + 60.0) / 40.0).clamp(0.0, 1.0);
        for i in 0..BARS {
            let wave = if active { ((tick * 6.0 + i as f32 * 0.9).sin() + 1.0) / 2.0 } else { 0.18 };
            // 始终有波形（说明在听），幅度跟真实音量走（说明有没有收到声音）
            let energy = if active { 0.14 + 0.86 * amp } else { 0.0 };
            let base = if active { energy * (0.35 + 0.65 * wave) } else { 0.12 };
            let h = if active { (6.0 + 14.0 * base).max(8.0) } else { 8.0 };
            let bh = px(h);
            let bx = left + px((i as f32) * (BAR_W + BAR_GAP));
            let bw = px(BAR_W).max(2);
            let by = (capsule_h - bh) / 2;
            let lum = if active { 0.55 + 0.45 * amp } else { 0.35 };
            for yy in by..(by + bh).min(capsule_h) {
                for xx in bx..(bx + bw).min(w) {
                    let p = buf.add(((yy * w + xx) * 4) as usize);
                    // #F8D088 = (248,208,136) → BGR 136,208,248
                    *p = (136.0 * lum) as u8;
                    *p.add(1) = (208.0 * lum) as u8;
                    *p.add(2) = (248.0 * lum) as u8;
                }
            }
        }

        // 3) 档位徽标：圆角方块 + 档名首字（macOS 用 SF Symbol，Windows 没有符号字体，用汉字更直观）
        let icon_x = left + px(BARS as f32 * (BAR_W + BAR_GAP) + 7.0);
        let icon_y = (capsule_h - icon) / 2;
        if !profile.is_empty() {
            let badge_r = px(5.0).max(2);
            for yy in icon_y..(icon_y + icon).min(capsule_h) {
                for xx in icon_x..(icon_x + icon).min(w) {
                    let cx = if xx < icon_x + badge_r { icon_x + badge_r } else if xx >= icon_x + icon - badge_r { icon_x + icon - badge_r - 1 } else { xx };
                    let cy = if yy < icon_y + badge_r { icon_y + badge_r } else if yy >= icon_y + icon - badge_r { icon_y + icon - badge_r - 1 } else { yy };
                    let corner = (xx < icon_x + badge_r || xx >= icon_x + icon - badge_r)
                        && (yy < icon_y + badge_r || yy >= icon_y + icon - badge_r);
                    if corner {
                        let dx = (xx - cx) as f32;
                        let dy = (yy - cy) as f32;
                        if dx * dx + dy * dy > (badge_r as f32) * (badge_r as f32) { continue; }
                    }
                    let p = buf.add(((yy * w + xx) * 4) as usize);
                    // 白色 18% 叠在底色上
                    *p = (*p as f32 * 0.82 + 255.0 * 0.18) as u8;
                    *p.add(1) = (*p.add(1) as f32 * 0.82 + 255.0 * 0.18) as u8;
                    *p.add(2) = (*p.add(2) as f32 * 0.82 + 255.0 * 0.18) as u8;
                }
            }
            let ch = icon_glyph(&profile_icon, &profile);
            // 档位图标是 MDL2 字符，必须用 Segoe MDL2 Assets 才有字形（雅黑里没有私有区字形）
            let glyph_face = wide("Segoe MDL2 Assets");
            let bf = unsafe {
                CreateFontW(
                    px(ICON * 0.62).max(9), 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET.0 as u32,
                    OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
                    ANTIALIASED_QUALITY.0 as u32, DEFAULT_PITCH.0 as u32, PCWSTR(glyph_face.as_ptr()),
                )
            };
            let oldb = SelectObject(mem_dc, bf);
            SetBkMode(mem_dc, TRANSPARENT);
            SetTextColor(mem_dc, COLORREF(0x00F0F0F0));
            let mut bt = wide(&ch);
            let mut br = RECT { left: icon_x, top: icon_y, right: icon_x + icon, bottom: icon_y + icon };
            DrawTextW(mem_dc, &mut bt, &mut br, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            SelectObject(mem_dc, oldb);
            let _ = DeleteObject(bf);
        }

        // 4) 正文 + 下划线（下划线闪动）；空文本时下划线居中
        SetBkMode(mem_dc, TRANSPARENT);
        let tf = font(text_px, 500);
        let old_font = SelectObject(mem_dc, tf);
        SetTextColor(mem_dc, COLORREF(0x00FFFFFF));
        let shown = if phase == "error" {
            format!("! {text}")
        } else {
            text.clone()
        };
        let reserved = dots_w + right;
        let avail = (w - text_x - reserved).max(px(10.0));
        if !shown.is_empty() {
            let fitted = if phase == "error" { fit_head(&shown, avail, text_px, 600) } else { fit_tail(&shown, avail, text_px, 600) };
            let mut t = wide(&fitted);
            let mut r = RECT { left: text_x, top: 0, right: w - reserved, bottom: capsule_h };
            DrawTextW(mem_dc, &mut t, &mut r, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        }
        SelectObject(mem_dc, old_font);
        let _ = DeleteObject(tf);

        // 下划线（默认 alpha 1.0，闪动时 0.25）
        let df = font(px(DOTS_PT).max(9), 600);
        let old_font2 = SelectObject(mem_dc, df);
        let alpha = if dots_on { 255.0 } else { 64.0 };
        SetTextColor(mem_dc, COLORREF((alpha as u32) << 24 | 0x00FFFFFF));
        let dots_x = if shown.is_empty() {
            (w - dots_w) / 2
        } else {
            let fitted = fit_tail(&shown, avail, text_px, 600);
            text_x + measure(&fitted, text_px, 600).min(avail) + px(1.0)
        };
        let mut d = wide("_");
        let mut dr = RECT { left: dots_x, top: 0, right: (dots_x + dots_w).min(w), bottom: capsule_h };
        DrawTextW(mem_dc, &mut d, &mut dr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        SelectObject(mem_dc, old_font2);
        let _ = DeleteObject(df);

        // 5) 提示行（胶囊下方，透明背景，靠亮度转 alpha）
        let secs_txt = if matches!(phase.as_str(), "starting" | "listening") && secs >= 1.0 {
            format!("在听… {}s · ", secs.round() as i64)
        } else {
            String::new()
        };
        let hint_listen = format!("{secs_txt}松开上屏 · Esc 取消");
        let hint = match phase.as_str() {
            "starting" | "listening" => hint_listen.as_str(),
            "processing" => "识别中…",
            "inserting" => "上屏中…",
            _ => "",
        };
        if !hint.is_empty() {
            let hf = font(hint_px, 400);
            let old_font3 = SelectObject(mem_dc, hf);
            let hy = capsule_h + px(HINT_GAP);
            // 提示行没有底色，只能靠字色适配桌面：先一律用白色画（后面按覆盖率染色），
            // 浅色主题染成深灰、深色主题染成浅灰。
            SetTextColor(mem_dc, COLORREF(0x00FFFFFF));
            let mut t = wide(hint);
            let mut r = RECT { left: 0, top: hy, right: w, bottom: hy + px(HINT_H) };
            DrawTextW(mem_dc, &mut t, &mut r, DT_CENTER | DT_TOP | DT_SINGLELINE);
            SelectObject(mem_dc, old_font3);
            let _ = DeleteObject(hf);
        }

        // 提示行颜色：优先按窗口正下方那块桌面的亮度选（浅壁纸→深字，深壁纸→浅字），
        // 采不到再退回系统主题。壁纸是浅色但主题是浅色/深色时都不会糊。
        let hint_tint: u32 = match bg_luma(x, y + total_h + 3, w) {
            Some(l) if l >= 128 => 0x33,
            Some(_) => 0xF2,
            None => if system_is_light() { 0x33 } else { 0xF2 },
        };

        // 6) 转预乘 alpha：胶囊内统一乘 240/255，提示行按像素亮度算 alpha，圆角外清 0
        let cap_alpha: u32 = if phase == "error" { 246 } else { 240 };
        for yy in 0..total_h {
            for xx in 0..w {
                let p = buf.add(((yy * w + xx) * 4) as usize);
                if yy < capsule_h {
                    // 圆角：左上/右上/左下/右下四个角按圆方程裁
                    let mut inside = true;
                    let cx = if xx < radius { radius } else if xx >= w - radius { w - radius - 1 } else { xx };
                    let cy = if yy < radius { radius } else if yy >= capsule_h - radius { capsule_h - radius - 1 } else { yy };
                    if (xx < radius || xx >= w - radius) && (yy < radius || yy >= capsule_h - radius) {
                        let dx = (xx - cx) as f32;
                        let dy = (yy - cy) as f32;
                        inside = dx * dx + dy * dy <= (radius as f32) * (radius as f32) + radius as f32;
                    }
                    if inside {
                        *p = ((*p as u32 * cap_alpha) / 255) as u8;
                        *p.add(1) = ((*p.add(1) as u32 * cap_alpha) / 255) as u8;
                        *p.add(2) = ((*p.add(2) as u32 * cap_alpha) / 255) as u8;
                        *p.add(3) = cap_alpha as u8;
                    } else {
                        *p = 0; *p.add(1) = 0; *p.add(2) = 0; *p.add(3) = 0;
                    }
                } else {
                    // 提示行：白色画出的笔画亮度就是覆盖率；再按主题把颜色染成深灰/浅灰，
                    // 并按预乘 alpha 写回（否则深色字算出来的 alpha 很低，等于看不见）。
                    // 12.5pt 小字在 1x 下抗锯齿会把笔画摊薄，覆盖率直接当 alpha 会发灰发虚；
                    // 放大约 1.5 倍（截顶 255）让笔画实一点。
                    // 覆盖率做一点 gamma 提升（x^1.7）：小字抗锯齿的边缘会被摊薄成灰雾，
                    // 直接当 alpha 会显得「发虚/重影」。提 gamma 让笔画实、边缘快速收干净。
                    let base = (*p).max(*p.add(1)).max(*p.add(2)) as f32 / 255.0;
                    let cov = (base.powf(1.7) * 255.0) as u32;
                    if cov < 24 {
                        *p = 0; *p.add(1) = 0; *p.add(2) = 0; *p.add(3) = 0;
                    } else {
                        let tint: u32 = hint_tint;
                        let c = ((tint * cov) / 255) as u8;
                        *p = c;
                        *p.add(1) = c;
                        *p.add(2) = c;
                        *p.add(3) = cov as u8;
                    }
                }
            }
        }

        let size = SIZE { cx: w, cy: total_h };
        let src = POINT { x: 0, y: 0 };
        let dst = POINT { x, y };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let ulw = UpdateLayeredWindow(hwnd, screen_dc, Some(&dst), Some(&size), mem_dc, Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
        if std::env::var("ERYU_RAW_DEBUG").is_ok() {
            use std::io::Write;
            let path = format!("{}/raw.log", crate::config_dir());
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let vis = IsWindowVisible(hwnd).as_bool();
                let _ = writeln!(f, "render w={w} h={total_h} pos=({x},{y}) scale={k} ulw_ok={:?} visible={vis} hwnd={:?}", ulw.is_ok(), hwnd);
            }
        }

        SelectObject(mem_dc, old);
        let _ = DeleteObject(dib);
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);

        // 分层窗口在 UpdateLayeredWindow 之后要再顶一次，否则在某些桌面组合状态
        // （远程/虚拟机断连后 DWM 进入 direct-flip）里画上去也不合成到屏幕。
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            size.cx,
            size.cy,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let _ = RedrawWindow(hwnd, None, None, RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(l: i32, t: i32, r: i32, b: i32) -> RECT {
        RECT { left: l, top: t, right: r, bottom: b }
    }

    #[test]
    fn caret_near_bottom_flips_above() {
        // 1080p：光标贴底 → 浮层翻到光标上方，且不越出工作区
        let (x, y) = place(Some((960, 1070)), 200, 56, work(0, 0, 1920, 1080), 1.0);
        assert!(y + 56 <= 1070, "应该翻到上方: y={y}");
        assert!(x >= 8);
    }

    #[test]
    fn clamps_at_right_edge() {
        let (x, _) = place(Some((1910, 500)), 200, 56, work(0, 0, 1920, 1080), 1.0);
        assert_eq!(x, 1920 - 200 - 8);
    }

    #[test]
    fn scaled_150pct_keeps_ratio() {
        // 150% 缩放：边距也按 1.5 倍算，200x56 的浮层同样夹在右边
        let (x, _) = place(Some((1900, 500)), 300, 84, work(0, 0, 1920, 1080), 1.5);
        assert_eq!(x, 1920 - 300 - 12);
    }

    #[test]
    fn tiny_screen_1280x720() {
        let (x, y) = place(Some((1200, 700)), 420, 74, work(0, 0, 1280, 720), 1.0);
        assert!(x + 420 <= 1280 - 8, "不能越过右边界: x={x}");
        assert!(y >= 4 && y + 74 <= 720 - 4, "必须完整留在工作区内: y={y}");
    }

    #[test]
    fn left_monitor_negative_origin() {
        // 左副屏：工作区 x 从 -1920 到 0
        let (x, _) = place(Some((-100, 500)), 200, 56, work(-1920, 0, 0, 1080), 1.0);
        assert!(x >= -1920 + 8);
        assert!(x + 200 <= 0 - 8);
    }

    #[test]
    fn no_anchor_centers_and_sits_above_dock() {
        let (x, y) = place(None, 300, 56, work(0, 0, 2560, 1440), 2.0);
        assert_eq!(x, (2560 - 300) / 2);
        assert_eq!(y, 1440 - 192 - 56);
    }
}
