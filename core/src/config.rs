//! 配置（DESIGN §9.2 / §4.5）：`<config_dir>/config.toml`，失败保留旧配置。
//!
//! 优先级：命令行/env 覆盖 > config.toml > 内置默认。
//! 保存总是重写整份文件（带注释的模板），避免出现"半新半旧"的字段。

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// 默认模型下载镜像（自建，避免 GitHub 受限网络）
/// 诊断包默认上传地址（DESIGN §12.8）：和模型镜像同一台服务器
pub const DEFAULT_DIAG_UPLOAD: &str = "https://officeace.harmonyos.fun/eryu-diag/upload";

pub const DEFAULT_MODEL_MIRROR: &str = "https://officeace.harmonyos.fun/eryu-models";

pub const DEFAULT_ENGINE: &str = "sense";
pub const MIN_RECORD_MS: u32 = 400;
pub const MAX_RECORD_SECS: u32 = 120;
pub const HISTORY_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct Config {
    pub dir: PathBuf,
    // [engine]
    pub engine: String,
    pub num_threads: i32,
    // [models]
    pub model_dir: PathBuf,
    pub vad_model: PathBuf,
    pub qwen3_dir: PathBuf,
    /// 模型下载镜像（DESIGN §9.2）：留空走官方 GitHub；填了就用「镜像 + 文件名」
    pub model_mirror: String,
    /// 模型下载代理；空 = 自动（环境变量 / Windows Internet Settings）
    pub model_proxy: String,
    /// 新用户引导是否已完成（DESIGN §10.1 onboarding）
    pub onboarding_done: bool,
    /// 引导第 3 步「试说一句」是否成功过一次（R-101；false 时下次启动回到这一步）
    pub onboarding_probe_done: bool,
    // [audio]
    pub min_record_ms: u32,
    pub max_record_secs: u32,
    pub device: String,
    // [history]
    pub history_enabled: bool,
    pub history_limit: usize,
    /// 隐私模式（DESIGN §10.2）：不写历史、不落盘失败音频、不显示识别文本
    pub privacy_mode: bool,
    /// 录音时暂停其它播放器（DESIGN §10.2「录音静音其他应用」，默认关）
    pub pause_media: bool,
    /// 背景降噪（RNNoise，DESIGN §9.7）：默认开，语音保真优先
    pub denoise: bool,
    /// 只录我的声音（声纹门控，D-051）：默认关，注册了声纹才可能生效
    pub voice_lock: bool,
    /// 声纹阈值：相似度低于它就把这段丢掉（默认 0.62，实测同人 0.91+/异人 <0.57）
    pub voice_lock_threshold: f32,
    /// 音源锁定（实验，D-065）：不用注册声纹，直接锁住这次录音里最先说话的人
    pub source_lock: bool,
    /// 标点模型（65MB，可选）：装了就在热词之后补一次标点
    pub punct: bool,
    // [insert]（M3：上屏方式，DESIGN §4.1）
    /// 全局上屏方式："paste"（默认）或 "keystroke"
    pub insert_mode: String,
    /// 按前台应用覆盖上屏方式：app 名 → 方式（大小写不敏感、支持包含匹配）
    pub insert_per_app: std::collections::BTreeMap<String, String>,
    // [hotkey]（壳侧使用，core 只负责原样保存与展示）
    pub hotkey: String,
    // [polish]（M2：可选润色，默认关；DESIGN §9.6）
    pub polish_enabled: bool,
    pub polish_level: String,
    pub polish_endpoint: String,
    pub polish_model: String,
    pub polish_api_key: String,
    pub polish_timeout_ms: u64,
    // [lan]（M5 路线②：局域网听写接口，默认关）
    pub lan_enabled: bool,
    pub lan_port: u16,
    pub lan_token: String,
    pub lan_allow_lan: bool,
    /// 诊断包上传地址（DESIGN §12.8 / D-114）；留空 = 不上传，只导到本地
    pub diag_upload_url: String,
    // [ui]
    pub launch_at_login: bool,
    /// 迷你主窗是否显示（R-103）；关闭窗口时写 false，菜单栏「显示主窗」写回 true
    pub mini_window: bool,
    /// 启动时读配置遇到的问题（供诊断页展示；None 表示正常）
    pub load_warning: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    #[serde(default)]
    engine: EngineSection,
    #[serde(default)]
    models: ModelsSection,
    #[serde(default)]
    audio: AudioSection,
    #[serde(default)]
    history: HistorySection,
    #[serde(default)]
    privacy: PrivacySection,
    #[serde(default)]
    hotkey: HotkeySection,
    #[serde(default)]
    polish: PolishSection,
    #[serde(default)]
    insert: InsertSection,
    #[serde(default)]
    lan: LanSection,
    #[serde(default)]
    diag: DiagSection,
    #[serde(default)]
    ui: UiSection,
}

