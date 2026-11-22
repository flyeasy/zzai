//! Eryu core：命令队列 + 单 worker 线程（DESIGN §3、§3.1）
//!
//! 线程模型：
//! - 采音回调线程（cpal）只做 混单声道 → 重采样 → try_send
//! - worker 线程独占 recognizer，串行处理所有命令与事件
//! - 解码在独立线程跑（VAD 切段 + 逐段识别），结果回送 worker
//! - 每个入口 catch_unwind：panic 记为 error 事件并停用实例，绝不穿过 FFI

// serde_json::json! 的字段数已接近默认递归上限（0.8.1 新增 ui 字段后触顶）
#![recursion_limit = "256"]

pub mod asr;
pub mod audio;
pub mod config;
pub mod denoise;
pub mod events;
pub mod fsutil;
pub mod hotwords;
pub mod lan;
pub mod models;
pub mod pipeline;
pub mod polish;
pub mod profiles;
pub mod punct;
pub mod session;
pub mod sourcelock;
pub mod store;
pub mod vad;
pub mod voicelock;

mod ffi;

pub use ffi::*;

use std::os::raw::{c_char, c_void};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// 标点模型的懒加载缓存（第一次解码时加载约 100ms，之后复用；没装模型时记住失败）
static PUNCT_CACHE: punct::Cache = punct::Cache::new();

pub const ABI_MAJOR: u32 = 1;
pub const ABI_MINOR: u32 = 14;

/// worker 主循环的 tick（状态机看门狗、采音排空都靠它）
pub const TICK_MS: u64 = 15;
/// 麦克风电平事件间隔（~15Hz）
pub const METER_INTERVAL_MS: u64 = 66;
/// 准流式预览的重解间隔（DESIGN §4.6）
pub const PARTIAL_INTERVAL_MS: u64 = 900;
/// 准流式自适应边界：最快 350ms 一轮（短句很轻），最慢 1200ms（长录音时解码变慢，别把机器压满）
pub const PARTIAL_MIN_MS: u64 = 350;
pub const PARTIAL_MAX_MS: u64 = 1200;

/// 准流式预览只看最近这么多秒：录到 1 分钟以上时，整段重跑会越跑越慢（RTF 0.05 也要 3–5 秒）
pub const PARTIAL_TAIL_SECS: usize = 20;

/// 松手时等「正在跑的那一块」提交的最长时间（DESIGN §4.6）
pub const COMMIT_WAIT_MS: u64 = 900;

/// 预览窗口：录音超过 PARTIAL_TAIL_SECS 时只取尾部；返回 (切片, 是否被截断)
pub fn partial_window(pcm: &[f32]) -> (&[f32], bool) {
    let budget = PARTIAL_TAIL_SECS * audio::TARGET_RATE as usize;
    if pcm.len() > budget {
        (&pcm[pcm.len() - budget..], true)
    } else {
        (pcm, false)
    }
}

/// 下一轮准流式预览的间隔：按上一轮解码耗时自适应（2 倍余量，夹在 350–1200ms）
pub fn partial_interval_ms(last_decode_ms: u64) -> u64 {
    last_decode_ms
        .saturating_mul(2)
        .clamp(PARTIAL_MIN_MS, PARTIAL_MAX_MS)
}
/// 失败音频暂存 TTL（用户 m00819 确认：24h）
pub const FAILED_CLIP_TTL_SECS: u64 = 24 * 3600;

pub type EventCb = extern "C" fn(*const c_char, *mut c_void);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 壳注册的回调；指针以 usize 存，便于跨线程（回调本身由壳保证线程安全）
pub struct CbSlot {
    cb: Option<(EventCb, usize)>,
}

impl Default for CbSlot {
    fn default() -> Self {
        Self { cb: None }
    }
}

impl CbSlot {
    pub fn set(&mut self, cb: EventCb, ctx: *mut c_void) {
        self.cb = Some((cb, ctx as usize));
    }
    pub fn clear(&mut self) {
        self.cb = None;
    }
    pub fn get(&self) -> Option<(EventCb, *mut c_void)> {
        self.cb.map(|(cb, ctx)| (cb, ctx as *mut c_void))
    }
}

enum Cmd {
    Down,
    Up,
    Cancel,
    Submit {
        task_id: String,
        status: String,
        detail: String,
    },
    Decoded {
        task_id: String,
        result: Result<pipeline::PipelineResult, String>,
        speech_ms: u64,
        decode_ms: u64,
        pcm: Vec<f32>,
        /// 润色前的原文（热词已应用）；润色关闭时与 text 相同
        raw_text: String,
        /// 润色失败的原因（非致命：已回退原文）
        polish_note: Option<String>,
    },
    DebugText(String),
    DebugRecord(f32),
    /// 把一段 16k PCM 当成一次「刚松手」的录音走完整解码链路（无麦克风的确定性自测）
    DecodePcm(Vec<f32>),
    RetryFailed,
    /// 渐进解码：一块「已闭合」音频解完（先进预览，松手时并入最终文本）
    Partial {
        text: String,
        ms: u64,
        up_to: usize,
        /// true = 这一块算「已说过去」，拼进前缀；false = 只是预览（尾巴还会变）
        commit: bool,
    },
    /// 只重试某一段（部分成功里识别失败的那几段）
    RetrySegment(usize),
    /// 单段重试结果：成功给文本，失败给原因
    SegmentDone {
        index: usize,
        result: Result<String, String>,
    },
    MeterStart,
    MeterStop,
    ApplyConfig(Box<config::Config>, u64),
    SetEngine(String),
    HotwordsChanged(usize),
    NoteApp(String),
    EngineBuilt {
        engine_id: String,
        result: Result<Box<dyn asr::AsrEngine>, String>,
    },
    PanicTest,
    Barrier(Sender<()>),
    Quit,
}

