//! ASR 引擎抽象与 sherpa-onnx 实现（DESIGN §4.2）
//!
//! 首版默认 SenseVoice（int8，~166MB 下载 / 峰值 ~600MB，11.4s 音频 RTF 0.018）。
//! Qwen3-0.6B 留作「短句精度档」（峰值 1.6GB、30s 硬上限）。

use std::path::Path;
use std::time::Instant;

use crate::config::Config;

/// 识别引擎：一次吃一段 ≤30s 的 16k 单声道 PCM
pub trait AsrEngine: Send {
    fn name(&self) -> &str;
    fn transcribe(&mut self, pcm: &[f32], sample_rate: u32) -> Result<String, String>;
}

/// 测试用假引擎：不加载任何模型
/// stub 引擎的固定回复。壳/核心用它识别「这段话不是真的识别出来的」，
/// 避免模型没装时把假文本上屏（见 lib.rs `on_decoded` 的守卫）。
pub const STUB_REPLY: &str = "测试文本：你好，Eryu。";

pub struct StubEngine {
    pub reply: String,
}

impl Default for StubEngine {
    fn default() -> Self {
        Self {
            reply: STUB_REPLY.to_string(),
        }
    }
}

impl AsrEngine for StubEngine {
    fn name(&self) -> &str {
        "stub"
    }
    fn transcribe(&mut self, _pcm: &[f32], _sample_rate: u32) -> Result<String, String> {
        Ok(self.reply.clone())
    }
}

pub struct SenseVoiceEngine {
    recognizer: sherpa_onnx::OfflineRecognizer,
    load_ms: u128,
}

impl SenseVoiceEngine {
    /// `dir` 内需有 model.int8.onnx 与 tokens.txt
    pub fn load(dir: &Path) -> Result<Self, String> {
        let model = dir.join("model.int8.onnx");
        let tokens = dir.join("tokens.txt");
        if !model.exists() {
            return Err(format!("缺少模型文件：{}", model.display()));
        }
        if !tokens.exists() {
            return Err(format!("缺少 tokens：{}", tokens.display()));
        }

        let t = Instant::now();
        let mut c = sherpa_onnx::OfflineRecognizerConfig::default();
        c.model_config.sense_voice.model = Some(model.to_string_lossy().into_owned());
        c.model_config.sense_voice.language = Some("auto".into());
        c.model_config.sense_voice.use_itn = true;
        c.model_config.tokens = Some(tokens.to_string_lossy().into_owned());
        // 实测：1/2/4 线程无差别，8 线程退化 → 固定 2
        c.model_config.num_threads = 2;
        let recognizer = sherpa_onnx::OfflineRecognizer::create(&c)
            .ok_or_else(|| "OfflineRecognizer::create 失败（模型损坏或内存不足）".to_string())?;
        Ok(Self {
            recognizer,
            load_ms: t.elapsed().as_millis(),
        })
    }

    pub fn load_ms(&self) -> u128 {
        self.load_ms
    }
}

impl AsrEngine for SenseVoiceEngine {
    fn name(&self) -> &str {
        "sense-voice-int8"
    }

    fn transcribe(&mut self, pcm: &[f32], sample_rate: u32) -> Result<String, String> {
        if pcm.is_empty() {
            return Ok(String::new());
        }
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(sample_rate as i32, pcm);
        self.recognizer.decode(&stream);
        let result = stream
            .get_result()
            .ok_or_else(|| "识别未返回结果".to_string())?;
        Ok(result.text.trim().to_string())
    }
}

/// Qwen3-ASR-0.6B（短句精度档）：峰值内存约 1.6GB，单段硬上限约 30s（512 token）
pub struct Qwen3Engine {
    recognizer: sherpa_onnx::OfflineRecognizer,
    load_ms: u128,
}

impl Qwen3Engine {
    /// `dir` 内需有 conv_frontend.onnx、encoder.int8.onnx、decoder.int8.onnx、tokenizer/
    pub fn load(dir: &Path) -> Result<Self, String> {
        let frontend = dir.join("conv_frontend.onnx");
        let encoder = dir.join("encoder.int8.onnx");
        let decoder = dir.join("decoder.int8.onnx");
        let tokenizer = dir.join("tokenizer");
        for (p, what) in [
            (&frontend, "conv_frontend.onnx"),
            (&encoder, "encoder.int8.onnx"),
            (&decoder, "decoder.int8.onnx"),
            (&tokenizer, "tokenizer 目录"),
        ] {
            if !p.exists() {
                return Err(format!("缺少模型文件：{}（{what}）", p.display()));
            }
        }

        let t = Instant::now();
        let mut c = sherpa_onnx::OfflineRecognizerConfig::default();
        let q = &mut c.model_config.qwen3_asr;
        q.conv_frontend = Some(frontend.to_string_lossy().into_owned());
        q.encoder = Some(encoder.to_string_lossy().into_owned());
        q.decoder = Some(decoder.to_string_lossy().into_owned());
        q.tokenizer = Some(tokenizer.to_string_lossy().into_owned());
        // 官方默认 512 token ≈ 30s；段上限在 VAD 层已按 30s 切
        q.max_total_len = 512;
        q.max_new_tokens = 256;
        q.temperature = 0.0;
        c.model_config.num_threads = 2;
        let recognizer = sherpa_onnx::OfflineRecognizer::create(&c)
            .ok_or_else(|| "Qwen3 recognizer 创建失败（模型损坏或内存不足）".to_string())?;
        Ok(Self {
            recognizer,
            load_ms: t.elapsed().as_millis(),
        })
    }

    pub fn load_ms(&self) -> u128 {
        self.load_ms
    }
}

impl AsrEngine for Qwen3Engine {
    fn name(&self) -> &str {
        "qwen3-asr-0.6b-int8"
    }

    fn transcribe(&mut self, pcm: &[f32], sample_rate: u32) -> Result<String, String> {
        if pcm.is_empty() {
            return Ok(String::new());
        }
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(sample_rate as i32, pcm);
        self.recognizer.decode(&stream);
        let result = stream
            .get_result()
            .ok_or_else(|| "识别未返回结果".to_string())?;
        Ok(result.text.trim().to_string())
    }
}

/// 按配置造引擎；失败返回错误说明（调用方决定是否回退 stub）。
pub fn make_engine(cfg: &Config) -> Result<Box<dyn AsrEngine>, String> {
    match cfg.engine.as_str() {
        "stub" => Ok(Box::new(StubEngine::default())),
        "qwen3" => {
            if !cfg.models_ready() {
                return Err(format!("Qwen3 模型未安装：{}", cfg.qwen3_dir.display()));
            }
            let e = Qwen3Engine::load(&cfg.qwen3_dir)?;
            eprintln!("[eryu] Qwen3 就绪，加载 {}ms", e.load_ms());
            Ok(Box::new(e))
        }
        other if other != "sense" => Err(format!("未知引擎：{other}（可选 sense|qwen3|stub）")),
        _ => {
            if !cfg.models_ready() {
                return Err(format!(
                    "SenseVoice 模型未安装：{}",
                    cfg.model_dir.display()
                ));
            }
            let e = SenseVoiceEngine::load(&cfg.model_dir)?;
            eprintln!("[eryu] SenseVoice 就绪，加载 {}ms", e.load_ms());
            Ok(Box::new(e))
        }
    }
}
