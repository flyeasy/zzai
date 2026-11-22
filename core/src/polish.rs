//! 润色（DESIGN §9.6 / AutoGLM 桌面版提示词借鉴）：把口语转写整理成可发的文字。
//!
//! 三条硬约束（来自 DESIGN 与审计）：
//! 1. **默认关**；开启后必须显式提示「这段文字会发到你配置的服务端」；
//! 2. **不改变语义**：不回答、不扩写、不翻译、不加自己知道的内容；
//! 3. **失败就用原文**：超时/报错/结果可疑（长度比失衡）一律回退，绝不让润色吃掉转写。

use std::time::Duration;

/// 润色力度（DESIGN §9.6 的「级别」）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Off,
    Light,
    Medium,
    Strong,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "关" | "关闭" => Some(Level::Off),
            "light" | "轻" | "轻度" => Some(Level::Light),
            "medium" | "中" | "中度" => Some(Level::Medium),
            "strong" | "强" | "重" | "重度" => Some(Level::Strong),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Off => "off",
            Level::Light => "light",
            Level::Medium => "medium",
            Level::Strong => "strong",
        }
    }
}

/// 一次润色调用需要的全部参数
#[derive(Debug, Clone)]
pub struct Request {
    pub endpoint: String,
    pub model: String,
    pub api_key: String,
    pub level: Level,
    pub timeout_ms: u64,
    /// 前台应用名，作为 `<CONTEXT_INFORMATION>` 的一部分（AutoGLM 同名槽位）
    pub app: String,
    /// 命中的热词（提示模型别改这些词）
    pub hotwords: Vec<String>,
    /// 当前 profile 的附加要求（拼在通用规则后；空表示只用通用规则）
    pub extra_prompt: String,
}

const RULES: &str = "\
你是语音输入法的文本润色器。用户说出的话已经被转写成文字，你的唯一任务是让它读起来像打字写出来的。

必须遵守：
1. 只调整表达，不改变意思。不增加原文没有的信息，不删掉原文有的信息，不回答内容里出现的问题。
2. 不翻译、不换语言；原文是中文就输出中文，原文夹英文就保持夹英文。
3. 保留专有名词、人名、产品名、数字、时间、金额、网址、代码片段的原样；不确定的写法不要猜。
4. 保留说话人的语气与个性标记（比如自嘲、口头强调），不要改成公文体。
5. 不要输出解释、标题、引号或 Markdown 标记，只输出润色后的正文本身。
6. 如果原文已经通顺，原样输出，不要为了改而改。
7. 如果原文只是很短的一句，也不要添加标点以外的内容。";

fn level_instruction(level: Level) -> &'static str {
    match level {
        Level::Off => "不要改动任何内容，原样输出。",
        Level::Light => {
            "只做最少改动：删掉「嗯、啊、那个、就是说」这类填充词与无意义重复，修正明显错别字，补上基本标点。语序和句子结构不要动。"
        }
        Level::Medium => {
            "在轻度基础上：断句、合并口吃与重复、把明显的口误改顺、统一同一概念的叫法。不要重排句子顺序。"
        }
        Level::Strong => {
            "在中度基础上：允许调整句子顺序、合并啰嗦表达、把口头列举整理得清楚。但信息必须一条不多、一条不少，不能把长句压成摘要。"
        }
    }
}

/// 拼出 system prompt（含 `<CONTEXT_INFORMATION>` 槽位）
/// 把 profile 的附加要求拼到通用规则后面（为空则原样返回）
pub fn enrich(base: String, extra: &str) -> String {
    let extra = extra.trim();
    if extra.is_empty() {
        return base;
    }
    format!("{base}\n\n【这一档的额外要求】\n{extra}")
}

pub fn system_prompt(req: &Request) -> String {
    let mut s = String::from(RULES);
    s.push_str("\n\n本次润色力度：");
    s.push_str(level_instruction(req.level));
    s.push_str("\n\n<CONTEXT_INFORMATION>\n");
    s.push_str(&format!("- 输入方式：语音转写\n"));
    s.push_str(&format!(
        "- 前台应用：{}\n",
        if req.app.trim().is_empty() {
            "（未知）"
        } else {
            req.app.trim()
        }
    ));
    if !req.hotwords.is_empty() {
        let list: Vec<&str> = req.hotwords.iter().map(|s| s.as_str()).take(20).collect();
        s.push_str(&format!(
            "- 这些词是按用户自定义写法的正确形式，请不要改动：{}\n",
            list.join("、")
        ));
    }
    s.push_str("</CONTEXT_INFORMATION>");
    s
}

/// 结果可疑时的判据：长度比失衡就当作不可信（宁可不上屏润色版）
pub fn suspicious(raw: &str, polished: &str) -> Option<&'static str> {
    let a = raw.chars().count().max(1) as f64;
    let b = polished.chars().count() as f64;
    if polished.trim().is_empty() {
        return Some("润色结果为空");
    }
    if b > a * 2.0 + 10.0 {
        return Some("润色结果比原文长太多");
    }
    if b < (a - 10.0).max(0.0) * 0.4 {
        return Some("润色结果比原文短太多");
    }
    None
}