pub struct EryuCore {
    /// 正在下载的模型 id（同一时刻只允许一个）
    pulling: Mutex<Option<String>>,
    tx: Sender<Cmd>,
    closing: Arc<AtomicBool>,
    cb: Arc<Mutex<CbSlot>>,
    last: Arc<Mutex<Option<String>>>,
    cfg: Arc<Mutex<config::Config>>,
    store: Arc<Mutex<store::Store>>,
    hotwords: Arc<Mutex<hotwords::Hotwords>>,
    profiles: Arc<Mutex<profiles::Profiles>>,
    /// 配置版本号（壳每次改配置 +1；worker 丢弃过期命令，I-075）
    cfg_gen: Arc<std::sync::atomic::AtomicU64>,
    /// 已经写进共享快照的版本号（worker 用它在锁内判断「这条是不是过期了」，I-075）
    cfg_applied: Arc<std::sync::atomic::AtomicU64>,
    /// 可重试的失败段序号（worker 出结果时刷新）
    failed_indexes: Arc<Mutex<Vec<usize>>>,
    /// 单段重试是否还在跑（退出前要等它，否则线程在 dylib 销毁后调用引擎会 aborts）
    retry_busy: Arc<AtomicBool>,
    /// 声纹门控（「只录我的声音」）：懒加载，注册/关闭后要重建
    voice_gate: Arc<Mutex<Option<voicelock::Gate>>>,
    /// 自测模式：`--decode-wav` / `--soak` / `--self-test` 等自动化路径不写历史、不落盘
    test_mode: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl EryuCore {
    pub fn start(config_dir: Option<&std::path::Path>) -> Result<Arc<Self>, String> {
        let cfg = config::Config::load(config_dir);
        let store = store::Store::load(&cfg.dir, cfg.history_enabled, cfg.history_limit);
        let hotwords = Arc::new(Mutex::new(hotwords::Hotwords::load(&cfg.dir)));
        let profiles = Arc::new(Mutex::new(profiles::Profiles::load(&cfg.dir)));
        let cfg = Arc::new(Mutex::new(cfg));
        let store = Arc::new(Mutex::new(store));
        let (engine, engine_fallback): (Arc<Mutex<Box<dyn asr::AsrEngine>>>, Arc<AtomicBool>) = {
            let c = cfg.lock().unwrap();
            match asr::make_engine(&c) {
                Ok(e) => (Arc::new(Mutex::new(e)), Arc::new(AtomicBool::new(false))),
                Err(e) => {
                    eprintln!("[eryu] 引擎不可用，回退 stub：{e}");
                    // 回退到 stub 只是为了让程序能起来（界面/设置还能用），
                    // 绝不能把 stub 的固定假文本当识别结果上屏（见 on_decoded 的守卫）。
                    (
                        Arc::new(Mutex::new(
                            Box::new(asr::StubEngine::default()) as Box<dyn asr::AsrEngine>
                        )),
                        Arc::new(AtomicBool::new(true)),
                    )
                }
            }
        };
        let (tx, rx) = channel::<Cmd>();
        let cb = Arc::new(Mutex::new(CbSlot::default()));
        let last = Arc::new(Mutex::new(None));
        let closing = Arc::new(AtomicBool::new(false));
        let failed_indexes = Arc::new(Mutex::new(Vec::new()));
        let retry_busy = Arc::new(AtomicBool::new(false));
        let voice_gate: Arc<Mutex<Option<voicelock::Gate>>> = Arc::new(Mutex::new(None));
        let test_mode = Arc::new(AtomicBool::new(false));
        let cfg_gen = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let cfg_applied = Arc::new(std::sync::atomic::AtomicU64::new(0));

        let worker = Worker {
            cfg: cfg.clone(),
            store: store.clone(),
            hotwords: hotwords.clone(),
            cfg_gen: cfg_gen.clone(),
            cfg_applied: cfg_applied.clone(),
            cfg_gen_seen: 0,
            profiles: profiles.clone(),
            failed_indexes: failed_indexes.clone(),
            retry_busy: retry_busy.clone(),
            voice_gate: voice_gate.clone(),
            test_mode: test_mode.clone(),
            sim_pcm: None,
            sim_pos: 0,
            sim_last: Instant::now(),
            engine_fallback,
            engine,
            rx,
            tx: tx.clone(),
            m: session::Machine::new(),
            cb: cb.clone(),
            last: last.clone(),
            seq: 0,
            capture: None,
            recorded: Vec::new(),
            recorded_at: None,
            decoding: false,
            debug_stop_at: None,
            foreground_app: String::new(),
            partial_busy: false,
            partial_alive: Arc::new(AtomicBool::new(false)),
            partial_commit: false,
            partial_up_to: 0,
            partial_due: None,
            partial_seen: 0,
            partial_last: String::new(),
            partial_last_ms: 0,
            prog_pos: 0,
            forced_mic_fail_at: None,
            prog_text: String::new(),
            waiting_commit: None,
            commit_deadline: None,
            failed_segments: Vec::new(),
            decode_started: None,
            wait_started: None,
            commit_wait_ms: 0,
            engine_wait_ms: 0,
            partial_cancel: Arc::new(AtomicBool::new(false)),
            meter: None,
            meter_scratch: Vec::new(),
            meter_last: Instant::now(),
            rec_meter_last: Instant::now() - Duration::from_millis(METER_INTERVAL_MS),
            last_failed: None,
            pending_history_id: None,
            last_device: None,
            lan_cfg: None,
            closing: closing.clone(),
        };
        let handle = thread::Builder::new()
            .name("eryu-worker".into())
            .spawn(move || worker.run())
            .map_err(|e| format!("启动 worker 线程失败：{e}"))?;

        Ok(Arc::new(EryuCore {
            pulling: Mutex::new(None),
            hotwords,
            cfg_gen,
            cfg_applied,
            tx,
            closing,
            cb,
            last,
            cfg,
            store,
            profiles,
            failed_indexes,
            retry_busy,
            voice_gate,
            test_mode,
            worker: Mutex::new(Some(handle)),
        }))
    }

    pub fn send(&self, cmd: Cmd) -> i32 {
        if self.closing.load(Ordering::SeqCst) {
            return -2;
        }
        match self.tx.send(cmd) {
            Ok(()) => 0,
            Err(_) => -2,
        }
    }

    pub fn set_cb(&self, cb: EventCb, ctx: *mut c_void) -> i32 {
        if self.closing.load(Ordering::SeqCst) {
            return -2;
        }
        self.cb.lock().unwrap().set(cb, ctx);
        0
    }

    pub fn clear_cb(&self) {
        self.cb.lock().unwrap().clear();
    }

    /// 自测模式：不写历史、不落盘（`--decode-wav` / `--soak` / `--self-test` 用）
    pub fn set_test_mode(&self, on: bool) {
        self.test_mode.store(on, Ordering::Relaxed);
    }

    pub fn last_result(&self) -> Option<String> {
        self.last.lock().unwrap().clone()
    }

    /// 清掉「上一次识别结果」的内存副本（不动落盘的 `last_result.txt`）。
    ///
    /// 「测试麦克风」要用它：启动时 `recover_pending_result()` 会把上次没上屏的文字读回 `last`，
    /// 如果测麦前不清，录音什么都没听到时会把它当成「刚听到的」显示出来（I-156）。
    pub fn clear_last_result(&self) {
        *self.last.lock().unwrap() = None;
    }

    pub fn set_last_result(&self, v: Option<String>) {
        *self.last.lock().unwrap() = v;
    }

    /// 用户已取回结果：删掉崩溃恢复用的暂存（历史保留）
    pub fn clear_pending_result_files(&self) {
        let cfg = self.config_snapshot();
        let _ = std::fs::remove_file(cfg.pending_dir().join("last_result.txt"));
    }

    pub fn config_snapshot(&self) -> config::Config {
        self.cfg.lock().unwrap().clone()
    }

    /// 立即把配置写进共享快照（读 config_json 马上能看到新值），再交 worker 落盘/生效
    pub fn apply_config_now(&self, cfg: config::Config) -> i32 {
        use std::sync::atomic::Ordering::SeqCst;
        // 版本号：worker 里排队的旧 ApplyConfig 不能覆盖刚写进去的新配置（I-075）
        let gen = self.cfg_gen.fetch_add(1, SeqCst) + 1;
        {
            let mut g = self.cfg.lock().unwrap();
            *g = cfg.clone();
            // 版本号与快照一起写，worker 在锁内比这个值，就能判出「我来晚了」
            self.cfg_applied.store(gen, SeqCst);
        }
        self.send(Cmd::ApplyConfig(Box::new(cfg), gen))
    }

    /// 热词表 JSON（给设置页）
    pub fn profiles_json(&self) -> String {
        self.profiles.lock().unwrap().to_json()
    }

    pub fn set_profiles_json(&self, text: &str) -> Result<(), String> {
        let mut p = self.profiles.lock().unwrap();
        let parsed = profiles::Profiles::from_json(text)?;
        p.replace(parsed.list().to_vec(), Some(parsed.active().to_string()))
    }

    pub fn set_active_profile(&self, name: &str) -> Result<(), String> {
        let mut p = self.profiles.lock().unwrap();
        p.set_active(name)
    }

    /// 单段重试（部分成功里失败的那几段）：索引来自 `failed_indexes()`
    pub fn retry_segment(&self, index: usize) -> Result<(), String> {
        if !self.failed_indexes().contains(&index) {
            return Err(format!("第 {index} 段没有可重试的音频"));
        }
        match self.send(Cmd::RetrySegment(index)) {
            0 => Ok(()),
            -2 => Err("Eryu 正在退出，没法重试".to_string()),
            n => Err(format!("发送重试请求失败（{n}）")),
        }
    }

    /// 当前可重试的失败段序号（worker 每次出结果后刷新）
    pub fn failed_indexes(&self) -> Vec<usize> {
        self.failed_indexes
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    /// 自测：把一段 16k 单声道 WAV 当成一次录音走完整链路（不需要麦克风）
    pub fn decode_wav(&self, path: &str) -> Result<(), String> {
        let pcm = audio::read_wav16(std::path::Path::new(path))?;
        match self.send(Cmd::DecodePcm(pcm)) {
            0 => Ok(()),
            -2 => Err("Eryu 正在退出".to_string()),
            n => Err(format!("发送解码请求失败（{n}）")),
        }
    }

    pub fn hotwords_json(&self) -> String {
        self.hotwords.lock().unwrap().to_json()
    }

    /// 上传诊断包（JSON 字符串）到指定地址：POST application/json，15 秒超时。
    /// 返回服务端响应体（截断到 500 字），失败给中文原因。
    pub fn upload_diag(&self, url: &str, body: &str) -> Result<String, String> {
        if url.trim().is_empty() {
            return Err("没设上传地址（[diag] upload_url）：诊断包已存在本地，可以直接发给我".into());
        }
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(15))
            .build();
        let resp = agent
            .post(url.trim())
            .set("Content-Type", "application/json; charset=utf-8")
            .set("User-Agent", concat!("Eryu/", env!("CARGO_PKG_VERSION")))
            .send_string(body);
        match resp {
            Ok(r) => {
                let status = r.status();
                if !(200..300).contains(&status) {
                    return Err(format!("服务端返回 HTTP {status}"));
                }
                let mut text = String::new();
                let mut reader = r.into_reader();
                let mut buf = vec![0u8; 2000];
                use std::io::Read as _;
                if let Ok(n) = reader.read(&mut buf) {
                    text = String::from_utf8_lossy(&buf[..n]).into_owned();
                }
                Ok(text.chars().take(500).collect())
            }
            Err(ureq::Error::Status(code, _)) => match code {
                404 => Err("服务端还没有开放诊断上传（404）：诊断包已存本地，把它发给我即可".into()),
                413 => Err("诊断包太大，服务端拒收（413）".into()),
                _ => Err(format!("上传失败：HTTP {code}")),
            },
            Err(e) => Err(format!("上传失败：{e}（网络不通或地址写错）")),
        }
    }

    /// 试着润色一句话（设置页「测试连接」用）。返回润色结果或中文错误。
    /// 问服务端有哪些模型（用已保存的 Key；不新开输入口，避免 Swift 侧再存一份密钥）
    pub fn polish_models(&self) -> Result<Vec<String>, String> {
        let c = self.config_snapshot();
        polish::list_models(&c.polish_endpoint, &c.polish_api_key, 8000)
    }

    pub fn polish_test(&self) -> Result<String, String> {
        let c = self.config_snapshot();
        let prof_cur = self.profiles.lock().unwrap().current();
        let mut level = polish::Level::parse(&prof_cur.level).unwrap_or(polish::Level::Off);
        if level == polish::Level::Off {
            level = polish::Level::parse(&c.polish_level).unwrap_or(polish::Level::Off);
        }
        if level == polish::Level::Off {
            return Err(
                "当前润色档的力度是「关闭」：在设置 → 润色 → 这一档里选 轻度/中度/重度".to_string(),
            );
        }
        if c.polish_api_key.trim().is_empty() {
            return Err("没有填 API Key".to_string());
        }
        let sample = "嗯那个我明天上午十点把设计文档发给你然后我们再对一下细节";
        let prof = self.profiles.lock().unwrap().current();
        let req = polish::Request {
            endpoint: c.polish_endpoint.clone(),
            model: c.polish_model.clone(),
            api_key: c.polish_api_key.clone(),
            level,
            timeout_ms: c.polish_timeout_ms,
            app: String::new(),
            hotwords: Vec::new(),
            extra_prompt: prof.prompt.clone(),
        };
        polish::run(&req, sample).map_err(|e| format!("润色失败：{e}"))
    }

    /// 选区指令改写（同步网络调用，壳在后台队列里调）：返回改写后的文字或中文错误
    pub fn run_command(&self, instruction: &str, text: &str) -> Result<String, String> {
        let c = self.config_snapshot();
        polish::run_command(
            &c.polish_endpoint,
            &c.polish_model,
            &c.polish_api_key,
            c.polish_timeout_ms.max(15_000),
            instruction,
            text,
        )
    }

    /// 保存热词表（解析 → 落盘 → 换内存副本 → 广播 hotwords_changed）
    pub fn set_hotwords_json(&self, text: &str) -> Result<(), String> {
        let dir = self.config_snapshot().dir;
        let path = dir.join(hotwords::FILE_NAME);
        let hw = hotwords::Hotwords::from_json(text)?.with_path(path);
        hw.save()?;
        let count = hw.len();
        *self.hotwords.lock().unwrap() = hw;
        let _ = self.send(Cmd::HotwordsChanged(count));
        Ok(())
    }

    pub fn store_json_history(&self, limit: i32) -> String {
        self.store
            .lock()
            .unwrap()
            .history_json(limit.max(0) as usize)
    }

    pub fn store_json_stats(&self) -> String {
        self.store.lock().unwrap().stats_json()
    }

    /// 非 worker 线程发事件（历史删除/清空这类只改存储通知）：seq/session 留 0
    fn emit_raw(&self, kind: &str, payload_json: &str) {
        let cb = {
            let slot = self.cb.lock().unwrap();
            slot.get()
        };
        let Some((cb, ctx)) = cb else { return };
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).unwrap_or_else(|_| serde_json::json!({}));
        let json = events::envelope(kind, 0, 0, 0, payload);
        if let Ok(c) = std::ffi::CString::new(json) {
            cb(c.as_ptr(), ctx);
        }
    }

    pub fn store_delete(&self, id: &str) -> i32 {
        let mut s = self.store.lock().unwrap();
        if s.delete(id) {
            let json = s.stats_json();
            drop(s);
            self.emit_raw("history_changed", &json);
            0
        } else {
            -1
        }
    }

    pub fn store_clear(&self) -> i32 {
        let mut s = self.store.lock().unwrap();
        s.clear();
        let json = s.stats_json();
        drop(s);
        self.emit_raw("history_changed", &json);
        0
    }

    pub fn models_list_json(&self) -> String {
        let c = self.config_snapshot();
        let sense_dir = &c.model_dir;
        let qwen_dir = &c.qwen3_dir;
        let items = serde_json::json!([
            {
                "id": "sense",
                "engine": true,
                "name": "SenseVoice Small（默认·快）",
                "installed": sense_dir.join("model.int8.onnx").exists(),
                "active": c.engine == "sense",
                "size_bytes": dir_size(sense_dir),
                "approx_mb": 233,
                "note": "中英日韩粤；峰值内存 ~600MB；11s 音频 ~200ms"
            },
            {
                "id": "qwen3",
                "engine": true,
                "name": "Qwen3-ASR 0.6B（精度档·慢）",
                "installed": qwen_dir.join("encoder.int8.onnx").exists(),
                "active": c.engine == "qwen3",
                "size_bytes": dir_size(qwen_dir),
                "approx_mb": 879,
                "note": "中英混合短句更准；峰值内存 ~1.6GB；单段上限 30s；11s 音频 ~2.4s"
            },
            {
                "id": "punct",
                "name": "标点模型（句末补逗号/句号）",
                "installed": c.dir.join("models").join("sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8/model.int8.onnx").exists(),
                "active": c.punct,
                "engine": false,
                "size_bytes": dir_size(&c.dir.join("models").join("sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8")),
                "approx_mb": 65,
                "note": "装了才有真标点；没装只在句末补句号"
            },
            {
                "id": "gtcrn",
                "name": "降噪模型（背景降噪用）",
                "installed": c.dir.join("models").join("gtcrn_simple.onnx").exists(),
                "active": c.denoise,
                "engine": false,
                "size_bytes": 0,
                "approx_mb": 1,
                "note": "1MB；开了背景降噪才需要"
            },
            {
                "id": "speaker",
                "name": "声纹模型（只录我的声音）",
                "installed": c.dir.join("models").join("3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx").exists(),
                "active": c.voice_lock,
                "engine": false,
                "size_bytes": 0,
                "approx_mb": 39,
                "note": "录过声纹才需要"
            },
            {
                "id": "stub",
                "name": "Stub（自测用，不识别）",
                "engine": true,
                "installed": true,
                "active": c.engine == "stub",
                "size_bytes": 0,
                "approx_mb": 0,
                "note": "只验证链路，不接模型"
            }
        ]);
        items.to_string()
    }

    /// 下载并安装一个模型（DESIGN §9.2）：后台线程 + `model_progress` / `model_done` / `model_failed` 事件。
    /// 返回 0 = 已开始；-1 参数错；-3 未知 id；-4 已有下载在跑；-5 已安装。
    pub fn models_pull(self: &Arc<Self>, id: &str) -> i32 {
        let Some(entry) = models::find(id) else {
            return -3;
        };
        let cfg = self.config_snapshot();
        if models::installed(&cfg.dir.join("models"), entry) {
            return -5;
        }
        {
            let mut slot = self.pulling.lock().unwrap();
            if slot.is_some() {
                return -4;
            }
            *slot = Some(entry.id.to_string());
        }
        let core = self.clone();
        let models_dir = cfg.dir.join("models");
        let id_owned = entry.id.to_string();
        std::thread::Builder::new()
            .name("eryu-model-pull".into())
            .spawn(move || {
                let closing = core.closing.clone();
                let emit = |body: &str| core.emit_raw("model_progress", body);
                core.emit_raw(
                    "model_progress",
                    &serde_json::json!({ "id": id_owned, "phase": "starting", "received": 0, "total": 0 }).to_string(),
                );
                let result = models::fetch(
                    &models_dir,
                    entry,
                    &cfg.model_mirror,
                    &cfg.model_proxy,
                    |received, total| {
                        emit(&serde_json::json!({
                            "id": id_owned, "phase": "downloading",
                            "received": received, "total": total,
                        }).to_string());
                    },
                    || closing.load(Ordering::SeqCst),
                );
                match result {
                    Ok(path) => {
                        // 下载完必须真能加载才算成功：断点续传/代理缓存都出现过「体积对、内容坏」，
                        // 直接报 done 会让人以为装好了，之后每次录音都失败（I-103）。
                        if let Err(e) = validate_installed_model(&models_dir, entry) {
                            let _ = std::fs::remove_file(&path);
                            let _ = std::fs::remove_dir_all(models_dir.join(entry.marker).parent().unwrap_or(&models_dir));
                            core.emit_raw(
                                "model_failed",
                                &serde_json::json!({
                                    "id": id_owned,
                                    "message": format!("下载的文件打不开（可能是网络断了或代理坏了），已删除，请重试：{e}"),
                                })
                                .to_string(),
                            );
                            *core.pulling.lock().unwrap() = None;
                            return;
                        }
                        core.emit_raw(
                            "model_done",
                            &serde_json::json!({ "id": id_owned, "path": path.display().to_string() }).to_string(),
                        );
                    }
                    Err(e) => {
                        core.emit_raw(
                            "model_failed",
                            &serde_json::json!({ "id": id_owned, "message": e }).to_string(),
                        );
                    }
                }
                *core.pulling.lock().unwrap() = None;
            })
            .ok();
        0
    }

