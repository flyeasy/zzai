//! 开机自启（D-108 的 Windows 侧）：写 HKCU 的 Run 键。
//!
//! 为什么不用计划任务/服务：个人工具，登录即启动即可；Run 键不需要管理员，
//! 卸载时删这个值就干净了。macOS 侧对应的是 SMAppService。

use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE: &str = "Eryu";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    let mut h = HKEY::default();
    let path = wide(RUN_KEY);
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            0,
            access,
            &mut h,
        )
    };
    if rc == ERROR_SUCCESS {
        Some(h)
    } else {
        None
    }
}

/// 当前 Run 键里登记的命令（没有就是 None）
pub fn current() -> Option<String> {
    let h = open(KEY_QUERY_VALUE)?;
    let name = wide(VALUE);
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegQueryValueExW(
            h,
            PCWSTR(name.as_ptr()),
            None,
            None,
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut len),
        )
    };
    unsafe {
        let _ = RegCloseKey(h);
    }
    if rc != ERROR_SUCCESS || len < 2 {
        return None;
    }
    let n = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..n.min(buf.len())]))
}

/// 打开/关闭开机自启；exe 路径取自当前进程
pub fn sync(enabled: bool) -> Result<(), String> {
    let h = open(KEY_SET_VALUE).ok_or_else(|| "打不开 Run 注册表键".to_string())?;
    let name = wide(VALUE);
    let rc = if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("取不到 exe 路径：{e}"))?;
        let cmd = format!("\"{}\"", exe.display());
        let data = wide(&cmd);
        unsafe {
            RegSetValueExW(
                h,
                PCWSTR(name.as_ptr()),
                0,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    data.as_ptr() as *const u8,
                    data.len() * 2,
                )),
            )
        }
    } else {
        unsafe { RegDeleteValueW(h, PCWSTR(name.as_ptr())) }
    };
    unsafe {
        let _ = RegCloseKey(h);
    }
    if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!("注册表写入失败：{rc:?}"))
    }
}

/// 配置里的开关与注册表实际状态对齐（启动时调用；容错，不让它阻塞启动）
pub fn reconcile(enabled: bool) {
    let want = enabled;
    let have = current();
    if std::env::var("ERYU_RAW_DEBUG").is_ok() {
        eprintln!("[eryu] autostart reconcile want={want} have={have:?}");
    }
    // 已存在时还要核对路径：exe 换过位置（便携版挪目录、重装到别的盘）就重写，
    // 否则 Run 键还指着旧路径，开机会静默起不来
    let stale = match (&have, want) {
        (Some(v), true) => {
            let me = std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            !v.trim_matches('"').eq_ignore_ascii_case(&me)
        }
        _ => false,
    };
    let ok = match (&have, want) {
        (None, false) => true,
        (Some(_), true) => !stale || sync(true).is_ok(),
        _ => sync(want).is_ok(),
    };
    if stale {
        crate::report("AUTOSTART 路径变了，已重写 Run 键");
    }
    if !ok {
        crate::report(&format!(
            "AUTOSTART 对齐失败 want={want} have={have:?}"
        ));
    }
}