#[derive(Debug, Deserialize, Default)]
struct DiagSection {
    upload_url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct LanSection {
    enabled: Option<bool>,
    port: Option<u16>,
    token: Option<String>,
    allow_lan: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct PrivacySection {
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct PolishSection {
    enabled: Option<bool>,
    level: Option<String>,
    endpoint: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct EngineSection {
    active: Option<String>,
    num_threads: Option<i32>,
}

#[derive(Debug, Deserialize, Default)]
struct ModelsSection {
    dir: Option<String>,
    vad: Option<String>,
    qwen3: Option<String>,
    mirror: Option<String>,
    proxy: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AudioSection {
    pause_media: Option<bool>,
    denoise: Option<bool>,
    voice_lock: Option<bool>,
    voice_lock_threshold: Option<f32>,
    source_lock: Option<bool>,
    punct: Option<bool>,
    min_record_ms: Option<u32>,
    max_record_secs: Option<u32>,
    device: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct HistorySection {
    enabled: Option<bool>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct HotkeySection {
    keys: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct InsertSection {
    mode: Option<String>,
    #[serde(default)]
    per_app: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
struct UiSection {
    launch_at_login: Option<bool>,
    onboarding_done: Option<bool>,
    onboarding_probe_done: Option<bool>,
    mini_window: Option<bool>,
}

impl Config {
    /// 默认配置目录（`~/.eryu`）；不读环境变量，环境覆盖在 `load()` 里做
    pub fn default_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(&home).join(".eryu")
    }
}

impl Default for Config {
    fn default() -> Self {
        let dir = Config::default_dir();
        let models = dir.join("models");
        Self {
            engine: DEFAULT_ENGINE.to_string(),
            num_threads: 2,
            model_dir: models.join("sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"),
            model_mirror: DEFAULT_MODEL_MIRROR.to_string(),
            model_proxy: String::new(),
            onboarding_done: false,
            onboarding_probe_done: false,
            vad_model: models.join("silero_vad.onnx"),
            qwen3_dir: models.join("sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25"),
            min_record_ms: MIN_RECORD_MS,
            max_record_secs: MAX_RECORD_SECS,
            device: String::new(),
            history_enabled: true,
            history_limit: HISTORY_LIMIT,
            privacy_mode: false,
            pause_media: false,
            denoise: true,
            voice_lock: false,
            voice_lock_threshold: 0.62,
            source_lock: false,
            punct: true,
            lan_enabled: false,
            lan_port: 8765,
            lan_token: String::new(),
            lan_allow_lan: false,
            diag_upload_url: DEFAULT_DIAG_UPLOAD.to_string(),
            insert_mode: "paste".into(),
            insert_per_app: Default::default(),
            // 默认单键：右 Option（用户 m01947/m02028：不要只会组合键；不产生字符、不和打字抢）
            hotkey: "ALT_R".to_string(),
            polish_enabled: false,
            polish_level: "off".to_string(),
            polish_endpoint: "https://api.deepseek.com".to_string(),
            polish_model: "deepseek-chat".to_string(),
            polish_api_key: String::new(),
            polish_timeout_ms: 3000,
            launch_at_login: false,
            mini_window: true,
            load_warning: None,
            dir,
        }
    }
}

impl Config {
    /// 读配置：config_dir 为 None 时用 `~/.eryu`。
    /// 文件不存在/损坏都退回默认值（不阻断启动），但把问题写进 `load_warning`。
    pub fn load(config_dir: Option<&Path>) -> Self {
        let mut cfg = Config::default();
        // 目录优先级：显式参数 > ERYU_CONFIG_DIR > ~/.eryu。
        // 注意必须在读 config.toml **之前**决定目录，否则「配置写在别处、读的却是默认目录」（D-077）。
        let dir = config_dir
            .map(|d| d.to_path_buf())
            .or_else(|| std::env::var("ERYU_CONFIG_DIR").ok().map(PathBuf::from))
            .unwrap_or_else(Config::default_dir);
        cfg.dir = dir;
        cfg.relocate_models();

        let mut existed = true;
        match fs::read_to_string(cfg.path()) {
            Ok(text) => {
                if let Err(e) = cfg.merge_toml(&text) {
                    // 解析失败会用默认值启动，但**不能**把用户写坏的文件晾在那儿等下一次保存覆盖掉：
                    // 先留一份副本，用户还能把自己的设置捡回来。
                    let broken = cfg.dir.join("config.toml.broken");
                    let saved = fs::copy(cfg.path(), &broken).is_ok();
                    cfg.load_warning = Some(if saved {
                        format!(
                            "config.toml 解析失败（已用默认值）：{e}。原文件备份在 {}",
                            broken.display()
                        )
                    } else {
                        format!("config.toml 解析失败（已用默认值）：{e}")
                    });
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => existed = false,
            Err(e) => cfg.load_warning = Some(format!("读取 config.toml 失败（已用默认值）：{e}")),
        }

        cfg.apply_env();
        // 首次运行把默认配置写出来：用户要能直接编辑 config.toml，设置页也才有"文件在哪"
        if !existed {
            if let Err(e) = cfg.save() {
                cfg.load_warning = Some(format!("无法写入默认 config.toml：{e}"));
            }
        }
        cfg
    }

    fn relocate_models(&mut self) {
        let models = self.dir.join("models");
        if std::env::var("ERYU_MODEL_DIR").is_err() {
            if let Some(name) = self.model_dir.file_name() {
                self.model_dir = models.join(name);
            }
        }
        if std::env::var("ERYU_QWEN3_DIR").is_err() {
            if let Some(name) = self.qwen3_dir.file_name() {
                self.qwen3_dir = models.join(name);
            }
        }
        if std::env::var("ERYU_VAD_MODEL").is_err() {
            if let Some(name) = self.vad_model.file_name() {
                self.vad_model = models.join(name);
            }
        }
    }

    /// 环境变量覆盖（目录与模型路径已在 `load()` 里处理，这里只管引擎与显式模型路径）
    fn apply_env(&mut self) {
        if let Ok(v) = std::env::var("ERYU_ENGINE") {
            self.engine = v;
        }
        if let Ok(v) = std::env::var("ERYU_MODEL_DIR") {
            self.model_dir = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("ERYU_VAD_MODEL") {
            self.vad_model = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("ERYU_QWEN3_DIR") {
            self.qwen3_dir = PathBuf::from(v);
        }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join("config.toml")
    }

    /// 把 TOML 文本合并进 self（失败不改动已有字段）。
    pub fn merge_toml(&mut self, text: &str) -> Result<(), String> {
        let f: FileConfig = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut next = self.clone();
        if let Some(v) = f.engine.active {
            next.engine = v;
        }
        if let Some(v) = f.engine.num_threads {
            next.num_threads = v.clamp(1, 8);
        }
        if let Some(v) = f.models.dir {
            next.model_dir = expand(&v);
        }
        if let Some(v) = f.models.vad {
            next.vad_model = expand(&v);
        }
        if let Some(v) = f.models.qwen3 {
            next.qwen3_dir = expand(&v);
        }
        if let Some(v) = f.models.mirror {
            next.model_mirror = v.trim().to_string();
        }
        if let Some(v) = f.models.proxy {
            next.model_proxy = v.trim().to_string();
        }
        if let Some(v) = f.audio.min_record_ms {
            next.min_record_ms = v.clamp(100, 10_000);
        }
        if let Some(v) = f.audio.max_record_secs {
            next.max_record_secs = v.clamp(5, 600);
        }
        if let Some(v) = f.audio.device {
            next.device = v;
        }
        if let Some(v) = f.history.enabled {
            next.history_enabled = v;
        }
        if let Some(v) = f.history.limit {
            next.history_limit = v.clamp(0, 2000);
        }
        if let Some(v) = f.privacy.enabled {
            next.privacy_mode = v;
        }
        if let Some(v) = f.audio.pause_media {
            next.pause_media = v;
        }
        if let Some(v) = f.audio.denoise {
            next.denoise = v;
        }
        if let Some(v) = f.audio.voice_lock {
            next.voice_lock = v;
        }
        if let Some(v) = f.audio.voice_lock_threshold {
            next.voice_lock_threshold = v.clamp(0.3, 0.95);
        }
        if let Some(v) = f.audio.source_lock {
            next.source_lock = v;
        }
        if let Some(v) = f.lan.enabled {
            next.lan_enabled = v;
        }
        if let Some(v) = f.lan.port {
            next.lan_port = v.clamp(1024, 65535);
        }
        if let Some(v) = f.lan.token {
            next.lan_token = v.trim().to_string();
        }
        if let Some(v) = f.lan.allow_lan {
            next.lan_allow_lan = v;
        }
        if let Some(v) = f.diag.upload_url {
            next.diag_upload_url = v.trim().to_string();
        }
        if let Some(v) = f.audio.punct {
            next.punct = v;
        }
        if let Some(m) = f.insert.mode.as_deref().and_then(normalize_mode) {
            next.insert_mode = m.to_string();
        }
        for (app, m) in f.insert.per_app {
            if let Some(m) = normalize_mode(&m) {
                next.insert_per_app.insert(app, m.to_string());
            }
        }
        if let Some(v) = f.hotkey.keys {
            next.hotkey = v;
        }
        if let Some(v) = f.polish.level {
            next.polish_level = normalize_level(&v)?;
        }
        match f.polish.enabled {
            Some(v) => next.polish_enabled = v,
            // 老配置里没有 enabled：按 level 是否 off 推断，迁移后保存会补上这个键
            None => next.polish_enabled = next.polish_level != "off",
        }
        if let Some(v) = f.polish.endpoint {
            next.polish_endpoint = v;
        }
        if let Some(v) = f.polish.model {
            next.polish_model = v;
        }
        if let Some(v) = f.polish.api_key {
            next.polish_api_key = v;
        }
        if let Some(v) = f.polish.timeout_ms {
            next.polish_timeout_ms = v.clamp(500, 20_000);
        }
        if let Some(v) = f.ui.launch_at_login {
            next.launch_at_login = v;
        }
        if let Some(v) = f.ui.onboarding_done {
            next.onboarding_done = v;
        }
        if let Some(v) = f.ui.onboarding_probe_done {
            next.onboarding_probe_done = v;
        }
        if let Some(v) = f.ui.mini_window {
            next.mini_window = v;
        }
        next.load_warning = None;
        *self = next;
        Ok(())
    }

    /// 有效配置的 JSON 表示（设置页直接绑定，不用解析 TOML）
    pub fn to_json(&self) -> String {
        let path = |p: &PathBuf| tilde_path(p);
        serde_json::json!({
            "dir": self.dir.to_string_lossy(),
            "engine": self.engine,
            "num_threads": self.num_threads,
            "model_dir": path(&self.model_dir),
            "vad_model": path(&self.vad_model),
            "qwen3_dir": path(&self.qwen3_dir),
            "min_record_ms": self.min_record_ms,
            "max_record_secs": self.max_record_secs,
            "device": self.device,
            "history_enabled": self.history_enabled,
            "privacy_mode": self.privacy_mode,
            "pause_media": self.pause_media,
            "denoise": self.denoise,
            "model_mirror": self.model_mirror,
            "model_proxy": self.model_proxy,
            "onboarding_done": self.onboarding_done,
            "onboarding_probe_done": self.onboarding_probe_done,
            "voice_lock": self.voice_lock,
            "voice_lock_threshold": self.voice_lock_threshold,
            "source_lock": self.source_lock,
            "punct": self.punct,
            "voice_enrolled": crate::voicelock::load(&self.dir).is_some(),
            "voice_model_ready": crate::voicelock::model_path(&self.dir).exists(),
            "punct_ready": crate::punct::model_path(&self.dir).exists(),
            "denoise_ready": crate::denoise::gtcrn_path(&self.dir).exists(),
            "insert_mode": self.insert_mode,
            "insert_per_app": self.insert_per_app,
            "history_limit": self.history_limit,
            "hotkey": self.hotkey,
            "polish_enabled": self.polish_enabled,
            "polish_level": self.polish_level,
            "polish_endpoint": self.polish_endpoint,
            "polish_model": self.polish_model,
            "polish_has_key": !self.polish_api_key.trim().is_empty(),
            "polish_timeout_ms": self.polish_timeout_ms,
            "lan_enabled": self.lan_enabled,
            "lan_port": self.lan_port,
            "lan_has_token": !self.lan_token.is_empty(),
            "lan_allow_lan": self.lan_allow_lan,
            "diag_upload_url": self.diag_upload_url,
            "launch_at_login": self.launch_at_login,
            "mini_window": self.mini_window,
            "load_warning": self.load_warning,
        })
        .to_string()
    }

    /// 合并设置页送来的 JSON（只认出现过的键；未知键忽略）
    pub fn merge_json(&mut self, json: &str) -> Result<(), String> {
        let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut next = self.clone();
        if let Some(s) = v.get("engine").and_then(|x| x.as_str()) {
            if !matches!(s, "sense" | "qwen3" | "stub") {
                return Err(format!("engine 只能是 sense/qwen3/stub，收到「{s}」"));
            }
            next.engine = s.to_string();
        }
        if let Some(n) = v.get("num_threads").and_then(|x| x.as_i64()) {
            next.num_threads = (n as i32).clamp(1, 8);
        }
        if let Some(s) = v.get("model_dir").and_then(|x| x.as_str()) {
            next.model_dir = expand(s);
        }
        if let Some(s) = v.get("vad_model").and_then(|x| x.as_str()) {
            next.vad_model = expand(s);
        }
        if let Some(s) = v.get("qwen3_dir").and_then(|x| x.as_str()) {
            next.qwen3_dir = expand(s);
        }
        if let Some(n) = v.get("min_record_ms").and_then(|x| x.as_u64()) {
            next.min_record_ms = (n as u32).clamp(100, 10_000);
        }
        if let Some(n) = v.get("max_record_secs").and_then(|x| x.as_u64()) {
            next.max_record_secs = (n as u32).clamp(5, 600);
        }
        if let Some(s) = v.get("device").and_then(|x| x.as_str()) {
            next.device = s.to_string();
        }
        if let Some(b) = v.get("history_enabled").and_then(|x| x.as_bool()) {
            next.history_enabled = b;
        }
        if let Some(b) = v.get("privacy_mode").and_then(|x| x.as_bool()) {
            next.privacy_mode = b;
        }
        if let Some(b) = v.get("pause_media").and_then(|x| x.as_bool()) {
            next.pause_media = b;
        }
        if let Some(b) = v.get("denoise").and_then(|x| x.as_bool()) {
            next.denoise = b;
        }
        if let Some(s) = v.get("model_mirror").and_then(|x| x.as_str()) {
            next.model_mirror = s.trim().to_string();
        }
        if let Some(s) = v.get("model_proxy").and_then(|x| x.as_str()) {
            next.model_proxy = s.trim().to_string();
        }
        if let Some(b) = v.get("onboarding_done").and_then(|x| x.as_bool()) {
            next.onboarding_done = b;
        }
        if let Some(b) = v.get("onboarding_probe_done").and_then(|x| x.as_bool()) {
            next.onboarding_probe_done = b;
        }
        if let Some(b) = v.get("voice_lock").and_then(|x| x.as_bool()) {
            next.voice_lock = b;
        }
        if let Some(x) = v.get("voice_lock_threshold").and_then(|x| x.as_f64()) {
            next.voice_lock_threshold = (x as f32).clamp(0.3, 0.95);
        }
        if let Some(b) = v.get("source_lock").and_then(|x| x.as_bool()) {
            next.source_lock = b;
        }
        if let Some(b) = v.get("punct").and_then(|x| x.as_bool()) {
            next.punct = b;
        }
        if let Some(m) = v
            .get("insert_mode")
            .and_then(|x| x.as_str())
            .and_then(normalize_mode)
        {
            next.insert_mode = m.to_string();
        }
        if let Some(map) = v.get("insert_per_app").and_then(|x| x.as_object()) {
            let mut out = std::collections::BTreeMap::new();
            for (app, val) in map {
                if let Some(m) = val.as_str().and_then(normalize_mode) {
                    out.insert(app.clone(), m.to_string());
                }
            }
            next.insert_per_app = out;
        }
        if let Some(b) = v
            .get("privacy")
            .and_then(|p| p.get("enabled"))
            .and_then(|x| x.as_bool())
        {
            next.privacy_mode = b;
        }
        if let Some(n) = v.get("history_limit").and_then(|x| x.as_u64()) {
            next.history_limit = (n as usize).clamp(0, 2000);
        }
        if let Some(s) = v.get("hotkey").and_then(|x| x.as_str()) {
            next.hotkey = s.to_string();
        }
        if let Some(b) = v.get("polish_enabled").and_then(|x| x.as_bool()) {
            next.polish_enabled = b;
        }
        if let Some(s) = v.get("polish_level").and_then(|x| x.as_str()) {
            next.polish_level = normalize_level(s)?;
            // 兼容旧界面：选了力度就等于开启，选 off 就是关闭
            if v.get("polish_enabled").is_none() {
                next.polish_enabled = next.polish_level != "off";
            }
        }
        if let Some(s) = v.get("polish_endpoint").and_then(|x| x.as_str()) {
            next.polish_endpoint = s.to_string();
        }
        if let Some(s) = v.get("polish_model").and_then(|x| x.as_str()) {
            next.polish_model = s.to_string();
        }
        if let Some(s) = v.get("polish_api_key").and_then(|x| x.as_str()) {
            next.polish_api_key = s.trim().to_string();
        }
        if let Some(b) = v.get("lan_enabled").and_then(|x| x.as_bool()) {
            next.lan_enabled = b;
        }
        if let Some(n) = v.get("lan_port").and_then(|x| x.as_u64()) {
            next.lan_port = (n as u16).clamp(1024, 65535);
        }
        if let Some(t) = v.get("lan_token").and_then(|x| x.as_str()) {
            next.lan_token = t.trim().to_string();
        }
        if let Some(b) = v.get("lan_allow_lan").and_then(|x| x.as_bool()) {
            next.lan_allow_lan = b;
        }
        if let Some(b) = v.get("diag_upload_url").and_then(|x| x.as_str()) {
            next.diag_upload_url = b.to_string();
        }
        if let Some(n) = v.get("polish_timeout_ms").and_then(|x| x.as_u64()) {
            next.polish_timeout_ms = n.clamp(500, 20_000);
        }
        if let Some(b) = v.get("launch_at_login").and_then(|x| x.as_bool()) {
            next.launch_at_login = b;
        }
        if let Some(b) = v.get("mini_window").and_then(|x| x.as_bool()) {
            next.mini_window = b;
        }
        next.load_warning = None;
        *self = next;
        Ok(())
    }

    /// 渲染成带注释的 TOML（保存时整份重写）
    pub fn to_toml(&self) -> String {
        let tilde = tilde_path;
        // Windows 路径含反斜杠：TOML 基本字符串里必须转义，否则读回来整份配置都解析失败（I-061）
        let esc_str = |t: &str| -> String { escape_toml(t) };
        let esc = |p: &std::path::Path| -> String { esc_str(&tilde(p)) };
        format!(
            r#"# Eryu 配置（保存即整份重写；改完不需要重启，下次录音生效）
#
# engine.active: sense（默认，快且省内存）| qwen3（短句精度档，内存约 1.6G）| stub（自测）
# 模型目录：scripts/fetch-models.sh 会把模型装到 ~/.eryu/models
# 快捷键在应用内「设置 → 快捷键」里改，这里同步保存一份

[engine]
active = "{engine}"
num_threads = {threads}

[models]
dir = "{model_dir}"
vad = "{vad}"
qwen3 = "{qwen3}"
# 模型下载镜像：留空走官方 GitHub release；填了就用「镜像地址 + 文件名」，例如
# http://your-host/eryu-models（把同批发布文件平铺在根目录即可）
mirror = "{model_mirror}"
proxy = "{model_proxy}"

[audio]
# 短于此长度的录音按「太短」处理（DESIGN §4.5）
min_record_ms = {min_record_ms}
# 单次录音上限，超时自动结束
max_record_secs = {max_record_secs}
# 输入设备名，空 = 系统默认
device = "{device}"
# 录音时暂停其它播放器（Music / Spotify），松手后恢复
pause_media = {pause_media}
# 背景降噪（RNNoise）：照顾「语音保真」，说话时几乎不动，只在噪声段压
denoise = {denoise}
# 只录我的声音（声纹门控）：需要先在设置里「录 10 秒声纹」，默认关
voice_lock = {voice_lock}
# 低于这个相似度就把这段丢掉（0.3–0.95，默认 0.62）
voice_lock_threshold = {voice_lock_threshold}
source_lock = {source_lock}
# 标点模型（可选，65MB）：装了就用它补逗号/句号，没装就只在句末补「。」
punct = {punct}

# [insert] 上屏方式（M3）：paste = 写剪贴板后合成粘贴（默认，最快最稳）
#                   keystroke = 逐字打字（安全输入/某些终端/网页编辑器更可靠）
[insert]
mode = "{insert_mode}"
# 按前台应用覆盖：应用名（大小写不敏感，支持包含匹配）→ 方式
per_app = {{{per_app}}}

[history]
enabled = {history_enabled}
limit = {history_limit}

[privacy]
# 隐私模式：不写历史、不落盘失败音频、浮层不显示识别文本（DESIGN §10.2）
# 开启后「重试上次失败」不可用（失败录音不留盘）
enabled = {privacy_mode}

[polish]
# 可选润色：off（默认，全程本地）| light | medium | strong
# 开启后，转写文本会发到下面这个兼容 OpenAI 的服务端做整理（DESIGN §9.6）
enabled = {polish_enabled}
level = "{polish_level}"
endpoint = "{polish_endpoint}"
model = "{polish_model}"
api_key = "{polish_api_key}"
timeout_ms = {polish_timeout_ms}

[lan]
# 局域网听写接口（M5 路线②）：别的设备把 WAV 发过来，本机识别后回文字。默认关。
# 默认只监听 127.0.0.1；allow_lan = true 才监听 0.0.0.0（这时一定要配 token）
enabled = {lan_enabled}
port = {lan_port}
token = "{lan_token}"
allow_lan = {lan_allow_lan}

[diag]
# 诊断包上传地址（DESIGN §12.8）：留空 = 只导出到本地，不上传
upload_url = "{diag_upload_url}"

[hotkey]
keys = "{hotkey}"

[ui]
launch_at_login = {launch_at_login}
# 新手引导是否完成；onboarding_probe_done 是第 3 步「试说一句」是否成功过一次
onboarding_done = {onboarding_done}
onboarding_probe_done = {onboarding_probe_done}
# 迷你主窗（R-103）：关掉窗口后不再自动出现，菜单栏「显示主窗」恢复
mini_window = {mini_window}
"#,
            engine = self.engine,
            threads = self.num_threads,
            model_dir = esc(&self.model_dir),
            vad = esc(&self.vad_model),
            qwen3 = esc(&self.qwen3_dir),
            model_mirror = esc_str(&self.model_mirror),
            model_proxy = esc_str(&self.model_proxy),
            min_record_ms = self.min_record_ms,
            max_record_secs = self.max_record_secs,
            device = esc_str(&self.device),
            pause_media = self.pause_media,
            denoise = self.denoise,
            voice_lock = self.voice_lock,
            voice_lock_threshold = self.voice_lock_threshold,
            source_lock = self.source_lock,
            punct = self.punct,
            insert_mode = self.insert_mode,
            per_app = per_app_body(&self.insert_per_app),
            history_enabled = self.history_enabled,
            history_limit = self.history_limit,
            privacy_mode = self.privacy_mode,
            hotkey = self.hotkey,
            polish_enabled = self.polish_enabled,
            polish_level = self.polish_level,
            polish_endpoint = self.polish_endpoint,
            polish_model = self.polish_model,
            polish_api_key = self.polish_api_key,
            polish_timeout_ms = self.polish_timeout_ms,
            lan_enabled = self.lan_enabled,
            lan_port = self.lan_port,
            lan_token = self.lan_token.clone(),
            lan_allow_lan = self.lan_allow_lan,
            diag_upload_url = self.diag_upload_url,
            launch_at_login = self.launch_at_login,
            onboarding_done = self.onboarding_done,
            onboarding_probe_done = self.onboarding_probe_done,
            mini_window = self.mini_window,
        )
    }

    /// 保存：先写临时文件再 rename（失败保留旧文件）
    pub fn save(&self) -> Result<(), String> {
        fs::create_dir_all(&self.dir).map_err(|e| format!("创建配置目录失败：{e}"))?;
        let path = self.path();
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, self.to_toml()).map_err(|e| format!("写配置失败：{e}"))?;
        fs::rename(&tmp, &path).map_err(|e| format!("替换配置失败：{e}"))?;
        Ok(())
    }

    /// 当前引擎的模型是否齐备
    pub fn models_ready(&self) -> bool {
        match self.engine.as_str() {
            "stub" => true,
            "qwen3" => {
                self.qwen3_dir.join("encoder.int8.onnx").exists()
                    && self.qwen3_dir.join("decoder.int8.onnx").exists()
                    && self.qwen3_dir.join("tokenizer").exists()
            }
            _ => {
                self.model_dir.join("model.int8.onnx").exists()
                    && self.model_dir.join("tokens.txt").exists()
            }
        }
    }

    pub fn vad_ready(&self) -> bool {
        self.vad_model.exists()
    }

    /// 失败音频暂存目录
    pub fn pending_dir(&self) -> PathBuf {
        self.dir.join("pending")
    }

    /// 失败音频文件（24h TTL，见 lib.rs FAILED_CLIP_TTL_SECS）
    pub fn failed_clip_path(&self) -> PathBuf {
        self.pending_dir().join("last_failed.wav")
    }

    /// 校验一段 TOML 能否被接受（设置页保存前用；不落盘）
    pub fn validate_toml(base: &Config, text: &str) -> Result<Config, String> {
        let mut next = base.clone();
        next.merge_toml(text)?;
        if !matches!(next.engine.as_str(), "sense" | "qwen3" | "stub") {
            return Err(format!(
                "engine.active 只能是 sense / qwen3 / stub，收到「{}」",
                next.engine
            ));
        }
        Ok(next)
    }
}

fn normalize_level(s: &str) -> Result<String, String> {
    match crate::polish::Level::parse(s) {
        Some(l) => Ok(l.as_str().to_string()),
        None => Err(format!(
            "polish.level 只能是 off / light / medium / strong，收到「{s}」"
        )),
    }
}

/// 润色是否真的会发网络请求
/// 上屏方式白名单："paste" / "keystroke"（未知值返回 None，调用方保留旧值）
pub fn normalize_mode(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "paste" | "粘贴" => Some("paste"),
        "keystroke" | "type" | "打字" | "逐字" => Some("keystroke"),
        _ => None,
    }
}

/// TOML 基本字符串的转义（反斜杠与双引号）
fn escape_toml(t: &str) -> String {
    t.replace('\\', "\\\\").replace('"', "\\\"")
}

/// 把「按应用覆盖」渲染成 TOML 内联表的表体（`"Notepad.exe" = "keystroke"`）。
/// 之前这里恒为 `{}`，导致设置页改的覆盖项在保存后丢失（I-070）。
fn per_app_body(map: &std::collections::BTreeMap<String, String>) -> String {
    if map.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::new();
    for (name, mode) in map {
        parts.push(format!(
            "\"{}\" = \"{}\"",
            escape_toml(name),
            escape_toml(mode)
        ));
    }
    parts.join(", ")
}

/// 该前台应用应该用哪种上屏方式：先按 app 覆盖（包含匹配），再全局
pub fn mode_for(cfg: &Config, app: &str) -> String {
    let app_lower = app.to_lowercase();
    if !app_lower.is_empty() {
        for (name, mode) in &cfg.insert_per_app {
            if name.to_lowercase().contains(&app_lower) || app_lower.contains(&name.to_lowercase())
            {
                return mode.clone();
            }
        }
    }
    cfg.insert_mode.clone()
}

pub fn polish_active(cfg: &Config) -> bool {
    cfg.polish_enabled && !cfg.polish_api_key.trim().is_empty()
}

/// `$HOME` 前缀缩成 `~`（展示/保存用）
pub fn home_dir() -> String {
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok())
        .unwrap_or_default()
}

pub fn tilde_path(p: &Path) -> String {
    // Windows 上不做 `~` 缩略：`~\AppData` 这种带反斜杠的形态在 TOML/展示里都容易被当转义，
    // 而且 expand() 只认 `~/`（I-062）。
    if cfg!(windows) {
        return p.to_string_lossy().to_string();
    }
    let home = home_dir();
    let s = p.to_string_lossy().to_string();
    if !home.is_empty() && s.starts_with(&home) {
        s.replacen(&home, "~", 1)
    } else {
        s
    }
}

fn expand(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        return PathBuf::from(home_dir()).join(rest);
    }
    PathBuf::from(p)
}

#[cfg(test)]
mod privacy_tests {
    use super::*;

    fn make() -> Config {
        Config::load(Some(&std::env::temp_dir().join("eryu-privacy-test")))
    }

    #[test]
    fn privacy_json_roundtrip() {
        let mut c = make();
        assert!(!c.privacy_mode);
        c.merge_json(r#"{"privacy_mode": true}"#).unwrap();
        assert!(c.privacy_mode);
        let j = c.to_json();
        assert!(j.contains("\"privacy_mode\":true"), "{j}");
        let mut back = make();
        back.merge_json(&j).unwrap();
        assert!(back.privacy_mode);
    }

    #[test]
    fn per_app_survives_save_and_reload() {
        let dir = std::env::temp_dir().join(format!("eryu-perapp-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg = Config::load(Some(&dir));
        cfg.insert_per_app
            .insert("Notepad.exe".into(), "keystroke".into());
        cfg.insert_per_app
            .insert("Terminal".into(), "keystroke".into());
        cfg.save().expect("save");
        let back = Config::load(Some(&dir));
        assert_eq!(
            back.insert_per_app.get("Notepad.exe").map(String::as_str),
            Some("keystroke")
        );
        assert_eq!(
            back.insert_per_app.get("Terminal").map(String::as_str),
            Some("keystroke")
        );
        assert_eq!(mode_for(&back, "notepad.exe"), "keystroke");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn onboarding_state_survives_toml() {
        let dir = std::env::temp_dir().join(format!("eryu-onb-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut c = Config::load(Some(&dir));
        assert!(!c.onboarding_probe_done);
        assert!(c.mini_window);
        c.merge_json(
            r#"{"onboarding_done": true, "onboarding_probe_done": true, "mini_window": false}"#,
        )
        .unwrap();
        c.save().expect("save");
        let back = Config::load(Some(&dir));
        assert!(back.onboarding_done, "onboarding_done 应落盘（I-101）");
        assert!(back.onboarding_probe_done);
        assert!(!back.mini_window);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pause_media_roundtrip() {
        let mut c = make();
        assert!(!c.pause_media);
        c.merge_json(r#"{"pause_media": true}"#).unwrap();
        assert!(c.pause_media);
        assert!(c.to_json().contains("\"pause_media\":true"));
        assert!(c.to_toml().contains("pause_media = true"));
    }

    #[test]
    fn privacy_toml_section() {
        let mut c = make();
        c.merge_toml("[privacy]\nenabled = true\n").unwrap();
        assert!(c.privacy_mode);
        assert!(c.to_toml().contains("[privacy]"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polish_enabled_migrates_from_old_level() {
        let mut c = Config::default();
        let dir = std::env::temp_dir().join(format!("eryu-cfg-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        c.dir = dir.clone();
        // 老配置：只有 level = "light"，没有 enabled → 迁移成开启
        let old = "[polish]\nlevel = \"light\"\napi_key = \"sk-x\"\n";
        c.merge_toml(old).unwrap();
        assert!(c.polish_enabled);
        // 明确写 enabled = false，即便 level 还在，也必须是关闭
        let new = "[polish]\nenabled = false\nlevel = \"light\"\n";
        c.merge_toml(new).unwrap();
        assert!(!c.polish_enabled);
        assert!(!polish_active(&c) || c.polish_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roundtrip_toml_keeps_values() {
        let mut c = Config::default();
        c.engine = "qwen3".into();
        c.min_record_ms = 600;
        c.history_enabled = false;
        let text = c.to_toml();
        let mut back = Config::default();
        back.merge_toml(&text).expect("parse");
        assert_eq!(back.engine, "qwen3");
        assert_eq!(back.min_record_ms, 600);
        assert!(!back.history_enabled);
    }

    #[test]
    fn bad_toml_is_rejected_without_touching_fields() {
        let mut c = Config::default();
        c.engine = "sense".into();
        assert!(c.merge_toml("engine = [broken").is_err());
        assert_eq!(c.engine, "sense");
    }

    #[test]
    fn clamps_out_of_range_values() {
        let mut c = Config::default();
        c.merge_toml("[audio]\nmin_record_ms = 1\nmax_record_secs = 100000\n")
            .expect("parse");
        assert_eq!(c.min_record_ms, 100);
        assert_eq!(c.max_record_secs, 600);
    }

    #[test]
    fn per_app_mode_matches_and_falls_back() {
        let dir = std::env::temp_dir().join(format!("eryu-perapp-mode-{}", std::process::id()));
        let mut cfg = Config::load(Some(&dir));
        cfg.insert_mode = "paste".into();
        cfg.insert_per_app
            .insert("Code.exe".into(), "keystroke".into());
        cfg.insert_per_app
            .insert("notepad".into(), "keystroke".into());

        // 大小写不敏感 + 双向包含（前台拿到的是进程名，配置里可能写全名/短名）
        assert_eq!(mode_for(&cfg, "notepad.exe"), "keystroke");
        assert_eq!(mode_for(&cfg, "NOTEPAD.EXE"), "keystroke");
        assert_eq!(mode_for(&cfg, "Code.exe"), "keystroke");
        // 没匹配上就用默认
        assert_eq!(mode_for(&cfg, "chrome.exe"), "paste");
        // 前台名拿不到（空串）不能误命中
        assert_eq!(mode_for(&cfg, ""), "paste");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