    pub fn devices_json(&self) -> String {
        let cfg = self.config_snapshot();
        let default_name = audio::default_input_name();
        let items: Vec<serde_json::Value> = audio::list_input_devices()
            .into_iter()
            .map(|name| {
                serde_json::json!({
                    "name": name,
                    "default": default_name.as_deref() == Some(name.as_str()),
                })
            })
            .collect();
        serde_json::json!({
            "devices": items,
            "selected": cfg.device,
            "default": default_name,
        })
        .to_string()
    }

    /// 注册声纹（从一段 16k WAV 里提取 embedding）。设置页「录 10 秒声纹」最终也走这里。
    pub fn voice_enroll_wav(&self, path: &str) -> Result<String, String> {
        let c = self.config_snapshot();
        let pcm = audio::read_wav16(std::path::Path::new(path))?;
        if pcm.len() < audio::TARGET_RATE as usize {
            return Err("样本太短了：至少说 1 秒（建议 8–10 秒）".to_string());
        }
        let model = voicelock::model_path(&c.dir);
        if !model.exists() {
            return Err(format!("声纹模型还没下载：{}", model.display()));
        }
        let Some(vec) = voicelock::embed(&model, &pcm) else {
            return Err("这段音频算不出声纹（可能太安静或全是噪声）".to_string());
        };
        let print = voicelock::VoicePrint {
            model: voicelock::MODEL_FILE.to_string(),
            dim: vec.len(),
            created: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            vec,
        };
        voicelock::save(&c.dir, &print)?;
        // 让下次录音重新加载声纹
        *self.voice_gate.lock().unwrap() = None;
        Ok(format!(
            "已注册声纹（{} 维，来自 {} 秒音频）",
            print.dim,
            pcm.len() / audio::TARGET_RATE as usize
        ))
    }

    /// 录一段麦克风音频当声纹（同步阻塞，界面上要自己显示倒计时）
    pub fn voice_enroll_mic(&self, secs: u32) -> Result<String, String> {
        let c = self.config_snapshot();
        let model = voicelock::model_path(&c.dir);
        if !model.exists() {
            return Err(format!("声纹模型还没下载：{}", model.display()));
        }
        let secs = secs.clamp(3, 20);
        let cap = audio::start_with(None)?;
        let mut all: Vec<f32> = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs as u64);
        while std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
            cap.drain(&mut all);
        }
        cap.finish(&mut all);
        audio::cap_len_to(&mut all, 20);
        if (all.len() as f32 / audio::TARGET_RATE as f32) < 1.0 {
            return Err("没录到声音：检查麦克风权限和设备".to_string());
        }
        let Some(vec) = voicelock::embed(&model, &all) else {
            return Err("这段音频算不出声纹（可能太安静或全是噪声）".to_string());
        };
        let print = voicelock::VoicePrint {
            model: voicelock::MODEL_FILE.to_string(),
            dim: vec.len(),
            created: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            vec,
        };
        voicelock::save(&c.dir, &print)?;
        *self.voice_gate.lock().unwrap() = None;
        Ok(format!("已注册声纹（{} 维，录了 {} 秒）", print.dim, secs))
    }

    pub fn voice_clear(&self) -> Result<(), String> {
        let c = self.config_snapshot();
        voicelock::clear(&c.dir)?;
        *self.voice_gate.lock().unwrap() = None;
        Ok(())
    }

    pub fn voice_state_json(&self) -> String {
        let c = self.config_snapshot();
        let print = voicelock::load(&c.dir);
        serde_json::json!({
            "enrolled": print.is_some(),
            "dim": print.as_ref().map(|p| p.dim).unwrap_or(0),
            "created": print.as_ref().map(|p| p.created.clone()).unwrap_or_default(),
            "model_ready": voicelock::model_path(&c.dir).exists(),
            "model_path": voicelock::model_path(&c.dir).to_string_lossy(),
            "enabled": c.voice_lock,
            "threshold": c.voice_lock_threshold,
        })
        .to_string()
    }

    pub fn diagnose(&self) -> String {
        let c = self.config_snapshot();
        let (count, enabled) = {
            let s = self.store.lock().unwrap();
            (s.count(), s.enabled)
        };
        let mut out = String::new();
        out.push_str(&format!(
            "Eryu core {} (ABI {}.{})\n",
            env!("CARGO_PKG_VERSION"),
            ABI_MAJOR,
            ABI_MINOR
        ));
        out.push_str(&format!("配置目录：{}\n", c.dir.display()));
        out.push_str(&format!(
            "引擎：{}（SenseVoice 就绪={} / Qwen3 就绪={}）\n",
            c.engine,
            c.models_ready(),
            c.qwen3_dir.join("encoder.int8.onnx").exists()
        ));
        out.push_str(&format!(
            "VAD：{} ({})\n",
            c.vad_model.display(),
            if c.vad_ready() { "有" } else { "缺失" }
        ));
        let devs = audio::list_input_devices();
        out.push_str(&format!(
            "输入设备：{}{}\n",
            if devs.is_empty() {
                "（无）".to_string()
            } else {
                devs.join(" / ")
            },
            match audio::default_input_name() {
                Some(d) => format!("（默认：{d}）"),
                None => "（无默认设备）".to_string(),
            }
        ));
        if !c.device.trim().is_empty() {
            out.push_str(&format!("已指定设备：{}\n", c.device));
        }
        out.push_str(&format!(
            "历史：{} 条（{}）\n",
            count,
            if enabled { "开启" } else { "已关闭" }
        ));
        {
            let pr = self.profiles.lock().unwrap();
            let cur = pr.current();
            out.push_str(&format!(
                "润色档：{}（{}，{}）共 {} 档\n",
                cur.name,
                cur.level,
                if cur.slot.is_empty() {
                    "未绑快捷键".to_string()
                } else {
                    cur.slot.clone()
                },
                pr.list().len()
            ));
        }
        out.push_str(&format!(
            "热词：{} 条（{}）\n",
            self.hotwords.lock().unwrap().len(),
            c.dir.join(crate::hotwords::FILE_NAME).display()
        ));
        out.push_str(&format!(
            "最短录音：{}ms；会话上限：{}s\n",
            c.min_record_ms, c.max_record_secs
        ));
        out.push_str(&format!(
            "上屏方式：{}（按应用覆盖 {} 条）；隐私模式：{}；降噪：{}；暂停播放：{}\n",
            c.insert_mode,
            c.insert_per_app.len(),
            if c.privacy_mode { "开" } else { "关" },
            if c.denoise {
                if crate::denoise::gtcrn_path(&c.dir).exists() {
                    "开（GTCRN）"
                } else {
                    "开（但模型没装，实际不降噪）"
                }
            } else {
                "关"
            },
            if c.pause_media { "开" } else { "关" }
        ));
        out.push_str(&format!("快捷键：{}\n", c.hotkey));
        {
            let print = voicelock::load(&c.dir);
            out.push_str(&format!(
                "只录我的声音：{}（{}；阈值 {:.2}）\n",
                if c.voice_lock { "开" } else { "关" },
                match &print {
                    Some(p) => format!("已注册 {} 维，{}", p.dim, p.created),
                    None => "未注册声纹".to_string(),
                },
                c.voice_lock_threshold
            ));
        }
        {
            let path = punct::model_path(&c.dir);
            out.push_str(&format!(
                "标点模型：{}{}\n",
                if c.punct { "开" } else { "关" },
                if path.exists() {
                    format!("（已装 {}，补逗号/句号）", path.display())
                } else {
                    "（未装，只在句末补「。」）".to_string()
                }
            ));
        }
        if let Some(mb) = resident_mb() {
            out.push_str(&format!("常驻内存：{mb:.0} MB\n"));
        }
        {
            let s = self.store.lock().unwrap();
            let lat = store::latency_line(&s.stats.latencies);
            if !lat.is_empty() {
                out.push_str(&format!("最近延迟：{lat}\n"));
            }
        }
        if let Some(w) = &c.load_warning {
            out.push_str(&format!("上次加载提示：{w}\n"));
        }
        out
    }
}

impl Drop for EryuCore {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Cmd::Quit);
        if let Some(h) = self.worker.lock().unwrap().take() {
            let _ = h.join();
        }
        // 单段重试线程可能还在跑（它会再进引擎）：等它退出，最多 5s。
        // 不等的话，进程/动态库卸载后它继续用引擎 → "Rust cannot catch foreign exceptions, aborting"。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while self.retry_busy.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        self.cb.lock().unwrap().clear();
    }
}

/// 当前进程常驻内存（MB）。core 与壳同进程，量的是同一个数字。
#[cfg(target_os = "macos")]
fn resident_mb() -> Option<f64> {
    #[repr(C)]
    struct MachTaskBasicInfo {
        virtual_size: u64,
        resident_size: u64,
        resident_size_max: u64,
        user_time: [u32; 2],
        system_time: [u32; 2],
        policy: i32,
        suspend_count: i32,
    }
    extern "C" {
        fn task_info(
            target_task: u32,
            flavor: u32,
            task_info_out: *mut i32,
            task_info_out_cnt: *mut u32,
        ) -> i32;
        fn mach_task_self() -> u32;
    }
    const MACH_TASK_BASIC_INFO: u32 = 20;
    let mut info = MachTaskBasicInfo {
        virtual_size: 0,
        resident_size: 0,
        resident_size_max: 0,
        user_time: [0; 2],
        system_time: [0; 2],
        policy: 0,
        suspend_count: 0,
    };
    let mut count = (std::mem::size_of::<MachTaskBasicInfo>() / std::mem::size_of::<u32>()) as u32;
    let kr = unsafe {
        task_info(
            mach_task_self(),
            MACH_TASK_BASIC_INFO,
            &mut info as *mut MachTaskBasicInfo as *mut i32,
            &mut count,
        )
    };
    if kr == 0 {
        Some(info.resident_size as f64 / 1024.0 / 1024.0)
    } else {
        None
    }
}

#[cfg(not(target_os = "macos"))]
fn resident_mb() -> Option<f64> {
    None
}

fn dir_size(p: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            if let Ok(md) = e.metadata() {
                total += if md.is_dir() {
                    dir_size(&e.path())
                } else {
                    md.len()
                };
            }
        }
    }
    total
}

// ============================== worker ==============================

