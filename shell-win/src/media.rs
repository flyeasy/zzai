//! 录音时暂停系统播放（对齐 macOS 的 MediaPause）
//!
//! 用 Windows 的 GSMTC（GlobalSystemMediaTransportControlsSessionManager）：
//! 只有当前会话确实在播放时才暂停，而且只恢复「我们自己暂停的」那一次，
//! 避免把用户本来就没在放的音乐打开。

use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus;

use crate::report;

/// 是否真的在播放
pub fn is_playing() -> bool {
    match current_session() {
        Some(s) => match unsafe { s.GetPlaybackInfo() }.and_then(|i| unsafe { i.PlaybackStatus() }) {
            Ok(st) => st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing,
            Err(_) => false,
        },
        None => false,
    }
}

fn current_session() -> Option<windows::Media::Control::GlobalSystemMediaTransportControlsSession> {
    unsafe {
        let mgr = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .ok()?
            .get()
            .ok()?;
        mgr.GetCurrentSession().ok()
    }
}

/// 暂停（返回是否真的暂停了，便于只恢复我们暂停的）
pub fn pause() -> bool {
    if !is_playing() {
        return false;
    }
    if let Some(s) = current_session() {
        unsafe {
            match s.TryPauseAsync().and_then(|a| a.get()) {
                Ok(true) => {
                    report("MEDIA 已暂停播放");
                    return true;
                }
                Ok(false) => report("MEDIA 暂停请求被拒绝"),
                Err(e) => report(&format!("MEDIA 暂停失败：{e}")),
            }
        }
    }
    false
}

pub fn resume() {
    if let Some(s) = current_session() {
        unsafe {
            match s.TryPlayAsync().and_then(|a| a.get()) {
                Ok(true) => report("MEDIA 已恢复播放"),
                Ok(false) => report("MEDIA 恢复请求被拒绝"),
                Err(e) => report(&format!("MEDIA 恢复失败：{e}")),
            }
        }
    }
}