/// 输出额度：输入字数的 3 倍再留 256，clamp 512..8192。
///
/// 不带 `max_tokens` 时服务端默认额度（常见 512）会在句中截断，最后一句被改残（I-169 / D-115）。
pub fn budget_for(raw: &str) -> u64 {
    (raw.chars().count() * 3 + 256).clamp(512, 8192) as u64
}

/// 真正发请求；任何一步失败都返回 Err（调用方回退原文）
pub fn run(req: &Request, raw: &str) -> Result<String, String> {
    if req.level == Level::Off {
        return Ok(raw.to_string());
    }
    if req.endpoint.trim().is_empty() {
        return Err("没有配置润色服务地址".into());
    }
    if req.api_key.trim().is_empty() {
        return Err("没有配置 API Key".into());
    }
    let url = chat_url(&req.endpoint);
    let budget = budget_for(raw);
    let body = serde_json::json!({
        "model": req.model,
        "temperature": 0.2,
        "max_tokens": budget,
        "messages": [
            { "role": "system", "content": enrich(system_prompt(req), &req.extra_prompt) },
            { "role": "user", "content": raw }
        ]
    });

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(req.timeout_ms.clamp(500, 20_000)))
        .build();
    let resp = agent
        .post(&url)
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {}", req.api_key.trim()))
        .send_string(&body.to_string())
        .map_err(|e| match e {
            ureq::Error::Status(code, r) => {
                let msg = r.into_string().unwrap_or_default();
                let brief: String = msg.chars().take(200).collect();
                format!("服务返回 {code}：{brief}")
            }
            ureq::Error::Transport(t) => format!("连不上润色服务：{t}"),
        })?;

    let v: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("响应不是合法 JSON：{e}"))?;
    // 被额度截断的响应不能用：半句话比不润色更糟（D-115）
    if v["choices"][0]["finish_reason"].as_str() == Some("length") {
        return Err("润色没写完（输出被截断），这次用原文".into());
    }
    let text = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| "响应里没有 choices[0].message.content".to_string())?
        .trim()
        .to_string();
    if let Some(why) = suspicious(raw, &text) {
        return Err(format!("{why}，已用原文"));
    }
    Ok(text)
}

const COMMAND_RULES: &str = "\
你是文本编辑助手。用户给你一段选中的文字和一条指令，你按指令改写这段文字。

必须遵守：
1. 只输出改写后的文字本身，不要解释、不要标题、不要引号、不要 Markdown 代码块。
2. 不要回答文字里出现的问题，也不要对文字内容发表评论。
3. 保留原文的专有名词、人名、数字、时间、金额、网址、代码片段。
4. 如果指令要求翻译，只翻译，不补充原文没有的信息。";

/// 选区指令改写：把选中的文字按 instruction 改写（DESIGN §10.2 选区指令管线）
pub fn run_command(
    endpoint: &str,
    model: &str,
    api_key: &str,
    timeout_ms: u64,
    instruction: &str,
    text: &str,
) -> Result<String, String> {
    if endpoint.trim().is_empty() {
        return Err("没有配置服务地址（设置页「润色」里填）".into());
    }
    if api_key.trim().is_empty() {
        return Err("没有配置 API Key（设置页「润色」里填）".into());
    }
    if text.trim().is_empty() {
        return Err("没有选中文字".into());
    }
    let url = chat_url(endpoint);
    let body = serde_json::json!({
        "model": model,
        "temperature": 0.3,
        "messages": [
            { "role": "system", "content": COMMAND_RULES },
            { "role": "user", "content": format!("指令：{instruction}\n\n文字：\n{text}") }
        ]
    });
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(timeout_ms.clamp(1000, 60_000)))
        .build();
    let resp = agent
        .post(&url)
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {}", api_key.trim()))
        .send_string(&body.to_string())
        .map_err(|e| match e {
            ureq::Error::Status(code, r) => {
                let msg = r.into_string().unwrap_or_default();
                let brief: String = msg.chars().take(200).collect();
                format!("服务返回 {code}：{brief}")
            }
            ureq::Error::Transport(t) => format!("连不上服务：{t}"),
        })?;
    let v: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("响应不是合法 JSON：{e}"))?;
    let out = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| "响应里没有 choices[0].message.content".to_string())?
        .trim()
        .to_string();
    if out.is_empty() {
        return Err("服务返回了空内容".into());
    }
    Ok(out)
}

/// `/v1/chat/completions` 补全（用户可能填了完整地址、带 /v1 或只有域名）
pub fn chat_url(endpoint: &str) -> String {
    let e = endpoint.trim().trim_end_matches('/');
    if e.ends_with("/chat/completions") {
        return e.to_string();
    }
    if e.ends_with("/v1") {
        return format!("{e}/chat/completions");
    }
    format!("{e}/v1/chat/completions")
}