struct Worker {
    cfg: Arc<Mutex<config::Config>>,
    cfg_gen: Arc<std::sync::atomic::AtomicU64>,
    cfg_applied: Arc<std::sync::atomic::AtomicU64>,
    cfg_gen_seen: u64,
    store: Arc<Mutex<store::Store>>,
    hotwords: Arc<Mutex<hotwords::Hotwords>>,
    profiles: Arc<Mutex<profiles::Profiles>>,
    engine: Arc<Mutex<Box<dyn asr::AsrEngine>>>,
    /// true 表示引擎加载失败、暂用 stub 顶着（此时不允许上屏假文本）
    engine_fallback: Arc<AtomicBool>,
    rx: Receiver<Cmd>,
    tx: Sender<Cmd>,
    m: session::Machine,
    cb: Arc<Mutex<CbSlot>>,
    last: Arc<Mutex<Option<String>>>,
    seq: u64,
    capture: Option<audio::Capture>,
    recorded: Vec<f32>,
    recorded_at: Option<Instant>,
    decoding: bool,
    debug_stop_at: Option<Instant>,
    foreground_app: String,
    /// 录音中的准流式预览（DESIGN §4.6）：定期把已录内容再解一遍，只显示不上屏
    partial_busy: bool,
    /// 预览线程是否还活着（线程退出时清）。比 partial_busy 严格：保证任何时刻最多一个预览线程
    partial_alive: Arc<AtomicBool>,
    /// 正在跑的那块是「提交块」（会推进 prog_pos）还是「预览块」（只显示）
    partial_commit: bool,
    /// 在飞的那块音频截止到哪个样本（松手时判断它是否已经覆盖了整段尾巴）
    partial_up_to: usize,
    partial_due: Option<Instant>,
    partial_seen: usize,
    partial_last: String,
    partial_last_ms: u64,
    /// 渐进解码已提交的样本数（松手后只解这之后的尾巴）
    prog_pos: usize,
    /// 测试钩子：到点就模拟一次「麦克风中断」（ERYU_FORCE_MIC_FAIL_MS）
    forced_mic_fail_at: Option<std::time::Instant>,
    /// 松手时正在跑的那一块：等它把结果送回来直接提交（最多 COMMIT_WAIT_MS），别白算一遍
    waiting_commit: Option<String>,
    commit_deadline: Option<Instant>,
    /// 渐进解码已提交的文本（松手后与尾巴拼起来再热词/标点）
    prog_text: String,
    /// 上次结果里识别失败的段（含音频），供「只重试这一段」
    failed_segments: Vec<(usize, Vec<f32>)>,
    /// 与 EryuCore 共享的失败段序号（UI 菜单据此决定是否可点）
    failed_indexes: Arc<Mutex<Vec<usize>>>,
    /// 与 EryuCore 共享的单段重试活跃标记
    retry_busy: Arc<AtomicBool>,
    /// 与 EryuCore 共享的声纹门控（懒加载）
    voice_gate: Arc<Mutex<Option<voicelock::Gate>>>,
    /// 与 EryuCore 共享的自测模式开关
    test_mode: Arc<AtomicBool>,
    /// 自测用「假麦克风」：环境变量 ERYU_MIC_WAV 指定的 16k WAV，按真实时间喂给录音缓冲
    sim_pcm: Option<std::sync::Arc<Vec<f32>>>,
    sim_pos: usize,
    sim_last: Instant,
    decode_started: Option<Instant>,
    /// 松手 → 真正开始解码之间的等待（等渐进块提交 / 等引擎锁），用于把延迟拆开
    wait_started: Option<Instant>,
    commit_wait_ms: u64,
    engine_wait_ms: u64,
    partial_cancel: Arc<AtomicBool>,
    meter: Option<audio::Capture>,
    meter_scratch: Vec<f32>,
    meter_last: Instant,
    /// 录音期给浮层用的电平节流（与设置页电平测试分开）
    rec_meter_last: Instant,
    last_failed: Option<Vec<f32>>,
    pending_history_id: Option<String>,
    /// 最近一次采音的设备名（静音/无声音时用来指名麦克风）
    last_device: Option<String>,
    /// 当前生效的局域网接口配置（开关/端口/token/allow_lan）：设置一改就重启监听
    lan_cfg: Option<(bool, u16, String, bool)>,
    closing: Arc<AtomicBool>,
}

impl Worker {
    fn privacy(&self) -> bool {
        self.cfg().privacy_mode
    }

    fn cfg(&self) -> config::Config {
        self.cfg.lock().unwrap().clone()
    }

