//! 本地存储（DESIGN §9.5 / M1b 统计）：转写历史环形 200 条 + 使用统计。
//!
//! - 文件放 `<config_dir>/`：`history.json`、`stats.json`
//! - 权限 0600（隐私边界：历史文本/失败音频只给本用户读）
//! - 历史可关闭（`history.enabled=false` 时不再写，也不返回）
//! - 统计不含原文，只有计数与时长

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    /// RFC3339 本地时间
    pub at: String,
    pub text: String,
    /// inserted | pending | failed
    pub status: String,
    pub app: String,
    pub chars: usize,
    pub ms: u64,
    /// 音频还在暂存里、可重试（只有识别失败才 true）
    pub retryable: bool,
    /// 松手 → 出文本耗时（毫秒）；老历史没有这个字段
    #[serde(default)]
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DayStat {
    pub date: String,
    pub sessions: u64,
    pub chars: u64,
    pub speech_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Stats {
    pub sessions: u64,
    pub segments: u64,
    pub chars: u64,
    pub speech_ms: u64,
    pub decode_ms: u64,
    /// 最近的「松手→出字」样本（最多 200 条，用来算 p50/p95）
    #[serde(default)]
    pub latencies: Vec<u64>,
    pub since: String,
    pub days: BTreeMap<String, DayStat>,
}

pub struct Store {
    dir: PathBuf,
    pub enabled: bool,
    pub limit: usize,
    pub history: Vec<HistoryEntry>,
    pub stats: Stats,
    next_id: u64,
}

impl Store {
    pub fn load(dir: &Path, enabled: bool, limit: usize) -> Self {
        let _ = fs::create_dir_all(dir);
        let mut history: Vec<HistoryEntry> =
            read_json(&dir.join("history.json")).unwrap_or_default();
        let mut stats: Stats = read_json(&dir.join("stats.json")).unwrap_or_default();
        if stats.since.is_empty() {
            stats.since = today();
        }
        let next_id = history
            .iter()
            .filter_map(|h| h.id.trim_start_matches('h').parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        if history.len() > limit {
            history.truncate(limit);
        }
        Self {
            dir: dir.to_path_buf(),
            enabled,
            limit,
            history,
            stats,
            next_id,
        }
    }

    pub fn count(&self) -> usize {
        if self.enabled {
            self.history.len()
        } else {
            0
        }
    }

    /// 记一条成功结果（插入成功或待复制）
    pub fn record_success(
        &mut self,
        text: &str,
        status: &str,
        app: &str,
        speech_ms: u64,
        decode_ms: u64,
        segments: u64,
        latency_ms: Option<u64>,
    ) -> String {
        let id = self.new_id();
        let chars = text.chars().count() as u64;
        let day = today();
        let d = self.stats.days.entry(day.clone()).or_insert(DayStat {
            date: day.clone(),
            ..Default::default()
        });
        self.stats.sessions += 1;
        self.stats.segments += segments;
        self.stats.chars += chars;
        self.stats.speech_ms += speech_ms;
        self.stats.decode_ms += decode_ms;
        d.sessions += 1;
        d.chars += chars;
        d.speech_ms += speech_ms;
        if let Some(l) = latency_ms {
            self.stats.latencies.push(l);
            let over = self.stats.latencies.len().saturating_sub(200);
            if over > 0 {
                self.stats.latencies.drain(0..over);
            }
        }
        self.push(HistoryEntry {
            id: id.clone(),
            at: now_rfc3339(),
            text: text.to_string(),
            status: status.to_string(),
            app: app.to_string(),
            chars: chars as usize,
            ms: speech_ms,
            retryable: false,
            latency_ms,
        });
        let _ = self.save_stats();
        id
    }

    /// 记一条识别失败（音频进暂存 → retryable）
    pub fn record_failure(&mut self, app: &str, speech_ms: u64, retryable: bool) -> String {
        let id = self.new_id();
        self.push(HistoryEntry {
            id: id.clone(),
            at: now_rfc3339(),
            text: String::new(),
            status: "failed".to_string(),
            app: app.to_string(),
            chars: 0,
            ms: speech_ms,
            retryable,
            latency_ms: None,
        });
        id
    }

    fn push(&mut self, e: HistoryEntry) {
        if !self.enabled || self.limit == 0 {
            return;
        }
        self.history.insert(0, e);
        if self.history.len() > self.limit {
            self.history.truncate(self.limit);
        }
        let _ = self.save_history();
    }

    pub fn mark_status(&mut self, id: &str, status: &str) {
        if let Some(h) = self.history.iter_mut().find(|h| h.id == id) {
            h.status = status.to_string();
            let _ = self.save_history();
        }
    }

    pub fn delete(&mut self, id: &str) -> bool {
        let before = self.history.len();
        self.history.retain(|h| h.id != id);
        let changed = self.history.len() != before;
        if changed {
            let _ = self.save_history();
        }
        changed
    }

    pub fn clear(&mut self) {
        self.history.clear();
        let _ = self.save_history();
    }

    pub fn history_json(&self, limit: usize) -> String {
        if !self.enabled {
            return "[]".to_string();
        }
        let n = limit.clamp(0, self.limit) as usize;
        let items: Vec<&HistoryEntry> = self.history.iter().take(n).collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
    }

    pub fn stats_json(&self) -> String {
        let today = today();
        let today_stat = self.stats.days.get(&today).cloned().unwrap_or(DayStat {
            date: today.clone(),
            ..Default::default()
        });
        let days: Vec<&DayStat> = self.stats.days.values().rev().take(30).collect();
        serde_json::json!({
            "sessions": self.stats.sessions,
            "segments": self.stats.segments,
            "chars": self.stats.chars,
            "speech_ms": self.stats.speech_ms,
            "decode_ms": self.stats.decode_ms,
            "since": self.stats.since,
            "today": today_stat,
            "days": days,
            "history_count": self.count(),
            "history_enabled": self.enabled,
            "latency": latency_summary(&self.stats.latencies),
        })
        .to_string()
    }

    fn new_id(&mut self) -> String {
        let id = format!("h{}", self.next_id);
        self.next_id += 1;
        id
    }

    fn save_history(&self) -> std::io::Result<()> {
        write_private(
            &self.dir.join("history.json"),
            &serde_json::to_vec(&self.history)?,
        )
    }

    fn save_stats(&self) -> std::io::Result<()> {
        write_private(
            &self.dir.join("stats.json"),
            &serde_json::to_vec(&self.stats)?,
        )
    }
}

pub fn now_rfc3339() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// 延时样本的 p50/p95（验收口径：5s 短句松手→出文本 p95 ≤ 1500ms，DESIGN §5）
/// 给 `diagnose` 用的一行延迟摘要（空样本返回空串）
pub fn latency_line(samples: &[u64]) -> String {
    if samples.is_empty() {
        return String::new();
    }
    let v = latency_summary(samples);
    format!(
        "{} 次，p50 {}ms / p95 {}ms / max {}ms",
        v["count"], v["p50"], v["p95"], v["max"]
    )
}

pub fn latency_summary(samples: &[u64]) -> serde_json::Value {
    if samples.is_empty() {
        return serde_json::json!({ "count": 0 });
    }
    let mut v = samples.to_vec();
    v.sort_unstable();
    let pick = |p: f64| -> u64 {
        let idx = ((v.len() - 1) as f64 * p).round() as usize;
        v[idx.min(v.len() - 1)]
    };
    serde_json::json!({
        "count": v.len(),
        "min": v[0],
        "p50": pick(0.50),
        "p95": pick(0.95),
        "max": v[v.len() - 1],
    })
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    crate::fsutil::write_atomic(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("eryu-store-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn history_is_ring_limited_and_newest_first() {
        let d = tmpdir("ring");
        let mut s = Store::load(&d, true, 3);
        for i in 0..5 {
            s.record_success(
                &format!("第{i}条"),
                "inserted",
                "Xcode",
                1000,
                100,
                1,
                Some(400 + i as u64),
            );
        }
        assert_eq!(s.history.len(), 3);
        assert_eq!(s.history[0].text, "第4条");
        assert_eq!(s.history[2].text, "第2条");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn disabled_history_writes_nothing() {
        let d = tmpdir("off");
        let mut s = Store::load(&d, false, 200);
        s.record_success("不该出现", "inserted", "", 1000, 100, 1, None);
        assert!(s.history.is_empty());
        assert_eq!(s.history_json(10), "[]");
        // 统计仍然累计
        assert_eq!(s.stats.sessions, 1);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn stats_and_history_survive_reload() {
        let d = tmpdir("persist");
        {
            let mut s = Store::load(&d, true, 200);
            s.record_success("你好 Eryu", "inserted", "Terminal", 2500, 300, 2, None);
        }
        let s = Store::load(&d, true, 200);
        assert_eq!(s.stats.sessions, 1);
        assert_eq!(s.stats.chars, 7);
        assert_eq!(s.history.len(), 1);
        assert!(s.stats_json().contains("\"sessions\":1"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn delete_and_clear() {
        let d = tmpdir("del");
        let mut s = Store::load(&d, true, 200);
        let id = s.record_success("a", "inserted", "", 1, 1, 1, None);
        assert!(s.delete(&id));
        assert!(!s.delete(&id));
        s.record_success("b", "inserted", "", 1, 1, 1, None);
        s.clear();
        assert!(s.history.is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmpdir("perms");
        let mut s = Store::load(&d, true, 200);
        s.record_success("x", "inserted", "", 1, 1, 1, None);
        let mode = fs::metadata(d.join("history.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = fs::remove_dir_all(&d);
    }
}
