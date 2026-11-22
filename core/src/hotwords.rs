//! 热词替换（DESIGN §4.3）：识别后把「常被听错的说法」换回正确写法。
//!
//! 规则文件 `hotwords.toml`：
//! ```toml
//! "艾瑞" = "Eryu"
//! "深度求索" = "DeepSeek"
//! "克劳德" = "Claude"
//! ```
//!
//! 三条约束（来自 DESIGN §4.3）：
//! 1. **最长匹配优先**：`"艾瑞笔记"` 比 `"艾瑞"` 长就先命中长的，避免子串误伤；
//! 2. **中文不切词**：中文没有空格，只能靠最长匹配，不做分词；
//! 3. **英文按词边界**：规则两端是 ASCII 字母/数字时，两侧不能紧贴字母/数字，
//!    否则 `"eryu"` 会把 `"eryuabc"` 里的子串也换掉。

use std::path::{Path, PathBuf};

/// 一条替换规则：错误写法 → 正确写法
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Default, Clone)]
pub struct Hotwords {
    /// 按 from 的字符数从长到短排序，扫描时先命中的就是最长匹配
    rules: Vec<Rule>,
    path: Option<PathBuf>,
    /// 文件本身有问题（格式错）时的说明：规则还在用，但要让用户看见
    error: Option<String>,
}

pub const FILE_NAME: &str = "hotwords.toml";

const TEMPLATE: &str = r#"# 热词表：左边是「常被听错的说法」，右边是「正确的写法」。
# 改完保存即生效，下一条录音就用新规则。
#
# "艾瑞" = "Eryu"
# "深度求索" = "DeepSeek"
"#;

/// 去除 TOML 字符串两侧的引号（逐行兜底解析用）
fn unquote(s: &str) -> String {
    let t = s.trim();
    let t = t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t);
    let t = t.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')).unwrap_or(t);
    t.trim().to_string()
}

impl Hotwords {
    pub fn empty() -> Self {
        Self::default()
    }