    fn run(mut self) {
        self.cleanup_failed_clips();
        self.recover_pending_result();
        self.start_lan_if_enabled();
        loop {
            let hint = self.wait_hint();
            match self.rx.recv_timeout(hint) {
                Ok(cmd) => {
                    let outcome = catch_unwind(AssertUnwindSafe(|| self.handle(cmd)));
                    match outcome {
                        Ok(true) => break,
                        Ok(false) => {}
                        Err(_) => {
                            self.on_panic();
                            break;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            let outcome = catch_unwind(AssertUnwindSafe(|| self.tick()));
            if outcome.is_err() {
                self.on_panic();
                break;
            }
            if self.closing.load(Ordering::SeqCst) {
                break;
            }
        }
        self.emit_idle_if_needed();
    }

    fn wait_hint(&self) -> Duration {
        let tick = Duration::from_millis(TICK_MS);
        match self.debug_stop_at {
            Some(t) => {
                let now = Instant::now();
                if t > now {
                    tick.min(t - now)
                } else {
                    Duration::from_millis(1)
                }
            }
            None => tick,
        }
    }

    fn on_panic(&mut self) {
        self.emit_error(
            "worker_panic",
            "核心内部错误，请重启 Eryu（详情见控制台日志）",
        );
        self.closing.store(true, Ordering::SeqCst);
    }

    /// 崩溃/被强杀时可能停在「结果已出、上屏未确认」之间：
    /// 启动时把暂存的文本读回来，用户还能从菜单取回（DESIGN §5 M1c「崩溃 pending_result 落盘」）。
    fn recover_pending_result(&mut self) {
        let path = self.cfg().pending_dir().join("last_result.txt");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            let _ = std::fs::remove_file(&path);
            return;
        }
        *self.last.lock().unwrap() = Some(text.clone());
        self.emit(
            "pending_result",
            serde_json::json!({ "text": text, "chars": text.chars().count() }),
        );
    }

    /// 局域网听写接口（M5 路线②）：开了就在后台监听，别的设备把 WAV POST 过来换文字
    fn start_lan_if_enabled(&mut self) {
        let (enabled, port, token, allow_lan, vad) = {
            let c = self.cfg.lock().unwrap();
            (
                c.lan_enabled,
                c.lan_port,
                c.lan_token.clone(),
                c.lan_allow_lan,
                c.vad_model.clone(),
            )
        };
        self.lan_cfg = Some((enabled, port, token.clone(), allow_lan));
        if !enabled {
            return;
        }
        // 监听 0.0.0.0 却没设 token = 同一网段谁都能用你的模型、还能借此判断你在不在线
        if allow_lan && token.trim().is_empty() {
            let msg = "局域网接口开了「允许其它设备访问」，但没设 token：为了安全没有启动。请在设置 → 音频里设一个 token（或关掉「允许局域网」只给本机用）。";
            eprintln!("[eryu][lan] 拒绝启动：{msg}");
            self.emit_error("lan_no_token", msg);
            return;
        }
        let opts = crate::lan::Options {
            port,
            token,
            allow_lan,
        };
        match crate::lan::spawn(self.engine.clone(), vad, opts) {
            Ok(actual) => {
                let host = if allow_lan { "0.0.0.0" } else { "127.0.0.1" };
                eprintln!("[eryu][lan] 监听 http://{host}:{actual}/transcribe");
                self.emit_notice(
                    "lan_listening",
                    &format!("局域网听写接口已开：http://{host}:{actual}（POST /transcribe，不上屏、不写历史）"),
                );
            }
            Err(e) => self.emit_error("lan_failed", &format!("局域网听写接口起不来：{e}")),
        }
    }

    fn cleanup_failed_clips(&self) {
        let dir = self.cfg().pending_dir();
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return;
        };
        let now = now_ms();
        for e in rd.flatten() {
            let Ok(md) = e.metadata() else { continue };
            let age = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| now.saturating_sub(d.as_millis() as u64) / 1000)
                .unwrap_or(0);
            if age > FAILED_CLIP_TTL_SECS {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    // ---------- 事件 ----------

    fn emit(&mut self, kind: &str, payload: serde_json::Value) {
        self.seq += 1;
        let json = events::envelope(kind, self.seq, self.m.session, self.m.generation, payload);
        let cb = {
            let slot = self.cb.lock().unwrap();
            slot.get()
        };
        if let Some((cb, ctx)) = cb {
            if let Ok(c) = std::ffi::CString::new(json) {
                cb(c.as_ptr(), ctx);
            }
        }
    }

    fn emit_state(&mut self) {
        let phase = self.m.phase.as_str();
        self.emit("state", events::state_payload(phase));
    }

    fn emit_error(&mut self, code: &str, message: &str) {
        self.emit("error", events::error_payload(code, message));
    }

    /// 提示（不打断录音、不算错误）：比如「到上限了」「略过了 N 段不像你的声音」
    fn emit_notice(&mut self, code: &str, message: &str) {
        self.emit(
            "notice",
            serde_json::json!({ "code": code, "message": message }),
        );
    }

    fn emit_stats(&mut self) {
        let json = self.store.lock().unwrap().stats_json();
        let payload: serde_json::Value =
            serde_json::from_str(&json).unwrap_or_else(|_| serde_json::json!({}));
        self.emit("stats", payload);
    }

    fn emit_idle_if_needed(&mut self) {
        if self.m.phase != session::Phase::Idle {
            let _ = self.m.cancel();
        }
    }

    // ---------- 命令 ----------

    fn handle(&mut self, cmd: Cmd) -> bool {
        match cmd {
            Cmd::Down => {
                self.stop_meter();
                self.start_session(None);
            }
            Cmd::Up => match self.m.up() {
                session::Step::Finish { task_id } => self.begin_decode(task_id, None),
                _ => {}
            },
            Cmd::Cancel => {
                let step = self.m.cancel();
                if let session::Step::RejectedCancel = step {
                    self.emit_error("cancel_rejected", "文字已在粘贴中，无法取消");
                } else {
                    self.capture = None;
                    self.recorded.clear();
                    self.recorded_at = None;
                    self.debug_stop_at = None;
                    self.emit_state();
                }
            }
            Cmd::Submit {
                task_id,
                status,
                detail,
            } => self.on_submit(&task_id, &status, &detail),
            Cmd::Decoded {
                task_id,
                result,
                speech_ms,
                decode_ms,
                pcm,
                raw_text,
                polish_note,
            } => self.on_decoded(
                &task_id,
                result,
                speech_ms,
                decode_ms,
                pcm,
                raw_text,
                polish_note,
            ),
            Cmd::DebugText(text) => {
                self.m.prepared_text = Some(text);
                self.start_session(Some(Duration::from_millis(40)));
            }
            Cmd::DecodePcm(pcm) => {
                if self.m.phase != session::Phase::Idle {
                    self.emit_error("busy", "正在处理上一次录音，稍后再试");
                } else {
                    let _ = self.m.down();
                    self.m.ready_at = Some(Instant::now());
                    let task_id = match self.m.up() {
                        session::Step::Finish { task_id } => task_id,
                        _ => {
                            if let session::Step::Finish { task_id } = self.m.begin_processing() {
                                task_id
                            } else {
                                String::new()
                            }
                        }
                    };
                    if !task_id.is_empty() {
                        self.begin_decode(task_id, Some(pcm));
                    } else {
                        self.emit_error("decode_busy", "排不进解码队列");
                    }
                }
            }
            Cmd::DebugRecord(secs) => {
                let secs = secs.clamp(0.5, 10.0);
                self.start_session(Some(Duration::from_secs_f32(secs)));
            }
            Cmd::Partial {
                text,
                ms,
                up_to,
                commit,
            } => self.on_partial(text, ms, up_to, commit),
            Cmd::RetrySegment(index) => self.retry_segment(index),
            Cmd::SegmentDone { index, result } => self.on_segment_done(index, result),
            Cmd::RetryFailed => self.retry_failed(),
            Cmd::MeterStart => self.start_meter(),
            Cmd::MeterStop => self.stop_meter(),
            Cmd::ApplyConfig(cfg, gen) => self.apply_config(*cfg, gen),
            Cmd::SetEngine(id) => self.set_engine(&id),
            Cmd::HotwordsChanged(n) => {
                self.emit("hotwords_changed", serde_json::json!({ "count": n }));
            }
            Cmd::NoteApp(name) => self.foreground_app = name,
            Cmd::EngineBuilt { engine_id, result } => match result {
                Ok(e) => {
                    *self.engine.lock().unwrap() = e;
                    // 真正装上了一个能用的引擎（包括用户主动选 stub），不再是「加载失败回退」
                    self.engine_fallback.store(false, Ordering::SeqCst);
                    self.emit_error(
                        "engine_ready",
                        &format!("已切换到 {engine_id}，下次录音生效"),
                    );
                }
                Err(e) => {
                    let msg = format!("切换引擎失败：{e}");
                    self.emit_error("engine_switch_failed", &msg);
                }
            },
            Cmd::PanicTest => panic!("debug panic"),
            Cmd::Barrier(ack) => {
                let _ = ack.send(());
            }
            Cmd::Quit => return true,
        }
        false
    }

    // ---------- 会话 ----------

    fn reset_partial(&mut self) {
        self.partial_busy = false;
        self.partial_commit = false;
        self.partial_due = None;
        self.partial_seen = 0;
        self.partial_last.clear();
        self.partial_last_ms = 0;
        self.prog_pos = 0;
        self.prog_text.clear();
    }

    fn start_session(&mut self, auto_stop: Option<Duration>) {
        self.debug_stop_at = auto_stop.map(|d| Instant::now() + d);
        self.forced_mic_fail_at = std::env::var("ERYU_FORCE_MIC_FAIL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(|ms| Instant::now() + Duration::from_millis(ms));
        self.reset_partial();
        self.failed_segments.clear();
        match self.m.down() {
            session::Step::SessionStarted => {}
            _ => return,
        }
        self.emit_state();
        self.recorded.clear();
        self.sim_pcm = std::env::var("ERYU_MIC_WAV")
            .ok()
            .and_then(|p| audio::read_wav16(std::path::Path::new(&p)).ok())
            // 假麦克风也走真实采集链的降噪（否则模拟测不到降噪这一段）
            .map(|pcm| {
                let gtcrn = crate::denoise::gtcrn_path(&self.cfg().dir);
                if self.cfg().denoise && gtcrn.exists() {
                    if let Some(d) = crate::denoise::Gtcrn::load(&gtcrn) {
                        return d.run(&pcm);
                    }
                }
                pcm
            })
            .map(std::sync::Arc::new);
        self.sim_pos = 0;
        self.sim_last = Instant::now();
        if self.sim_pcm.is_some() {
            eprintln!(
                "[eryu][sim] 用 WAV 代替麦克风（{} 采样）",
                self.sim_pcm.as_ref().map(|v| v.len()).unwrap_or(0)
            );
            self.last_device = Some("模拟麦克风（ERYU_MIC_WAV）".to_string());
            self.recorded_at = Some(Instant::now());
        } else if self.m.prepared_text.is_none() {
            // 模型没装就别把路径交给 sherpa（它会在每条会话里打两行 Load model 失败），
            // 静默退回不降噪；设置页/诊断里仍会显示「开了但模型没装」。
            let dn = if self.cfg().denoise {
                Some(crate::denoise::gtcrn_path(&self.cfg().dir)).filter(|p| p.exists())
            } else {
                None
            };
            match audio::start_with_opts(Some(self.cfg().device.as_str()), dn) {
                Ok(c) => {
                    self.last_device = Some(c.device_name.clone());
                    eprintln!(
                        "[eryu] 采音开始 {} {}Hz {}ch",
                        c.device_name, c.input_rate, c.channels
                    );
                    self.recorded_at = Some(Instant::now());
                    self.capture = Some(c);
                }
                Err(e) => {
                    self.m.abort_processing();
                    self.recorded_at = None;
                    self.emit_error("mic_unavailable", &e);
                    self.emit_state();
                }
            }
        } else {
            self.recorded_at = Some(Instant::now());
        }
    }

    fn tick(&mut self) {
        if let Some(dl) = self.commit_deadline {
            if Instant::now() >= dl {
                // 等不到就回退到老路：取消预览，自己从头解尾巴
                let task = self.waiting_commit.take().unwrap_or_default();
                self.commit_deadline = None;
                if !task.is_empty() {
                    self.start_decode(task, None);
                }
            }
        }
        let now = Instant::now();
        // 排空采音
        if let Some(c) = &self.capture {
            c.drain(&mut self.recorded);
        }
        // 假麦克风：按真实时间把 WAV 喂进去（自测「按住 → 流式 → 松手」全链路，不碰真麦克风）
        if self.m.phase == session::Phase::Listening {
            if let Some(pcm) = self.sim_pcm.clone() {
                let elapsed = now.duration_since(self.sim_last);
                // 自测可以加速喂样本（ERYU_SIM_SPEED=8 表示 8 倍速），这样冒烟里跑 1 分钟音频不用真等 1 分钟；
                // 识别照常按真实算力跑，量到的「松手→出字」仍是真时间。
                let speed: f64 = std::env::var("ERYU_SIM_SPEED")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .filter(|v: &f64| *v > 0.1)
                    .unwrap_or(1.0);
                // 目标位置按「按下到现在」的墙上时间算（不是按上次 tick 的增量），
                // 这样某次 tick 被拖慢时下一跳会自己补上，不会像以前那样把落后量永久丢掉——
                // 那正是「长句最后一句没了」的模拟侧成因。
                let target = ((elapsed.as_secs_f64() * 16000.0 * speed) as usize).min(pcm.len());
                if target > self.sim_pos {
                    self.recorded
                        .extend_from_slice(&pcm[self.sim_pos..target]);
                    self.sim_pos = target;
                }
                if self.sim_pos >= pcm.len() && !pcm.is_empty() {
                    // 素材播完等价于用户松手
                    if let session::Step::Finish { task_id } = self.m.up() {
                        self.begin_decode(task_id, None);
                    }
                }
            }
        }
        let cap = self.cfg();
        // 录音中到上限就收工：不再悄悄丢开头（用户会以为「我明明说了」）
        if self.m.phase == session::Phase::Listening
            && audio::secs_of(&self.recorded) >= cap.max_record_secs as f32
        {
            audio::cap_len_to(&mut self.recorded, cap.max_record_secs);
            self.emit(
                "notice",
                serde_json::json!({
                    "code": "record_limit",
                    "message": format!(
                        "到 {} 秒上限了，已自动结束并开始识别（上限可在设置里调）",
                        cap.max_record_secs
                    )
                }),
            );
            if let session::Step::Finish { task_id } = self.m.up() {
                self.begin_decode(task_id, None);
            }
        }
        audio::cap_len_to(&mut self.recorded, cap.max_record_secs);

        // 设备被拔掉/被独占：保住已经录到的部分，立刻收尾（DESIGN §10.1 第 7 条）
        if self.m.phase == session::Phase::Listening {
            // 测试钩子：模拟「录到一半麦克风被拔掉」，验证已录内容被保住（scripts/mic_unplug.sh）
            let forced_now = self
                .forced_mic_fail_at
                .map(|at| std::time::Instant::now() >= at)
                .unwrap_or(false);
            if forced_now {
                self.forced_mic_fail_at = None;
                self.emit_error(
                    "mic_unplugged",
                    "麦克风中断了（模拟拔掉）：已保留中断前录到的内容",
                );
                if let session::Step::Finish { task_id } = self.m.up() {
                    self.begin_decode(task_id, None);
                }
            } else if let Some(c) = &self.capture {
                if c.has_failed() {
                    self.emit_error(
                        "mic_unplugged",
                        "麦克风中断了（可能被拔掉或被其他应用占用）：已保留中断前录到的内容",
                    );
                    if let session::Step::Finish { task_id } = self.m.up() {
                        self.begin_decode(task_id, None);
                    }
                }
            }
        }

        // 录音期电平（浮层电平条用）：只发 rms/peak，不发音频
        if self.m.phase == session::Phase::Listening
            && now.duration_since(self.rec_meter_last).as_millis() >= METER_INTERVAL_MS as u128
        {
            self.rec_meter_last = now;
            let tail: &[f32] = if self.recorded.len() > 2400 {
                &self.recorded[self.recorded.len() - 2400..]
            } else {
                &self.recorded
            };
            let peak = audio::peak(tail);
            let rms = if tail.is_empty() {
                0.0
            } else {
                (tail.iter().map(|s| (s * s) as f64).sum::<f64>() / tail.len() as f64).sqrt() as f32
            };
            if std::env::var("ERYU_DEBUG_METER").is_ok() {
                eprintln!("[eryu][meter] rms={rms:.4} peak={peak:.4}");
            }
            self.emit(
                "meter",
                serde_json::json!({
                    "rms": rms,
                    "peak": peak,
                    "silent": rms < 0.003,
                    "live": true,
                    // 已录秒数：浮层显示「在听… 12s」，长录音时用户知道还在收
                    "secs": audio::secs_of(&self.recorded),
                }),
            );
        }

        // 准流式预览：录音中每 ~900ms 把已录内容再解一遍（只显示，不上屏）
        self.tick_partial(now);

        // 电平测试
        self.tick_meter(now);

        // 调试自动停
        if let Some(t) = self.debug_stop_at {
            if now >= t {
                self.debug_stop_at = None;
                if let session::Step::Finish { task_id } = self.m.up() {
                    self.begin_decode(task_id, None);
                }
            }
        }

        let step = self.m.tick(now);
        match step {
            session::Step::Listening => {
                self.emit_state();
                if let session::Step::Finish { task_id } = self.m.take_pending_stop() {
                    self.begin_decode(task_id, None);
                }
            }
            session::Step::SubmitTimedOut { task_id } => {
                self.emit_error(
                    "submit_timeout",
                    &format!("上屏结果未知（任务 {task_id}），文字已保存，可从菜单复制"),
                );
                self.emit_state();
            }
            session::Step::DecodeStalled { task_id } => {
                self.emit_error(
                    "decode_stalled",
                    &format!("识别超时（任务 {task_id}），音频已保留，可重试"),
                );
                self.emit_state();
            }
            _ => {}
        }
    }

    fn begin_decode(&mut self, task_id: String, pcm_override: Option<Vec<f32>>) {
        // 模拟麦克风：按墙上时间喂样本，松手那一刻常常还差零点几秒到几秒没喂进去，
        // 表现出来就是「长句最后一句话没了」。这里一次补齐，真麦克风走 Capture::finish 的宽限排空。
        if let Some(pcm) = self.sim_pcm.clone() {
            // 只补到「按下到现在」该喂到的位置；不能把整个 WAV 一次倒进来，
            // 否则松手很早的用例（长听写、长跑）会凭空多出几十分钟音频。
            let speed: f64 = std::env::var("ERYU_SIM_SPEED")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|v: &f64| *v > 0.1)
                .unwrap_or(1.0);
            let elapsed = self.sim_last.elapsed().as_secs_f64();
            let target = ((elapsed * 16000.0 * speed) as usize).min(pcm.len());
            if target > self.sim_pos {
                self.recorded
                    .extend_from_slice(&pcm[self.sim_pos..target]);
                self.sim_pos = target;
            }
        }
        self.wait_started = Some(Instant::now());
        // 松手时若有一块正在算：等它（最多 COMMIT_WAIT_MS）把它提交掉，尾巴就只剩一点；
        // 直接取消再重算会把这块音频再解一遍，实测让「松手→出字」多 0.6–1.2s。
        let covers_tail =
            self.partial_up_to + audio::TARGET_RATE as usize / 2 >= self.recorded.len();
        if self.partial_busy
            && (self.partial_commit || covers_tail)
            && pcm_override.is_none()
            && self.m.prepared_text.is_none()
        {
            self.waiting_commit = Some(task_id);
            self.commit_deadline = Some(Instant::now() + Duration::from_millis(COMMIT_WAIT_MS));
            return;
        }
        self.start_decode(task_id, pcm_override);
    }

    fn start_decode(&mut self, task_id: String, pcm_override: Option<Vec<f32>>) {
        self.waiting_commit = None;
        self.commit_deadline = None;
        self.commit_wait_ms = self
            .wait_started
            .take()
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);
        self.decode_started = Some(Instant::now());
        // 准流式预览和最终识别共用一个引擎（Arc<Mutex>）：松手时若上一轮预览还在跑，
        // 等它结束再开最终识别，否则最终识别会先卡在锁上（实测多花 ~700ms）。
        self.partial_cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let mut waited = 0u64;
        while self.partial_busy && waited < 200 {
            thread::sleep(Duration::from_millis(2));
            waited += 2;
        }
        self.engine_wait_ms = waited;
        if self.decoding {
            self.emit_error("busy", "上一段还在识别，请稍后再试");
            return;
        }
        let prepared = self.m.prepared_text.take();
        if let Some(text) = prepared {
            if let Some(c) = &self.capture {
                let _ = &c.device_name;
            }
            self.capture = None;
            let result = pipeline::PipelineResult {
                text: text.clone(),
                segments: vec![pipeline::SegmentInfo {
                    index: 0,
                    samples: 0,
                    text,
                    error: None,
                    audio: None,
                }],
                failed: 0,
                skipped: 0,
            };
            let speech_ms = self
                .recorded_at
                .take()
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);
            let raw = result.text.trim().to_string();
            let _ = self.tx.send(Cmd::Decoded {
                task_id,
                result: Ok(result),
                speech_ms,
                decode_ms: 0,
                pcm: Vec::new(),
                raw_text: raw,
                polish_note: None,
            });
            return;
        }

        let mut pcm = match pcm_override {
            Some(p) => p,
            None => {
                let mut out = std::mem::take(&mut self.recorded);
                if let Some(c) = self.capture.take() {
                    c.finish(&mut out);
                }
                if std::env::var("ERYU_TAIL_DEBUG").is_ok() {
                    eprintln!(
                        "[eryu][tail] 松手时音频 {:.3}s（= {} 采样 / 16000），录音时长 {:.3}s",
                        out.len() as f64 / 16000.0,
                        out.len(),
                        self.recorded_at.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0)
                    );
                }
                out
            }
        };
        audio::cap_len_to(&mut pcm, self.cfg().max_record_secs);
        if std::env::var("ERYU_TAIL_DEBUG").is_ok() {
            eprintln!(
                "[eryu][tail] cap 之后 {:.3}s（上限 {}s）",
                pcm.len() as f64 / 16000.0,
                self.cfg().max_record_secs
            );
        }
        let speech_ms = self
            .recorded_at
            .take()
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or((pcm.len() as u64 * 1000) / audio::TARGET_RATE as u64);

        let min_ms = self.cfg().min_record_ms;
        let peak = audio::peak(&pcm);
        if std::env::var("ERYU_DEBUG_GAIN").is_ok() {
            eprintln!(
                "[eryu][gain] peak={peak:.4} speech={}",
                audio::looks_like_speech(&pcm)
            );
        }
        // 太轻的一律不当说话：否则自动增益会把底噪放大，让模型「听出」一句不存在的话（D-072）
        // 分两档说清楚：完全没声音 vs 有声音但太轻（后者常是扬声器回声/耳机漏音，插进去只会变成乱码）
        if pcm.is_empty() || !audio::looks_like_speech(&pcm) || peak < audio::SPEECH_MIN_PEAK {
            self.m.abort_processing();
            let too_quiet =
                !pcm.is_empty() && audio::looks_like_speech(&pcm) && peak < audio::SPEECH_MIN_PEAK;
            let (code, mut msg) = if too_quiet {
                ("too_quiet", format!(
                    "声音太小了（峰值 {:.0}%）：靠近麦克风一点，或把系统「声音 → 输入」的音量调高。这次没有上屏",
                    peak * 100.0
                ))
            } else {
                (
                    "no_audio",
                    "麦克风没有收到声音：检查系统「声音 → 输入」是否选对设备，以及设备是否被静音"
                        .to_string(),
                )
            };
            if !too_quiet {
                if let Some(dev) = self.last_device.clone() {
                    msg =
                        format!("麦克风「{dev}」没有收到声音：检查它是否被静音，或换一个输入设备");
                }
            }
            self.emit_error(code, &msg);
            self.emit_state();
            return;
        }
        if speech_ms < min_ms as u64 {
            self.m.abort_processing();
            self.emit_error(
                "too_short",
                &format!("按住 {speech_ms}ms 太短了（至少 {min_ms}ms）：按住说完整句再松开"),
            );
            self.emit_state();
            return;
        }

        self.decoding = true;
        self.emit_state();
        // 渐进解码已经交出去的前缀 + 还没解的尾巴：松手只解尾巴
        let prefix = std::mem::take(&mut self.prog_text);
        let commit_pos = self.prog_pos.min(pcm.len());
        self.prog_pos = 0;
        let tail = pcm[commit_pos.min(pcm.len())..].to_vec();
        if std::env::var("ERYU_DEBUG_TAIL").is_ok() {
            eprintln!(
                "[eryu][tail] 总 {:.1}s 已提交 {:.1}s 尾巴 {:.1}s 前缀 {} 字",
                pcm.len() as f32 / audio::TARGET_RATE as f32,
                commit_pos as f32 / audio::TARGET_RATE as f32,
                tail.len() as f32 / audio::TARGET_RATE as f32,
                prefix.chars().count()
            );
        }
        let engine = self.engine.clone();
        let cfg = self.cfg();
        let tx = self.tx.clone();
        let task = task_id.clone();
        let prefix_for_thread = prefix.clone();
        let pcm_for_thread = if tail.len() >= audio::TARGET_RATE as usize / 4 {
            // 小声说话（离得远/系统输入音量低）先提上来再送 VAD/ASR（D-067）
            let mut v = audio::auto_gain(&tail);
            // 尾巴补 300ms 静音：松手瞬间音频是硬切的，VAD 需要一点静音才能把最后一段封口，
            // 否则「请把材料提前发到群里」会丢成「请把材料提前发到。」（实测）
            // 500ms：VAD 的 min_silence_duration 是 400ms，补得比它短会封不上口（会丢最后一两个字）
            v.extend(std::iter::repeat_n(
                0.0f32,
                audio::TARGET_RATE as usize * 5 / 10,
            ));
            v
        } else {
            Vec::new()
        };
        let hw = self.hotwords.clone();
        let app = self.foreground_app.clone();
        let prof_snapshot = self.profiles.lock().unwrap().current();
        let gate_arc = self.voice_gate.clone();
        let voice_lock_on = cfg.voice_lock && voicelock::load(&cfg.dir).is_some();
        // 音源锁定需要声纹模型（39MB，装在 models/ 下），没装就静默不启用
        let source_lock_on = cfg.source_lock && voicelock::model_path(&cfg.dir).exists();
        let _ = thread::Builder::new().name("eryu-decode".into()).spawn(move || {
            let t0 = Instant::now();
            let seg = if cfg.vad_ready() {
                match vad::Segmenter::new(&cfg.vad_model) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        eprintln!("[eryu] VAD 加载失败，退回固定窗口：{e}");
                        None
                    }
                }
            } else {
                None
            };
            if std::env::var("ERYU_DEBUG_TIMING").is_ok() {
                eprintln!("[eryu][t] 线程起点→VAD 就绪 {}ms", t0.elapsed().as_millis());
            }
            // 「只录我的声音」：先按声纹过滤段，再送识别。
            // 保底：过滤后剩下的段不到一半（可能阈值定太高/声纹模型不认），就整段重识别一遍，绝不把用户的话吞掉。
            let mut skipped = 0usize;
            let mut used_gate = false;
            let mut used_source_lock = false;
            let mut result = if pcm_for_thread.is_empty() {
                Ok(pipeline::PipelineResult::default())
            } else {
                let mut gate_guard = gate_arc.lock().unwrap();
                if voice_lock_on {
                    if gate_guard.is_none() {
                        let model = voicelock::model_path(&cfg.dir);
                        if let Some(print) = voicelock::load(&cfg.dir) {
                            *gate_guard = voicelock::Gate::new(&model, print.vec, cfg.voice_lock_threshold);
                        }
                    }
                    if let Some(gate) = gate_guard.as_ref() {
                        used_gate = true;
                        let filter = |pcm: &[f32]| gate.keep(pcm);
                        let mut guard = engine.lock().unwrap();
                        pipeline::run_filtered(
                            guard.as_mut(),
                            seg.as_ref(),
                            &pcm_for_thread,
                            audio::TARGET_RATE,
                            None,
                            Some(&filter),
                        )
                    } else {
                        let mut guard = engine.lock().unwrap();
                        pipeline::run(guard.as_mut(), seg.as_ref(), &pcm_for_thread, audio::TARGET_RATE)
                    }
                } else if source_lock_on {
                    // 音源锁定（D-065）：不用注册声纹，锁住这次录音里最先说话的人
                    used_source_lock = true;
                    let model = voicelock::model_path(&cfg.dir);
                    let lk = std::sync::Mutex::new(crate::sourcelock::Lock::new());
                    let filter = |pcm: &[f32]| -> bool {
                        let mut g = lk.lock().unwrap();
                        let idx = g.kept + g.dropped;
                        !matches!(
                            g.check(pcm, &model, cfg.voice_lock_threshold, idx),
                            crate::sourcelock::Decision::Drop
                        )
                    };
                    let mut guard = engine.lock().unwrap();
                    let out = pipeline::run_filtered(
                        guard.as_mut(),
                        seg.as_ref(),
                        &pcm_for_thread,
                        audio::TARGET_RATE,
                        None,
                        Some(&filter),
                    );
                    if let Ok(g) = lk.lock() {
                        eprintln!("[eryu][sourcelock] {}", g.summary());
                    }
                    out
                } else {
                    let t_lock = Instant::now();
                    let mut guard = engine.lock().unwrap();
                    let waited = t_lock.elapsed().as_millis();
                    let out = pipeline::run(guard.as_mut(), seg.as_ref(), &pcm_for_thread, audio::TARGET_RATE);
                    if std::env::var("ERYU_DEBUG_TIMING").is_ok() {
                        eprintln!("[eryu][t] 等引擎锁 {waited}ms 识别 {}ms", t_lock.elapsed().as_millis() - waited);
                    }
                    out
                }
            };
            if let Ok(r) = result.as_ref() {
                skipped = r.skipped;
            }
            if used_gate && skipped > 0 {
                let total = result.as_ref().map(|r| r.segments.len() + skipped).unwrap_or(0);
                let too_many = total >= 3 && skipped * 2 >= total;
                if too_many {
                    eprintln!("[eryu][voice] 声纹过滤掉 {skipped}/{total} 段，疑似不认你的声音，改为整段重识别");
                    result = {
                        let mut guard = engine.lock().unwrap();
                        pipeline::run(guard.as_mut(), seg.as_ref(), &pcm_for_thread, audio::TARGET_RATE)
                    };
                    skipped = 0;
                } else {
                    eprintln!("[eryu][voice] 声纹过滤掉 {skipped}/{total} 段");
                }
            }
            if std::env::var("ERYU_DEBUG_TIMING").is_ok() {
                eprintln!("[eryu][t] ASR 回来 {}ms", t0.elapsed().as_millis());
            }
            // 故障注入（只在设置了 ERYU_FAULT_FAIL_SEGMENTS 时生效）：把第 N 段判成失败，
            // 用来端到端验证「部分成功 → 只重试这一段」这条链路，不影响正常录音。
            if let Ok(n) = std::env::var("ERYU_FAULT_FAIL_SEGMENTS") {
                if let Ok(mut r) = result.as_mut() {
                    if let Some(k) = n.parse::<usize>().ok() {
                        let cap = 2 * audio::TARGET_RATE as usize;
                        let slice: Vec<f32> = pcm_for_thread.iter().take(cap).copied().collect();
                        for seg in r.segments.iter_mut() {
                            if seg.index == k {
                                seg.text.clear();
                                seg.error = Some("故障注入：模拟这一段识别失败".to_string());
                                seg.audio = Some(slice.clone());
                            }
                        }
                        r.failed = r.segments.iter().filter(|s| s.error.is_some()).count();
                        r.text = pipeline::join_segments(
                            &r.segments.iter().filter(|s| s.error.is_none()).map(|s| s.text.clone()).collect::<Vec<_>>(),
                        );
                    }
                }
            }
            // 流水线次序（DESIGN §4.2）：ASR → 热词替换 →（可选）润色 → 上屏。
            // 放在解码线程里，网络润色不会卡住 worker。
            let mut polish_note = None;
            let mut raw_text = String::new();
            let result = match result {
                Ok(mut r) => {
                    raw_text = if prefix_for_thread.trim().is_empty() {
                        r.text.trim().to_string()
                    } else {
                        pipeline::join_overlap(&prefix_for_thread, &r.text)
                    };
                    if !raw_text.is_empty() {
                        // 热词替换 → 标点（装了标点模型就用它补逗号句号，否则只兜底句号）
                        let t_punct = Instant::now();
                        let replaced = hw.lock().unwrap().apply(&raw_text);
                        // Qwen3 自己就带标点；再喂 ct-punc 会在词中间插句号（实测「周三。三下午」）
                        // 并叠出「。。」，所以它跳过标点模型（D-060）。
                        let want_punct = cfg.punct && cfg.engine != "qwen3";
                        let hot = if want_punct {
                            let model = punct::model_path(&cfg.dir);
                            match PUNCT_CACHE.apply(&model, &replaced) {
                                Some(p) => pipeline::add_final_punct(&p),
                                None => pipeline::add_final_punct(&replaced),
                            }
                        } else {
                            pipeline::add_final_punct(&replaced)
                        };
                        if std::env::var("ERYU_DEBUG_TIMING").is_ok() {
                            eprintln!("[eryu][t] 热词+标点 {}ms", t_punct.elapsed().as_millis());
                        }
                        let prof = prof_snapshot.clone();
                        let level = polish::Level::parse(&prof.level).unwrap_or(polish::Level::Off);
                        // 测试/验收（WAV 当麦克风）时不走润色：那是网络调用，
                        // 会把「松手→出字」的本地链路口径污染成 3–5s（实测 2.5s）。
                        // 联调时可以显式放开（ERYU_POLISH_IN_SIM=1），用来验证「润色 → 上屏」这段
                        // 在真实服务端下的表现；默认仍然跳过，保持本地链路口径干净。
                        let simulated = std::env::var("ERYU_MIC_WAV").is_ok()
                            && std::env::var("ERYU_POLISH_IN_SIM").ok().as_deref() != Some("1");
                        let polished_wanted = !simulated && cfg.polish_enabled
                            && level != polish::Level::Off
                            && !cfg.polish_api_key.trim().is_empty();
                        let text = if polished_wanted {
                            let req = polish::Request {
                                endpoint: cfg.polish_endpoint.clone(),
                                model: cfg.polish_model.clone(),
                                api_key: cfg.polish_api_key.clone(),
                                level,
                                timeout_ms: cfg.polish_timeout_ms,
                                app: app.clone(),
                                hotwords: Vec::new(),
                                extra_prompt: prof.prompt.clone(),
                            };
                            match polish::run(&req, &hot) {
                                Ok(p) => p,
                                Err(e) => {
                                    polish_note = Some(e);
                                    hot
                                }
                            }
                        } else {
                            hot
                        };
                        r.text = text;
                        if std::env::var("ERYU_DEBUG_TIMING").is_ok() {
                            eprintln!("[eryu][t] 文本组装好 {}ms", t0.elapsed().as_millis());
                        }
                    }
                    Ok(r)
                }
                Err(e) => Err(e),
            };
            if std::env::var("ERYU_DEBUG_TIMING").is_ok() {
                eprintln!("[eryu][t] 到 send 前 {}ms", t0.elapsed().as_millis());
            }
            let decode_ms = t0.elapsed().as_millis() as u64;
            let _ = tx.send(Cmd::Decoded {
                task_id: task,
                result,
                speech_ms,
                decode_ms,
                pcm: pcm_for_thread,
                raw_text,
                polish_note,
            });
        });
    }

    fn on_decoded(
        &mut self,
        task_id: &str,
        result: Result<pipeline::PipelineResult, String>,
        speech_ms: u64,
        decode_ms: u64,
        pcm: Vec<f32>,
        raw_text: String,
        polish_note: Option<String>,
    ) {
        self.decoding = false;
        if std::env::var("ERYU_DEBUG_TIMING").is_ok() {
            if let Some(t) = self.m.processing_since {
                eprintln!(
                    "[eryu][t] 松手→出文本 {}ms（decode_ms={decode_ms}）",
                    t.elapsed().as_millis()
                );
            }
        }
        let app = self.foreground_app.clone();
        match result {
            Ok(r) if r.failed == 0 && raw_text.trim().is_empty() && r.skipped > 0 => {
                self.m.abort_processing();
                self.emit_notice(
                    "voice_all_skipped",
                    "这段话都不像你的声音，已跳过（设置里可以关掉「只录我的声音」）",
                );
                self.emit_state();
            }
            Ok(r) if r.failed == 0 && raw_text.trim().is_empty() => {
                self.m.abort_processing();
                let peak = audio::peak(&pcm);
                let msg = if !audio::looks_like_speech(&pcm) {
                    "录音几乎是静音：麦克风可能被静音、选错了输入设备，或者离得太远".to_string()
                } else {
                    "声音太小或不清楚：靠近麦克风再说一次，或检查是否选错了输入设备".to_string()
                };
                self.emit_error("empty_result", &msg);
                self.emit_state();
            }
            Ok(r) => {
                // 模型没装时引擎回退成 stub，它会固定回一句「测试文本：你好，Eryu。」。
                // 把它当识别结果上屏等于往用户文档里塞假话，所以这里直接按失败处理。
                if self.engine_fallback.load(Ordering::SeqCst)
                    && raw_text.trim() == crate::asr::STUB_REPLY
                {
                    self.m.abort_processing();
                    self.emit_error(
                        "asr_failed",
                        "语音模型还没下载：打开设置 →「引擎」页，选一个模型后点「下载选中模型」。",
                    );
                    self.emit_state();
                    return;
                }
                if r.skipped > 0 {
                    self.emit_notice(
                        "voice_skipped",
                        &format!(
                            "略过 {} 段不像你的声音的内容（设置里可关「只录我的声音」）",
                            r.skipped
                        ),
                    );
                }
                // 验收口径：松手（进入 Processing）→ 出文本
                let latency_ms = self
                    .m
                    .processing_since
                    .map(|t| t.elapsed().as_millis() as u64);
                let partial = r.failed > 0;
                // 记住失败段（含音频）：用户可以在菜单里只重试这一段（DESIGN §10.2）
                self.failed_segments.clear();
                for seg in r.segments.iter() {
                    if seg.error.is_some() {
                        if let Some(a) = &seg.audio {
                            self.failed_segments.push((seg.index, a.clone()));
                        }
                    }
                }
                if let Ok(mut shared) = self.failed_indexes.lock() {
                    *shared = self.failed_segments.iter().map(|(i, _)| *i).collect();
                }
                self.emit(
                    "failed_segments",
                    serde_json::json!({
                        "count": self.failed_segments.len(),
                        "indexes": self.failed_segments.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
                    }),
                );
                let segments = r.segments.len() as u64;
                let text = r.text.trim().to_string();
                let privacy = self.privacy();
                let test = self.test_mode.load(Ordering::Relaxed);
                let id = if privacy || test {
                    // 隐私模式：不写历史（DESIGN §10.2）
                    String::new()
                } else {
                    self.store.lock().unwrap().record_success(
                        &text, "pending", &app, speech_ms, decode_ms, segments, latency_ms,
                    )
                };
                self.pending_history_id = if id.is_empty() {
                    None
                } else {
                    Some(id.clone())
                };
                if partial {
                    self.emit_error(
                        "partial_result",
                        &format!(
                            "{} 段里有 {} 段没识别成功，已保留成功部分",
                            segments, r.failed
                        ),
                    );
                }
                let payload = serde_json::json!({
                    "task_id": task_id,
                    "text": text,
                    "raw_text": raw_text,
                    "partial": partial,
                    "segments": r.segments.iter().map(|s| serde_json::json!({
                        "index": s.index, "samples": s.samples, "text": s.text, "error": s.error
                    })).collect::<Vec<_>>(),
                    "history_id": id,
                    "speech_ms": speech_ms,
                    "decode_ms": decode_ms,
                    // 松手 → 出结果（含热词/润色）的总耗时：验收口径 p95 ≤ 1.5s
                    "latency_ms": latency_ms.unwrap_or(0),
                    "wait_ms": self.commit_wait_ms,
                    "engine_wait_ms": self.engine_wait_ms,
                    "privacy": privacy,
                });
                if let Some(note) = polish_note {
                    self.emit(
                        "notice",
                        serde_json::json!({ "code": "polish_failed", "message": format!("润色没成功，已用原文：{note}") }),
                    );
                }
                self.emit("result", payload);
                *self.last.lock().unwrap() = Some(text.clone());
                // 上屏未确认前留一份：崩溃/强杀后下次启动还能取回（隐私模式不落盘）。
                // 自测模式下默认不落盘（D-055：不许污染用户数据）；但崩溃恢复的端到端验证需要它落盘，
                // 所以给一个显式开关 ERYU_KEEP_PENDING=1，只由 scripts/crash_recovery.sh 使用。
                let keep_pending = std::env::var("ERYU_KEEP_PENDING").is_ok();
                if !privacy && (!test || keep_pending) {
                    let _ = self.save_pending_result(&text);
                }
                self.m.commit_submit();
                self.emit_state();
                // mode 是建议：壳可按自己的能力拒绝（返回 undelivered）；M3 起按前台 app 覆盖
                let mode = {
                    let c = self.cfg();
                    crate::config::mode_for(&c, &app)
                };
                self.emit(
                    "submit_request",
                    serde_json::json!({
                        "task_id": task_id,
                        "text": text,
                        "phase": self.m.phase.as_str(),
                        "mode": mode,
                        "app": app,
                    }),
                );
                self.emit_stats();
            }
            Err(e) => {
                self.m.abort_processing();
                if self.privacy() || self.test_mode.load(Ordering::Relaxed) {
                    // 隐私模式 / 自测模式：失败音频只留内存（本次运行内可重试），不落盘、不写历史
                    self.last_failed = Some(pcm);
                    self.emit_error(
                        "asr_failed",
                        &format!("识别失败：{e}。隐私模式下音频没落盘，本次运行内可从菜单重试"),
                    );
                    self.emit_state();
                } else {
                    let path = self.cfg().failed_clip_path();
                    if let Err(werr) = audio::write_wav16(&path, &pcm) {
                        eprintln!("[eryu] 失败音频暂存失败：{werr}");
                    } else {
                        self.last_failed = Some(pcm);
                    }
                    let id = self
                        .store
                        .lock()
                        .unwrap()
                        .record_failure(&app, speech_ms, true);
                    self.emit_error(
                        "asr_failed",
                        &format!("识别失败：{e}。音频已保留 24 小时，可从菜单重试（历史 {id}）"),
                    );
                    self.emit_state();
                }
            }
        }
    }

    fn retry_failed(&mut self) {
        if self.m.phase != session::Phase::Idle {
            self.emit_error("busy", "正在录音或识别，稍后再重试");
            return;
        }
        let pcm = match self.last_failed.clone() {
            Some(p) => p,
            None => match audio::read_wav16(&self.cfg().failed_clip_path()) {
                Ok(p) => p,
                Err(_) => {
                    self.emit_error(
                        "retry_unavailable",
                        "没有可重试的音频（暂存已过期或从未失败）",
                    );
                    return;
                }
            },
        };
        match self.m.begin_retry() {
            session::Step::Finish { task_id } => {
                self.recorded_at = None;
                self.begin_decode(task_id, Some(pcm));
            }
            _ => self.emit_error("busy", "正在录音或识别，稍后再重试"),
        }
    }

    fn save_pending_result(&self, text: &str) -> Result<(), String> {
        let path = self.cfg().pending_dir().join("last_result.txt");
        fsutil::write_text_atomic(&path, text).map_err(|e| e.to_string())
    }

    fn clear_pending_result(&self) {
        let _ = std::fs::remove_file(self.cfg().pending_dir().join("last_result.txt"));
    }

    fn on_submit(&mut self, task_id: &str, status: &str, detail: &str) {
        let outcome = self.m.submit_result(task_id);
        // 超时（submit_timeout）之后壳才把 ok 报回来：文字其实已经进文档了，
        // 这时不能当没发生——否则历史永远停在「待复制」，下次启动还弹「上次没上屏」（I-142）。
        let late_ok = matches!(outcome, session::SubmitOutcome::NoPending)
            && status == "ok"
            && self.pending_history_id.is_some();
        if matches!(outcome, session::SubmitOutcome::Accepted) || late_ok {
            let id = self.pending_history_id.take();
            let mapped = match status {
                "ok" => "inserted",
                _ => "pending",
            };
            if let Some(id) = id {
                self.store.lock().unwrap().mark_status(&id, mapped);
            }
            if status == "ok" {
                self.clear_pending_result();
            }
            if status != "ok" {
                self.emit_error(
                    &format!("submit_{status}"),
                    if detail.is_empty() {
                        "无法自动粘贴，文字已保存，可从菜单复制"
                    } else {
                        detail
                    },
                );
            }
            self.emit_state();
            self.emit_stats();
        } else if matches!(outcome, session::SubmitOutcome::StaleTaskId) {
            self.emit_error("stale_submit", "上屏回报与当前任务不匹配，已忽略");
        }
    }

    // ---------- 电平测试 ----------

    fn start_meter(&mut self) {
        if self.meter.is_some() {
            return;
        }
        let cfg = self.cfg();
        let dn = if cfg.denoise {
            Some(crate::denoise::gtcrn_path(&cfg.dir)).filter(|p| p.exists())
        } else {
            None
        };
        match audio::start_with_opts(Some(cfg.device.as_str()), dn) {
            Ok(c) => {
                self.meter = Some(c);
                self.meter_scratch.clear();
                self.meter_last = Instant::now();
            }
            Err(e) => self.emit_error("mic_unavailable", &e),
        }
    }

    fn stop_meter(&mut self) {
        if let Some(c) = self.meter.take() {
            c.stopped();
        }
        self.meter_scratch.clear();
    }

    /// 渐进解码（DESIGN §4.4 / §4.6）：录音期间把「已经说过去的一段」当场解出来攒着，
    /// 松手时只解尾巴。这样 1–2 分钟的长听写不会在松手后卡十几秒。
    /// 切块尽量切在安静的缝里（audio::quiet_cut），减少把词切两半。
    fn tick_partial(&mut self, now: Instant) {
        if self.m.phase != session::Phase::Listening
            || self.decoding
            || self.partial_busy
            || self.partial_alive.load(Ordering::Relaxed)
        {
            return;
        }
        let end = self.recorded.len();
        let pending = end.saturating_sub(self.prog_pos);
        // 太短的块识别质量差（半个字），预览也等 0.8s 再开
        if pending < audio::TARGET_RATE as usize * 8 / 10 {
            return;
        }
        // 攒够一整块就别再等预览节流：提交块是「边录边解」的主线，节流会让长录音越积越多
        let chunk_now = audio::TARGET_RATE as usize
            * if self.cfg().engine.as_str() == "qwen3" {
                4
            } else {
                4
            };
        let commit_due = pending >= chunk_now;
        if let Some(due) = self.partial_due {
            if now < due && !commit_due {
                return;
            }
        }
        // 攒够一整块就「封口」（切在安静缝里），拼进前缀，松手时只剩尾巴；不够时只做预览。
        // 块时长按引擎的 RTF 定：SenseVoice（RTF≈0.08）10s 一块解 ~0.8s；Qwen3（RTF≈0.21）4s 一块解 ~0.85s，
        // 这样两种引擎的「松手→出字」都压在 1.5s 以内（D-060）。
        let chunk_secs: usize = match self.cfg().engine.as_str() {
            "qwen3" => 4,
            _ => 4,
        };
        // 短录音（<12s）不做「封口提交」，只预览最近 5 秒：
        // 提交大块会让松手时必须等它算完（实测 +0.8s），而短录音整段重解只要 ~0.85s。
        let chunk = audio::TARGET_RATE as usize * chunk_secs;
        let short_recording = self.prog_pos == 0 && end < audio::TARGET_RATE as usize * 12;
        let (range_start, range_end, commit) = if short_recording {
            let win = audio::TARGET_RATE as usize * 5;
            (end.saturating_sub(win), end, false)
        } else {
            let mut c = pending >= chunk;
            let cut = if !c {
                end
            } else {
                match audio::quiet_cut(&self.recorded, end) {
                    Some(x) => x,
                    // 连续说话没有安静缝：攒到两倍块长就硬切，避免尾巴无限长
                    None if pending >= chunk * 2 => end,
                    None => return,
                }
            };
            if cut <= self.prog_pos {
                return;
            }
            if cut == end && pending < chunk * 2 {
                c = false; // 只预览
            }
            (self.prog_pos, cut, c)
        };
        // 纯预览块只解末尾 5 秒：预览只是为了「让你看见正在录进去」，
        // 解 20 秒会长时间占住引擎锁，松手时最终识别得排队（实测多 1–2s）。
        let (range_start, range_end) = if commit {
            (range_start, range_end)
        } else {
            let win = audio::TARGET_RATE as usize * 5;
            (
                range_end
                    .saturating_sub(win)
                    .max(range_start.min(range_end)),
                range_end,
            )
        };
        if range_end <= range_start {
            return;
        }
        self.partial_busy = true;
        self.partial_commit = commit;
        // 只有「从开头覆盖到结尾」的预览，松手时才值得等它、直接采用它的文本
        self.partial_up_to = if range_start == 0 { range_end } else { 0 };
        let interval = partial_interval_ms(self.partial_last_ms);
        self.partial_due = Some(now + Duration::from_millis(interval));
        let pcm = self.recorded[range_start..range_end].to_vec();
        let engine = self.engine.clone();
        let tx = self.tx.clone();
        let vad_model = self.cfg().vad_model.clone();
        let cancel = self.partial_cancel.clone();
        self.partial_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.partial_alive.store(true, Ordering::Relaxed);
        let alive = self.partial_alive.clone();
        let _ = thread::Builder::new()
            .name("eryu-progressive".into())
            .spawn(move || {
                let seg = vad::Segmenter::new(&vad_model).ok();
                let t0 = Instant::now();
                let out = {
                    let lock_t0 = Instant::now();
                    let mut guard = engine.lock().unwrap();
                    if lock_t0.elapsed().as_millis() > 50 {}
                    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        Ok(pipeline::PipelineResult::default())
                    } else {
                        pipeline::run_cancellable(
                            guard.as_mut(),
                            seg.as_ref(),
                            &pcm,
                            audio::TARGET_RATE,
                            Some(&cancel),
                        )
                    }
                };
                let text = match out {
                    Ok(r) => r.text.trim().to_string(),
                    Err(_) => String::new(),
                };
                let ms = t0.elapsed().as_millis() as u64;
                // 只推进到切点：那块补白静音会被下一块重新覆盖，保证切点上的字不被两段都丢掉
                let _ = tx.send(Cmd::Partial {
                    text,
                    ms,
                    up_to: range_end,
                    commit,
                });
                alive.store(false, Ordering::Relaxed);
            });
    }

    /// 单段重试：只把这一段音频再送一次模型，结果走 segment_result 事件（不自动上屏）
    fn retry_segment(&mut self, index: usize) {
        let Some(samples) = self
            .failed_segments
            .iter()
            .find(|(i, _)| *i == index)
            .map(|(_, a)| a.clone())
        else {
            self.emit_error(
                "segment_not_found",
                &format!("第 {index} 段没有可重试的音频"),
            );
            return;
        };
        if self.decoding {
            self.emit_error("busy", "正在识别，稍后再重试这一段");
            return;
        }
        self.emit(
            "segment_retry_started",
            serde_json::json!({ "index": index, "samples": samples.len() }),
        );
        let tx = self.tx.clone();
        let engine = self.engine.clone();
        let busy = self.retry_busy.clone();
        busy.store(true, Ordering::SeqCst);
        let spawned = std::thread::Builder::new()
            .name("eryu-segment".into())
            .spawn(move || {
                let result = engine
                    .lock()
                    .map_err(|_| "识别引擎状态异常".to_string())
                    .and_then(|mut e| e.transcribe(&samples, 16000));
                let _ = tx.send(Cmd::SegmentDone { index, result });
            });
        if spawned.is_err() {
            busy.store(false, Ordering::SeqCst);
            self.emit_error("segment_spawn", "起不了重试线程");
        }
    }

    fn on_segment_done(&mut self, index: usize, result: Result<String, String>) {
        self.retry_busy.store(false, Ordering::SeqCst);
        match result {
            Ok(text) => {
                let chars = text.chars().count();
                if text.trim().is_empty() {
                    self.emit_error("segment_empty", &format!("第 {index} 段重试后还是空的"));
                    return;
                }
                self.emit(
                    "segment_result",
                    serde_json::json!({ "index": index, "text": text, "chars": chars }),
                );
                // 成功后从待重试列表里移除，避免反复点同一段
                self.failed_segments.retain(|(i, _)| *i != index);
                if let Ok(mut shared) = self.failed_indexes.lock() {
                    *shared = self.failed_segments.iter().map(|(i, _)| *i).collect();
                }
            }
            Err(e) => self.emit_error(
                "segment_retry_failed",
                &format!("第 {index} 段重试失败：{e}"),
            ),
        }
    }

    fn on_partial(&mut self, text: String, ms: u64, up_to: usize, commit: bool) {
        self.partial_busy = false;
        self.partial_last_ms = ms;
        // 松手后迟到的块：已算完的部分照样提交（省一次重复识别），然后继续解尾巴
        if self.m.phase != session::Phase::Listening {
            if up_to > self.prog_pos && !text.trim().is_empty() {
                self.prog_text = pipeline::join_overlap(&self.prog_text.clone(), &text);
                self.prog_pos = up_to;
            }
            if let Some(task) = self.waiting_commit.take() {
                self.start_decode(task, None);
            }
            return;
        }
        let shown = if commit {
            if up_to > self.prog_pos && !text.trim().is_empty() {
                self.prog_text = pipeline::join_overlap(&self.prog_text.clone(), &text);
            }
            if up_to > self.prog_pos {
                self.prog_pos = up_to;
            }
            self.prog_text.trim().to_string()
        } else {
            // 预览：前缀 + 还没封口的尾巴（尾巴会变，所以只显示不提交）
            pipeline::join_overlap(&self.prog_text, &text)
                .trim()
                .to_string()
        };
        if shown.is_empty() || shown == self.partial_last {
            return;
        }
        self.partial_last = shown.clone();
        self.emit(
            "partial_text",
            serde_json::json!({ "text": shown, "chars": shown.chars().count(), "ms": ms }),
        );
    }

    fn tick_meter(&mut self, now: Instant) {
        let Some(c) = &self.meter else { return };
        c.drain(&mut self.meter_scratch);
        if now.duration_since(self.meter_last).as_millis() < METER_INTERVAL_MS as u128 {
            return;
        }
        self.meter_last = now;
        let mut peak = 0f32;
        let mut sum = 0f64;
        for s in &self.meter_scratch {
            let a = s.abs();
            if a > peak {
                peak = a;
            }
            sum += (a as f64) * (a as f64);
        }
        let rms = if self.meter_scratch.is_empty() {
            0.0
        } else {
            (sum / self.meter_scratch.len() as f64).sqrt() as f32
        };
        self.meter_scratch.clear();
        let payload = serde_json::json!({
            "rms": (rms * 100.0).min(100.0) / 100.0,
            "peak": (peak * 100.0).min(100.0) / 100.0,
            "silent": rms < 0.003,
        });
        self.emit("meter", payload);
    }

    // ---------- 配置 ----------

    fn reload_hotwords(&mut self, dir: &Path) {
        *self.hotwords.lock().unwrap() = hotwords::Hotwords::load(dir);
    }

    fn apply_config(&mut self, cfg: config::Config, gen: u64) {
        use std::sync::atomic::Ordering::SeqCst;
        // 锁内比较：如果壳已经写进更新的版本，这条旧命令连盘都不要落
        {
            let g = self.cfg.lock().unwrap();
            if gen < self.cfg_applied.load(SeqCst) {
                drop(g);
                return;
            }
        }
        let store_dir = cfg.dir.clone();
        let old_engine = self.cfg().engine.clone();
        if let Err(e) = cfg.save() {
            self.emit_error("config_save_failed", &format!("保存配置失败：{e}"));
            return;
        }
        let enabled = cfg.history_enabled;
        {
            let mut g = self.cfg.lock().unwrap();
            *g = cfg.clone();
            self.cfg_applied.store(gen, SeqCst);
        }
        self.reload_store(&store_dir, enabled, cfg.history_limit);
        self.reload_hotwords(&store_dir);
        let active = self.cfg().engine.clone();
        self.emit_config_changed(&active);
        if active != old_engine {
            self.rebuild_engine(&active);
        }
        // 局域网接口改了开关/端口/token/allow_lan：不用重启 App，直接重开监听
        let want = {
            let c = self.cfg.lock().unwrap();
            (
                c.lan_enabled,
                c.lan_port,
                c.lan_token.clone(),
                c.lan_allow_lan,
            )
        };
        if self.lan_cfg.as_ref() != Some(&want) {
            crate::lan::stop();
            self.start_lan_if_enabled();
        }
    }

    fn set_engine(&mut self, id: &str) {
        let mut cfg = self.cfg();
        if !matches!(id, "sense" | "qwen3" | "stub") {
            self.emit_error("bad_engine", &format!("未知引擎：{id}"));
            return;
        }
        if id == "sense" && !cfg.models_ready() {
            self.emit_error("model_missing", "SenseVoice 模型未安装：设置页可下载");
            return;
        }
        if id == "qwen3" && !cfg.qwen3_dir.join("encoder.int8.onnx").exists() {
            self.emit_error("model_missing", "Qwen3 模型未安装：设置页可下载");
            return;
        }
        cfg.engine = id.to_string();
        if let Err(e) = cfg.save() {
            self.emit_error("config_save_failed", &format!("保存配置失败：{e}"));
            return;
        }
        *self.cfg.lock().unwrap() = cfg;
        self.emit_config_changed(id);
        self.rebuild_engine(id);
    }

    fn emit_config_changed(&mut self, engine: &str) {
        let toml = self.cfg().to_toml();
        self.emit(
            "config_changed",
            serde_json::json!({ "engine": engine, "toml": toml }),
        );
    }

    fn rebuild_engine(&mut self, id: &str) {
        let cfg = self.cfg();
        let tx = self.tx.clone();
        let id = id.to_string();
        let _ = thread::Builder::new()
            .name("eryu-engine".into())
            .spawn(move || {
                let result = asr::make_engine(&cfg);
                if let Ok(e) = &result {
                    eprintln!("[eryu] 引擎已切换：{}", e.name());
                }
                let _ = tx.send(Cmd::EngineBuilt {
                    engine_id: id,
                    result,
                });
            });
    }

    fn reload_store(&self, dir: &std::path::Path, enabled: bool, limit: usize) {
        let mut s = self.store.lock().unwrap();
        if s.enabled != enabled {
            *s = store::Store::load(dir, enabled, limit);
        }
    }
}

