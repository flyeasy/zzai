//! 应用图标：把打包进来的 .ico 落到配置目录，再 LoadImage 成 HICON。
//!
//! 为什么不把图标编进 exe 资源：交叉编译时没有 windres/rc.exe 这条链路，
//! 用一个普通文件反而更稳；文件本身就放在 exe 同目录/资源里由 `include_bytes!` 带进来。

use std::sync::OnceLock;
use windows::core::PCWSTR;
use windows::Win32::UI::WindowsAndMessaging::{LoadImageW, HICON, IMAGE_ICON, LR_DEFAULTSIZE, LR_LOADFROMFILE};

static BYTES: &[u8] = include_bytes!("../assets/eryu.ico");
static CACHED: OnceLock<isize> = OnceLock::new();

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn icon_path() -> Option<String> {
    let dir = std::env::var("APPDATA").ok()?;
    let dir = format!("{dir}\\Eryu");
    std::fs::create_dir_all(&dir).ok()?;
    let p = format!("{dir}\\eryu.ico");
    // 每次启动写一遍（60KB，代价可忽略），这样换图标不用清缓存
    if std::fs::read(&p).map(|b| b != BYTES).unwrap_or(true) {
        std::fs::write(&p, BYTES).ok()?;
    }
    Some(p)
}

/// 应用图标；拿不到就返回空 HICON（调用方自己退回系统默认）
pub fn app_icon() -> HICON {
    let v = *CACHED.get_or_init(|| unsafe {
        let Some(p) = icon_path() else { return 0 };
        let w = wide(&p);
        let h = LoadImageW(None, PCWSTR(w.as_ptr()), IMAGE_ICON, 0, 0, LR_LOADFROMFILE | LR_DEFAULTSIZE)
            .unwrap_or_default();
        h.0 as isize
    });
    HICON(v as *mut _)
}
