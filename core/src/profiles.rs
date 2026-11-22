//! 润色 profile（DESIGN §9.6 / R-014）
//!
//! 一个 profile = 名称 + 图标 + 润色力度 + 附加提示词。切换即时生效（下一次润色起）。
//! 与 AutoGLM personas v3 的差异：我们先不做「内置只读 + fork」，直接可编辑（个人自用工具，
//! 少一层心智负担），但把内置三条写进首次生成的 `profiles.toml`，用户改坏了删掉即可恢复。

use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "profiles.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    /// SF Symbol 名，UI 用；空则用默认图标
    pub icon: String,
    /// off / light / medium / strong
    pub level: String,
    /// 附加提示词：拼在通用规则后面（为空表示只用通用规则）
    pub prompt: String,
    /// 风格快捷键槽（≤3 档可用）：带修饰键的组合，如 CTRL+ALT+1；空表示不绑
    pub slot: String,
}

#[derive(Debug, Clone)]
pub struct Profiles {
    list: Vec<Profile>,
    active: String,
    path: Option<PathBuf>,
}

fn builtin() -> Vec<Profile> {
    vec![
        Profile {
            name: "日常".into(),
            icon: "message".into(),
            level: "light".into(),
            prompt: String::new(),
            slot: "CTRL+ALT+1".into(),
        },
        Profile {
            name: "邮件".into(),
            icon: "envelope".into(),
            level: "strong".into(),
            prompt: "整理成邮件正文：分点清晰，语气礼貌但不套话，不编造称呼、称呼用「你」".into(),
            slot: "CTRL+ALT+2".into(),
        },
        Profile {
            name: "笔记".into(),
            icon: "note.text".into(),
            level: "medium".into(),
            prompt: "保留口语里的顺序和要点，可以用短句和分号，不要总结成结论".into(),
            slot: "CTRL+ALT+3".into(),
        },
    ]
}

impl Default for Profiles {
    fn default() -> Self {
        let list = builtin();
        let active = list[0].name.clone();
        Self {
            list,
            active,
            path: None,
        }
    }
}