#[cfg(test)]
mod partial_tests {
    use super::*;

    #[test]
    fn interval_adapts_to_decode_cost() {
        assert_eq!(partial_interval_ms(0), PARTIAL_MIN_MS); // 还没解过：最快
        assert_eq!(partial_interval_ms(60), PARTIAL_MIN_MS); // 短句很快：仍然最快档
        assert_eq!(partial_interval_ms(200), 400); // 中段：给两倍余量
        assert_eq!(partial_interval_ms(900), PARTIAL_MAX_MS); // 长录音解码慢：退到最慢档
    }
}

#[cfg(test)]
mod partial_window_tests {
    use super::*;

    #[test]
    fn windows_tail_only_for_long_recordings() {
        let short = vec![0f32; audio::TARGET_RATE as usize * 5];
        let (slice, cut) = partial_window(&short);
        assert_eq!(slice.len(), short.len());
        assert!(!cut);

        let long = vec![0f32; audio::TARGET_RATE as usize * 90];
        let (slice, cut) = partial_window(&long);
        assert_eq!(slice.len(), PARTIAL_TAIL_SECS * audio::TARGET_RATE as usize);
        assert!(cut);
        // 取的是尾部：与原始缓冲区末尾对齐
        assert_eq!(slice.as_ptr(), long[long.len() - slice.len()..].as_ptr());
    }
}