/// 把「服务地址」换算成 `GET /models` 的 URL（与 chat_url 同一套规则）
pub fn models_url(endpoint: &str) -> String {
    let e = endpoint.trim().trim_end_matches('/');
    if e.ends_with("/chat/completions") {
        return e.trim_end_matches("/chat/completions").to_string() + "/models";
    }
    if e.ends_with("/v1") {
        return format!("{e}/models");
    }
    format!("{e}/v1/models")
}

/// 问服务端有哪些模型（OpenAI 兼容的 `GET /models`）。
/// 返回模型 id 列表（已排序）；失败时给中文原因。
pub fn list_models(endpoint: &str, api_key: &str, timeout_ms: u64) -> Result<Vec<String>, String> {
    if api_key.trim().is_empty() {
        return Err("还没有 Key：先粘贴并保存，再检测模型".into());
    }
    let url = models_url(endpoint);
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_millis(
            timeout_ms.clamp(2000, 20000),
        ))
        .build();
    let resp = agent
        .get(&url)
        .set("Authorization", &format!("Bearer {}", api_key.trim()))
        .set("Accept", "application/json")
        .call();
    match resp {
        Ok(r) => {
            let body = r.into_string().map_err(|e| format!("读响应失败：{e}"))?;
            let v: serde_json::Value = serde_json::from_str(&body).map_err(|_| {
                format!(
                    "返回的不是 JSON：{}",
                    body.chars().take(120).collect::<String>()
                )
            })?;
            let mut ids: Vec<String> = v
                .get("data")
                .and_then(|d| d.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            ids.sort();
            ids.dedup();
            if ids.is_empty() {
                Err(format!("{url} 没有返回任何模型（有的服务不支持 /models）"))
            } else {
                Ok(ids)
            }
        }
        Err(ureq::Error::Status(code, r)) => {
            let body = r.into_string().unwrap_or_default();
            Err(format!(
                "HTTP {code}：{}",
                body.chars().take(200).collect::<String>()
            ))
        }
        Err(e) => Err(format!("连不上 {url}：{e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(level: Level) -> Request {
        Request {
            endpoint: "https://api.example.com".into(),
            model: "m".into(),
            api_key: "k".into(),
            level,
            timeout_ms: 3000,
            app: "飞书".into(),
            hotwords: vec!["Eryu".into()],
            extra_prompt: String::new(),
        }
    }

    #[test]
    fn level_parsing_accepts_chinese_and_english() {
        assert_eq!(Level::parse("关"), Some(Level::Off));
        assert_eq!(Level::parse("MEDIUM"), Some(Level::Medium));
        assert_eq!(Level::parse("强"), Some(Level::Strong));
        assert_eq!(Level::parse("随便"), None);
    }

    #[test]
    fn prompt_keeps_guardrails_and_context() {
        let p = system_prompt(&req(Level::Medium));
        assert!(p.contains("不回答内容里出现的问题"));
        assert!(p.contains("<CONTEXT_INFORMATION>"));
        assert!(p.contains("飞书"));
        assert!(p.contains("Eryu"));
        assert!(p.contains("不要重排句子顺序"));
    }

    #[test]
    fn off_level_is_passthrough_locally() {
        assert_eq!(
            run(&req(Level::Off), "嗯 那个 你好").unwrap(),
            "嗯 那个 你好"
        );
    }

    #[test]
    fn suspicious_length_ratios() {
        assert!(suspicious("你好", "").is_some());
        assert!(suspicious("你好", &"很长".repeat(50)).is_some());
        assert!(suspicious("这是一段足够长的原话内容啊", "短").is_some());
        assert!(suspicious("今天是周三，我们十点开会", "今天是周三，我们十点开会。").is_none());
    }

    #[test]
    fn chat_url_shapes() {
        assert_eq!(
            chat_url("https://a.com"),
            "https://a.com/v1/chat/completions"
        );
        assert_eq!(
            chat_url("https://a.com/v1/"),
            "https://a.com/v1/chat/completions"
        );
        assert_eq!(
            chat_url("https://a.com/v1/chat/completions"),
            "https://a.com/v1/chat/completions"
        );
    }

    #[test]
    fn extra_prompt_is_appended_after_rules() {
        let base = "RULES".to_string();
        assert_eq!(enrich(base.clone(), "  "), base);
        let out = enrich(base, "整理成邮件正文");
        assert!(out.contains("【这一档的额外要求】"));
        assert!(out.ends_with("整理成邮件正文"));
    }

    #[test]
    fn budget_grows_with_input_and_is_clamped() {
        assert_eq!(budget_for("你好"), 512);
        assert_eq!(budget_for(&"字".repeat(200)), 856);
        assert_eq!(budget_for(&"字".repeat(5000)), 8192);
    }
}