impl Profiles {
    /// 读 `dir/profiles.toml`；不存在就写一份内置模板（0600）
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(text) => match Self::from_toml(&text) {
                Ok(mut p) => {
                    p.path = Some(path);
                    // 老文件（v1.5 之前没有 slot）迁移一次：全空就按顺序补 ⌃⌥1..3
                    if p.list.iter().all(|x| x.slot.is_empty()) {
                        p.fill_default_slots();
                        let _ = p.save();
                    }
                    p
                }
                Err(e) => {
                    eprintln!("[eryu] profiles.toml 解析失败，用内置：{e}");
                    let mut p = Self::default();
                    p.path = Some(path);
                    p
                }
            },
            Err(_) => {
                let mut p = Self::default();
                p.path = Some(path);
                let _ = p.save();
                p
            }
        }
    }

    pub fn from_toml(text: &str) -> Result<Self, String> {
        #[derive(serde::Deserialize)]
        struct File {
            #[serde(default)]
            active: Option<String>,
            #[serde(default, rename = "profile")]
            profiles: Vec<Row>,
        }
        #[derive(serde::Deserialize)]
        struct Row {
            name: String,
            #[serde(default)]
            icon: String,
            #[serde(default)]
            level: String,
            #[serde(default)]
            prompt: String,
            #[serde(default)]
            slot: String,
        }
        let f: File = toml::from_str(text).map_err(|e| e.to_string())?;
        let list: Vec<Profile> = f
            .profiles
            .into_iter()
            .filter(|r| !r.name.trim().is_empty())
            .map(|r| Profile {
                name: r.name.trim().to_string(),
                icon: r.icon,
                level: normalize_level(&r.level),
                prompt: r.prompt,
                slot: r.slot.trim().to_string(),
            })
            .collect();
        if list.is_empty() {
            return Err("profiles.toml 里一条 profile 都没有".into());
        }
        let active = f
            .active
            .filter(|a| list.iter().any(|p| &p.name == a))
            .unwrap_or_else(|| list[0].name.clone());
        Ok(Self {
            list,
            active,
            path: None,
        })
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let arr = v
            .get("profiles")
            .and_then(|x| x.as_array())
            .or_else(|| v.as_array())
            .ok_or_else(|| "缺少 profiles 数组".to_string())?;
        let mut list = Vec::new();
        for item in arr {
            let name = item
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim();
            if name.is_empty() {
                continue;
            }
            list.push(Profile {
                name: name.to_string(),
                icon: item
                    .get("icon")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                level: normalize_level(
                    item.get("level")
                        .and_then(|x| x.as_str())
                        .unwrap_or("light"),
                ),
                prompt: item
                    .get("prompt")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                slot: item
                    .get("slot")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }
        if list.is_empty() {
            return Err("至少要有一个 profile".into());
        }
        let active = v
            .get("active")
            .and_then(|x| x.as_str())
            .filter(|a| list.iter().any(|p| p.name == *a))
            .map(|s| s.to_string())
            .unwrap_or_else(|| list[0].name.clone());
        Ok(Self {
            list,
            active,
            path: None,
        })
    }

    pub fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let text = self.to_toml();
        crate::fsutil::write_text_atomic(path, &text)
            .map_err(|e| format!("写 profiles.toml 失败：{e}"))
    }

    /// 生成 TOML 文本（`save` 与单测共用；prompt 里的换行/制表符必须转义）
    pub fn to_toml(&self) -> String {
        let mut text = String::new();
        text.push_str("# Eryu 润色 profile：一条 [[profile]] 就是一档风格\n");
        text.push_str("# level = off / light / medium / strong；prompt 会拼在通用润色规则后面\n\n");
        text.push_str(&format!("active = {}\n\n", toml_str(&self.active)));
        for p in &self.list {
            text.push_str("[[profile]]\n");
            text.push_str(&format!("name = {}\n", toml_str(&p.name)));
            text.push_str(&format!("icon = {}\n", toml_str(&p.icon)));
            text.push_str(&format!("level = {}\n", toml_str(&p.level)));
            text.push_str(&format!("prompt = {}\n", toml_str(&p.prompt)));
            text.push_str(&format!("slot = {}\n\n", toml_str(&p.slot)));
        }
        text
    }

    /// 前 3 档补默认槽位 ⌃⌥1..3（只补空的，不覆盖已配的）
    fn fill_default_slots(&mut self) {
        let defaults = ["CTRL+ALT+1", "CTRL+ALT+2", "CTRL+ALT+3"];
        for (i, p) in self.list.iter_mut().enumerate() {
            if p.slot.is_empty() && i < defaults.len() {
                p.slot = defaults[i].to_string();
            }
        }
    }

    pub fn list(&self) -> &[Profile] {
        &self.list
    }
    pub fn active(&self) -> &str {
        &self.active
    }
    /// 当前生效的 profile（active 找不到就退回第一条）
    pub fn current(&self) -> Profile {
        self.list
            .iter()
            .find(|p| p.name == self.active)
            .cloned()
            .unwrap_or_else(|| self.list[0].clone())
    }
    /// 切换 active；名字不存在返回错误（UI 就别让它切到空气里）
    pub fn set_active(&mut self, name: &str) -> Result<(), String> {
        if !self.list.iter().any(|p| p.name == name) {
            return Err(format!("没有叫「{name}」的 profile"));
        }
        self.active = name.to_string();
        self.save()
    }
    pub fn replace(&mut self, list: Vec<Profile>, active: Option<String>) -> Result<(), String> {
        if list.is_empty() {
            return Err("至少要有一个 profile".into());
        }
        self.list = list;
        if let Some(a) = active {
            if self.list.iter().any(|p| p.name == a) {
                self.active = a;
            }
        }
        if !self.list.iter().any(|p| p.name == self.active) {
            self.active = self.list[0].name.clone();
        }
        self.save()
    }
    pub fn to_json(&self) -> String {
        let list: Vec<serde_json::Value> = self
            .list
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name, "icon": p.icon, "level": p.level,
                    "prompt": p.prompt, "slot": p.slot, "active": p.name == self.active
                })
            })
            .collect();
        serde_json::json!({ "active": self.active, "profiles": list, "path": self.path.as_ref().map(|p| p.display().to_string()) }).to_string()
    }
}

fn normalize_level(raw: &str) -> String {
    crate::polish::Level::parse(raw)
        .unwrap_or(crate::polish::Level::Light)
        .as_str()
        .to_string()
}

fn toml_str(s: &str) -> String {
    // 换行必须转义：prompt 是多行中文时，裸换行会让 TOML 报
    // `invalid basic string`（实测 I-078），整份 profiles.toml 都会解析失败。
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_with_newlines_roundtrips() {
        let mut p = Profiles::default();
        p.list[0].prompt = "第一行\n第二行\t带制表符\n\"引号\"".to_string();
        let text = p.to_toml();
        let back = Profiles::from_toml(&text).expect("应该能解析回来");
        assert_eq!(back.list[0].prompt, p.list[0].prompt);
    }

    #[test]
    fn builtin_profile_roundtrip_toml() {
        let p = Profiles::default();
        let dir = std::env::temp_dir().join(format!("eryu-profiles-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut saved = p.clone();
        saved.path = Some(dir.join(FILE_NAME));
        saved.save().unwrap();
        let text = std::fs::read_to_string(dir.join(FILE_NAME)).unwrap();
        let back = Profiles::from_toml(&text).unwrap();
        assert_eq!(back.list().len(), 3);
        assert_eq!(back.active(), "日常");
        assert_eq!(back.current().level, "light");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_roundtrip_and_active_fallback() {
        let p = Profiles::from_json(
            r#"{"active":"不存在","profiles":[{"name":"甲","level":"strong","prompt":"x"},{"name":"乙","level":"off"}]}"#,
        )
        .unwrap();
        assert_eq!(p.active(), "甲");
        assert_eq!(p.current().prompt, "x");
        assert_eq!(p.list()[1].level, "off");
    }

    #[test]
    fn set_active_rejects_unknown() {
        let mut p = Profiles::default();
        assert!(p.set_active("不存在").is_err());
        assert!(p.set_active("邮件").is_ok());
        assert_eq!(p.current().level, "strong");
    }
}