/// 装完的模型真的能加载吗？（I-103：体积对、内容坏的情况）
/// 只对「能便宜验证」的类型做：语音/标点模型尝试构建一次（几百 ms），其余看标志文件。
fn validate_installed_model(models_dir: &Path, entry: &models::Entry) -> Result<(), String> {
    let marker = models_dir.join(entry.marker);
    if !marker.exists() {
        return Err(format!("解包后没找到 {}", marker.display()));
    }
    let dir = marker.parent().unwrap_or(models_dir);
    match entry.id {
        "sense" => asr::SenseVoiceEngine::load(dir).map(|_| ()),
        "qwen3" => asr::Qwen3Engine::load(dir).map(|_| ()),
        "punct" => {
            let p =
                punct::Punctuator::load(&marker).ok_or_else(|| "标点模型加载失败".to_string())?;
            let _ = p.apply("这是一次加载自检");
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod model_validate_tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("eryu-val-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn corrupt_onnx_is_rejected() {
        let dir = tmp("sense");
        let sub = dir.join("sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("model.int8.onnx"), b"definitely not an onnx model").unwrap();
        let entry = models::find("sense").unwrap();
        let err = validate_installed_model(&dir, entry).unwrap_err();
        assert!(!err.is_empty(), "坏模型应给出错误说明");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_marker_is_rejected() {
        let dir = tmp("missing");
        let entry = models::find("gtcrn").unwrap();
        let err = validate_installed_model(&dir, entry).unwrap_err();
        assert!(err.contains("没找到"), "应提示文件缺失: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