    /// 文件本身有格式问题时给界面看的说明
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// 从目录读 `hotwords.toml`；文件不存在就写一份带注释的空模板（方便用户编辑）。
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE_NAME);
        if !path.exists() {
            let _ = crate::fsutil::write_text_atomic(&path, TEMPLATE);
            return Self {
                rules: Vec::new(),
                path: Some(path),
                error: None,
            };
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        Self::from_toml_with_path(&text, Some(path))
    }

    pub fn from_toml(text: &str) -> Self {
        Self::from_toml_with_path(text, None)
    }

    fn from_toml_with_path(text: &str, path: Option<PathBuf>) -> Self {
        let mut rules = Vec::new();
        let mut error = None;
        match text.parse::<toml::Value>() {
            Ok(toml::Value::Table(table)) => {
                for (k, v) in table {
                    if let toml::Value::String(to) = v {
                        let from = k.trim().to_string();
                        let to = to.trim().to_string();
                        if from.is_empty() || to.is_empty() {
                            continue;
                        }
                        rules.push(Rule { from, to });
                    }
                }
            }
            Ok(_) | Err(_) => {
                // 整份文件解析不了（多一个引号、少一个等号之类）。以前这里直接把
                // 表清空 —— 用户看到「共 0 条」，热词全失效却不知道为什么；更糟的是
                // 再点一次「保存热词」会把文件重写成空模板，原来的规则就真没了。
                // 现在尽力逐行捞回 `"错" = "对"`，并把错误原因带出去给界面显示。
                let mut salvaged = 0usize;
                for line in text.lines() {
                    let t = line.split('#').next().unwrap_or("").trim();
                    if t.is_empty() {
                        continue;
                    }
                    let Some((k, v)) = t.split_once('=') else { continue };
                    let (k, v) = (k.trim(), v.trim());
                    // 引号只开不闭（最常见的手误）说明这一行本身就坏了，别硬猜
                    if k.starts_with('"') != k.ends_with('"')
                        || v.starts_with('"') != v.ends_with('"')
                    {
                        continue;
                    }
                    let key = unquote(k);
                    let val = unquote(v);
                    if key.is_empty() || val.is_empty() {
                        continue;
                    }
                    rules.push(Rule { from: key, to: val });
                    salvaged += 1;
                }
                // 只留能一眼看懂的：第几行 + 认得出几条（完整 toml 报错太长，界面放不下）
                let why = text
                    .parse::<toml::Value>()
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_default();
                let line = why
                    .split("line ")
                    .nth(1)
                    .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
                    .filter(|s| !s.is_empty());
                let where_ = match line {
                    Some(n) => format!("（第 {n} 行）"),
                    None => String::new(),
                };
                error = Some(format!(
                    "文件有格式错误{where_}，只载入 {salvaged} 条 · 点「保存热词」重写"
                ));
            }
        }
        rules.sort_by_key(|r| std::cmp::Reverse(r.from.chars().count()));
        // 同一 from 只留第一条
        rules.dedup_by(|a, b| a.from == b.from);
        Self { rules, path, error }
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// 把规则写回磁盘（0600）。写失败返回错误，UI 据此保留旧值。
    pub fn save(&self) -> Result<(), String> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| "这份热词表没有绑定文件路径".to_string())?;
        let mut out = String::from("# 热词表：左边是「常被听错的说法」，右边是「正确的写法」。\n");
        out.push_str("# 改完保存即生效，下一条录音就用新规则。\n\n");
        for r in &self.rules {
            out.push_str(&format!("{} = {}\n", toml_key(&r.from), toml_str(&r.to)));
        }
        crate::fsutil::write_text_atomic(&path, &out).map_err(|e| format!("写热词表失败：{e}"))?;
        Ok(())
    }

    /// 绑定落盘路径（UI 保存时用）
    pub fn with_path(mut self, path: PathBuf) -> Self {
        self.path = Some(path);
        self
    }

    /// 用给定规则替换（不落盘），供 UI 预览
    pub fn with_rules(rules: Vec<Rule>, path: Option<PathBuf>) -> Self {
        let mut rules: Vec<Rule> = rules
            .into_iter()
            .filter(|r| !r.from.trim().is_empty() && !r.to.trim().is_empty())
            .map(|r| Rule {
                from: r.from.trim().to_string(),
                to: r.to.trim().to_string(),
            })
            .collect();
        rules.sort_by_key(|r| std::cmp::Reverse(r.from.chars().count()));
        rules.dedup_by(|a, b| a.from == b.from);
        Self { rules, path, error: None }
    }

    /// 对识别结果做替换
    pub fn apply(&self, text: &str) -> String {
        if self.rules.is_empty() || text.is_empty() {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len() + 8);
        let mut i = 0usize;
        while i < chars.len() {
            let mut hit: Option<&Rule> = None;
            for r in &self.rules {
                let pat: Vec<char> = r.from.chars().collect();
                let n = pat.len();
                if n == 0 || i + n > chars.len() {
                    continue;
                }
                if chars[i..i + n] != pat[..] {
                    continue;
                }
                if needs_boundary(&pat) && !boundary_ok(&chars, i, n) {
                    continue;
                }
                hit = Some(r);
                break; // rules 已按长度降序，第一条命中即最长
            }
            match hit {
                Some(r) => {
                    out.push_str(&r.to);
                    i += r.from.chars().count();
                }
                None => {
                    out.push(chars[i]);
                    i += 1;
                }
            }
        }
        out
    }

    pub fn to_json(&self) -> String {
        let arr: Vec<serde_json::Value> = self
            .rules
            .iter()
            .map(|r| serde_json::json!({ "from": r.from, "to": r.to }))
            .collect();
        let mut v = serde_json::json!({ "rules": arr });
        if let Some(p) = &self.path {
            v["path"] = serde_json::Value::String(p.display().to_string());
        }
        if let Some(e) = &self.error {
            v["error"] = serde_json::Value::String(e.clone());
        }
        v.to_string()
    }

    /// 从 JSON（`{"rules":[{"from","to"}]}` 或裸数组）解析并落盘
    pub fn from_json(text: &str) -> Result<Self, String> {
        let v: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("热词表格式不对：{e}"))?;
        let arr = match v {
            serde_json::Value::Array(a) => a,
            serde_json::Value::Object(o) => o
                .get("rules")
                .and_then(|r| r.as_array())
                .cloned()
                .ok_or_else(|| "热词表缺少 rules 字段".to_string())?,
            _ => return Err("热词表应该是数组或带 rules 的对象".into()),
        };
        let mut rules = Vec::new();
        for item in arr {
            let from = item
                .get("from")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let to = item
                .get("to")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if from.trim().is_empty() || to.trim().is_empty() {
                continue;
            }
            rules.push(Rule { from, to });
        }
        Ok(Self::with_rules(rules, None))
    }
}

