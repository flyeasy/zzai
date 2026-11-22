//! 注入后编辑追踪（DESIGN §10.2「热词自动学习」的 Windows 侧）。
//!
//! 上屏成功后盯住焦点输入框十几秒：用户如果把某个词改掉了，就把
//! 「原词 → 新词」当成一条待采纳的热词候选（存 `suggestions.json`，不落原文）。
//! 只在隐私模式关闭时工作；读不到文本（自绘控件/终端）就安静放弃。

use std::sync::atomic::{AtomicU64, Ordering};

/// 盯多久（毫秒）
const WINDOW_MS: u64 = 15_000;
/// 轮询间隔（毫秒）
const INTERVAL_MS: u64 = 1_000;

/// 每次 watch 递增；旧线程发现代数变了就退出（避免多次上屏互相打架）
static GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub from: String,
    pub to: String,
}

fn suggestions_path() -> std::path::PathBuf {
    std::path::Path::new(&crate::config_dir()).join("suggestions.json")
}

pub fn load() -> Vec<Suggestion> {
    let Ok(s) = std::fs::read_to_string(suggestions_path()) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
        return Vec::new();
    };
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|x| {
                    Some(Suggestion {
                        from: x.get("from")?.as_str()?.to_string(),
                        to: x.get("to")?.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn save(list: &[Suggestion]) {
    let arr: Vec<serde_json::Value> = list
        .iter()
        .take(50)
        .map(|s| serde_json::json!({ "from": s.from, "to": s.to }))
        .collect();
    let _ = std::fs::write(
        suggestions_path(),
        serde_json::to_string_pretty(&serde_json::Value::Array(arr)).unwrap_or_default(),
    );
}

pub fn push(s: Suggestion) {
    let mut list = load();
    if list.iter().any(|x| *x == s) {
        return;
    }
    list.push(s);
    if list.len() > 50 {
        let keep = list.len() - 50;
        list.drain(0..keep);
    }
    save(&list);
}

pub fn remove(s: &Suggestion) {
    let mut list = load();
    list.retain(|x| x != s);
    save(&list);
}

/// 开始观察：`expected` 是刚上屏的文本
pub fn watch(expected: String) {
    if expected.trim().is_empty() {
        return;
    }
    if crate::privacy_mode() {
        return;
    }
    let Some(hwnd) = crate::inject::focused_control_hwnd() else {
        return;
    };
    // 读一次确认这个控件真的能读（自绘/终端就读不到，别白盯 15 秒）
    if crate::inject::read_text(hwnd).is_none() {
        return;
    }
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        let mut waited = 0u64;
        while waited < WINDOW_MS {
            std::thread::sleep(std::time::Duration::from_millis(INTERVAL_MS));
            waited += INTERVAL_MS;
            if GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            let Some(actual) = crate::inject::read_text(hwnd) else {
                return;
            };
            // 还没改过：值里仍然包含上屏文本（用户可能只是点了别处）
            if actual.contains(&expected) {
                continue;
            }
            if let Some(s) = diff(&expected, &actual) {
                push(s.clone());
                crate::tray::set_tip(&format!("字在 学会了：{} → {}", s.from, s.to));
                return;
            }
            return;
        }
    });
}

/// 找出「原词 → 新词」：取公共前后缀，中间那段就是改动（与 mac `EditObserver.diff` 同规则）
pub fn diff(expected: &str, actual: &str) -> Option<Suggestion> {
    let a: Vec<char> = expected.chars().collect();
    let b: Vec<char> = actual.chars().collect();
    if b.is_empty() {
        return None;
    }
    let ratio = b.len() as f64 / a.len().max(1) as f64;
    if !(0.6..1.6).contains(&ratio) {
        return None;
    }
    let mut prefix = 0;
    while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < a.len() - prefix
        && suffix < b.len() - prefix
        && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let from: String = a[prefix..a.len() - suffix].iter().collect();
    let to: String = b[prefix..b.len() - suffix].iter().collect();
    if from.is_empty() || to.is_empty() || from == to {
        return None;
    }
    // 太小/太大的改动都不像「一个词」
    let fl = from.chars().count();
    let tl = to.chars().count();
    if fl < 2 || fl > 24 || tl > 24 {
        return None;
    }
    // 纯标点/空白不学
    let meaningful = from
        .chars()
        .any(|c| c.is_alphanumeric() || (c as u32) > 0x2E80);
    if !meaningful {
        return None;
    }
    // 已包含关系（"深度求索" → "深度求索AI"）学习价值低
    if to.contains(&from) {
        return None;
    }
    Some(Suggestion { from, to })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_change_no_suggestion() {
        assert!(diff("今天天气不错我们开始吧", "今天天气不错我们开始吧").is_none());
    }

    #[test]
    fn learns_word_correction() {
        let s = diff("我们用 qwen 转写", "我们用 通义 转写").unwrap();
        assert_eq!(s.from, "qwen");
        assert_eq!(s.to, "通义");
    }

    #[test]
    fn rejects_whole_rewrite() {
        assert!(diff("今天天气不错", "完全不同的另一句话在这里").is_none());
    }
}
