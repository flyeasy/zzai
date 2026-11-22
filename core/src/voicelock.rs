//! 「只录我的声音」——声纹门控（DESIGN §9.7 的「音源锁定」实验，M4+）
//!
//! 只做一件保守的事：录音切成段后，用说话人 embedding 跟注册时的声纹比一比，
//! **明显不像**的段直接丢掉、不送去识别。阈值故意偏松（默认 0.62）：
//! 同一人不同句子实测 0.91–0.93，不同人同一句话 0.49–0.57（TTS A/B，见 docs/DECISIONS.md D-051）。
//! 任何「全都像/全都不像」的异常都由上层兜底（skipped 过半就整段重识别一遍，避免把用户自己的声音全滤掉）。

use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FILE_NAME: &str = "voice_print.bin";
pub const MODEL_FILE: &str = "3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoicePrint {
    pub model: String,
    pub dim: usize,
    pub created: String,
    pub vec: Vec<f32>,
}

pub fn path(dir: &Path) -> std::path::PathBuf {
    dir.join(FILE_NAME)
}

pub fn model_path(dir: &Path) -> std::path::PathBuf {
    dir.join("models").join(MODEL_FILE)
}

pub fn load(dir: &Path) -> Option<VoicePrint> {
    let data = std::fs::read(path(dir)).ok()?;
    serde_json::from_slice(&data).ok()
}

pub fn save(dir: &Path, print: &VoicePrint) -> Result<(), String> {
    let json = serde_json::to_vec(print).map_err(|e| e.to_string())?;
    crate::fsutil::write_atomic(&path(dir), &json).map_err(|e| e.to_string())
}

pub fn clear(dir: &Path) -> Result<(), String> {
    match std::fs::remove_file(path(dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// 一段音频的 embedding（16k 单声道 f32）。模型加载失败/音频太短都返回 None。
pub fn embed(model: &Path, pcm: &[f32]) -> Option<Vec<f32>> {
    use sherpa_onnx::{SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig};
    let cfg = SpeakerEmbeddingExtractorConfig {
        model: Some(model.to_string_lossy().to_string()),
        num_threads: 2,
        debug: false,
        provider: Some("cpu".into()),
    };
    let ex = SpeakerEmbeddingExtractor::create(&cfg)?;
    let stream = ex.create_stream()?;
    stream.accept_waveform(16000, pcm);
    stream.input_finished();
    if !ex.is_ready(&stream) {
        return None;
    }
    ex.compute(&stream)
}

/// 门控：常驻一个 extractor，逐段算相似度
pub struct Gate {
    ex: sherpa_onnx::SpeakerEmbeddingExtractor,
    print: Vec<f32>,
    pub threshold: f32,
}

impl Gate {
    pub fn new(model: &Path, print: Vec<f32>, threshold: f32) -> Option<Self> {
        let cfg = sherpa_onnx::SpeakerEmbeddingExtractorConfig {
            model: Some(model.to_string_lossy().to_string()),
            num_threads: 2,
            debug: false,
            provider: Some("cpu".into()),
        };
        let ex = sherpa_onnx::SpeakerEmbeddingExtractor::create(&cfg)?;
        Some(Self {
            ex,
            print,
            threshold,
        })
    }

    pub fn score(&self, pcm: &[f32]) -> Option<f32> {
        let stream = self.ex.create_stream()?;
        stream.accept_waveform(16000, pcm);
        stream.input_finished();
        if !self.ex.is_ready(&stream) {
            return None;
        }
        let e = self.ex.compute(&stream)?;
        Some(cosine(&self.print, &e))
    }

    /// 不像（或算不出来）→ 丢掉这一段。算不出来时偏向「保留」：宁可多识别一次，也别把用户的话吞掉。
    pub fn keep(&self, pcm: &[f32]) -> bool {
        match self.score(pcm) {
            Some(s) => s >= self.threshold,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_hits_one_for_same_vector() {
        let v = vec![0.5, 0.2, -0.3, 0.9];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-5);
        assert!(cosine(&v, &vec![-0.5, -0.2, 0.3, -0.9]) < -0.99);
    }

    #[test]
    fn voice_print_roundtrip_and_clear() {
        let dir = std::env::temp_dir().join(format!("eryu-vp-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = VoicePrint {
            model: "m".into(),
            dim: 3,
            created: "now".into(),
            vec: vec![0.1, 0.2, 0.3],
        };
        save(&dir, &p).unwrap();
        let back = load(&dir).expect("读回");
        assert_eq!(back.vec, p.vec);
        clear(&dir).unwrap();
        assert!(load(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