/// 规则两端是 ASCII 字母/数字时需要词边界
fn needs_boundary(pat: &[char]) -> bool {
    let first = pat.first().copied().unwrap_or(' ');
    let last = pat.last().copied().unwrap_or(' ');
    first.is_ascii_alphanumeric() || last.is_ascii_alphanumeric()
}

fn boundary_ok(chars: &[char], start: usize, len: usize) -> bool {
    let before = start.checked_sub(1).map(|i| chars[i]);
    let after = chars.get(start + len).copied();
    let ok_before = before.map_or(true, |c| !c.is_ascii_alphanumeric());
    let ok_after = after.map_or(true, |c| !c.is_ascii_alphanumeric());
    ok_before && ok_after
}

fn toml_key(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn toml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hw(pairs: &[(&str, &str)]) -> Hotwords {
        Hotwords::with_rules(
            pairs
                .iter()
                .map(|(a, b)| Rule {
                    from: a.to_string(),
                    to: b.to_string(),
                })
                .collect(),
            None,
        )
    }

    #[test]
    fn replaces_chinese_term() {
        let h = hw(&[("艾瑞", "Eryu")]);
        assert_eq!(h.apply("我觉得艾瑞挺好用"), "我觉得Eryu挺好用");
    }

    #[test]
    fn longest_match_wins() {
        let h = hw(&[("艾瑞", "Eryu"), ("艾瑞笔记", "EryuNotes")]);
        assert_eq!(h.apply("打开艾瑞笔记"), "打开EryuNotes");
        assert_eq!(h.apply("这是艾瑞"), "这是Eryu");
    }

    #[test]
    fn english_needs_word_boundary() {
        let h = hw(&[("eryu", "Eryu")]);
        assert_eq!(h.apply("use eryu now"), "use Eryu now");
        assert_eq!(h.apply("eryuabc"), "eryuabc", "英文子串不能被误伤");
    }

    #[test]
    fn ascii_mixed_rule_matches_inside_chinese() {
        let h = hw(&[("deepseek", "DeepSeek")]);
        assert_eq!(h.apply("我用deepseek写代码"), "我用DeepSeek写代码");
    }

    #[test]
    fn toml_roundtrip() {
        let t = "\"艾瑞\" = \"Eryu\"\n\"克劳德\" = \"Claude\"\n";
        let h = Hotwords::from_toml(t);
        assert_eq!(h.len(), 2);
        assert_eq!(h.apply("克劳德和艾瑞"), "Claude和Eryu");
    }

    #[test]
    fn json_roundtrip() {
        let j = r#"{"rules":[{"from":"艾瑞","to":"Eryu"}]}"#;
        let h = Hotwords::from_json(j).unwrap();
        assert_eq!(h.len(), 1);
        let back: serde_json::Value = serde_json::from_str(&h.to_json()).unwrap();
        assert_eq!(back["rules"][0]["to"], "Eryu");
    }

    #[test]
    fn ignores_empty_and_duplicate_rules() {
        let h = hw(&[("艾瑞", "Eryu"), ("艾瑞", "别的"), ("", "x")]);
        assert_eq!(h.len(), 1, "空 from 被丢弃，同一 from 只留第一条");
        assert_eq!(h.apply("艾瑞"), "Eryu", "先出现的那条生效");
    }

    #[test]
    fn salvages_lines_when_file_is_broken() {
        // 少一个引号：整份 TOML 解析失败，但两条能认出来的规则要继续生效
        let text = "\"艾瑞\" = \"Eryu\"\n\"深度求索\" = \"DeepSeek\"\n\"坏的 = \"值\"\n";
        let hw = Hotwords::from_toml(text);
        assert_eq!(hw.len(), 2, "两条好规则应该被捞回来");
        assert!(hw.error().is_some(), "要带出「格式有问题」的说明");
        assert_eq!(hw.apply("我用艾瑞和深度求索"), "我用Eryu和DeepSeek");
    }

    #[test]
    fn good_file_has_no_error() {
        let text = "\"艾瑞\" = \"Eryu\"\n";
        let hw = Hotwords::from_toml(text);
        assert_eq!(hw.len(), 1);
        assert!(hw.error().is_none());
    }
}
