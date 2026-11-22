//! VAD 分段（DESIGN §4.4：段上限 30s、静音 400ms 封口、段间重叠留给流水线）

use std::path::Path;

use sherpa_onnx::{VadModelConfig, VoiceActivityDetector};

/// 单段送入模型的上限（SenseVoice/Qwen3 都是 ≤30s 段模型）
pub const SEGMENT_MAX_SECS: f32 = 30.0;
/// 静音多久封口（DESIGN §4.5 min_silence 400ms）
pub const SILENCE_SECS: f32 = 0.4;
/// 最短语音，过短不送模型
pub const MIN_SPEECH_SECS: f32 = 0.25;
/// Silero VAD 的窗口长度（16k 采样点）。**必须按此长度喂**：
/// 实测把整段一次性交给 `accept_waveform` 只会吐出最后一小段（probe: 5.6s 语音只剩 0.31s）。
pub const WINDOW_SAMPLES: usize = 512;

pub struct Segmenter {
    vad: VoiceActivityDetector,
}

impl Segmenter {
    pub fn new(model: &Path) -> Result<Self, String> {
        if !model.exists() {
            return Err(format!("缺少 VAD 模型：{}", model.display()));
        }
        let mut cfg = VadModelConfig::default();
        cfg.silero_vad.model = Some(model.to_string_lossy().into_owned());
        cfg.silero_vad.threshold = 0.5;
        cfg.silero_vad.min_silence_duration = SILENCE_SECS;
        cfg.silero_vad.min_speech_duration = MIN_SPEECH_SECS;
        cfg.silero_vad.window_size = WINDOW_SAMPLES as i32;
        // max_speech_duration 让 VAD 自己切 30s，不靠喂整段
        cfg.silero_vad.max_speech_duration = SEGMENT_MAX_SECS;
        cfg.sample_rate = 16000;
        cfg.num_threads = 1;
        let vad = VoiceActivityDetector::create(&cfg, 60.0)
            .ok_or_else(|| "VAD 创建失败（模型损坏？）".to_string())?;
        Ok(Self { vad })
    }

    /// 喂入 16k 单声道，返回本次封口的完整段（内部按 VAD 窗口切，逐窗喂）
    pub fn accept(&self, samples: &[f32]) -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        for chunk in samples.chunks(WINDOW_SAMPLES) {
            self.vad.accept_waveform(chunk);
            out.extend(self.drain());
        }
        out
    }

    /// 会话结束：把最后一段也吐出来
    pub fn flush(&self) -> Vec<Vec<f32>> {
        self.vad.flush();
        self.drain()
    }

    fn drain(&self) -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        while let Some(seg) = self.vad.front() {
            let s = seg.samples().to_vec();
            if !s.is_empty() {
                out.push(s);
            }
            self.vad.pop();
        }
        out
    }
}
