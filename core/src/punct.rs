//! 标点恢复（可选，DESIGN §4.6 的补强）
//!
//! ASR 输出本身常带标点，但 VAD 切段后拼接、以及某些引擎（SenseVoice 短段）会丢标点。
//! 这里用 sherpa-onnx 的 ct-transformer 标点模型在**本机**补一次，模型 65MB（int8），
//! 实测 10ms/句、加载 99ms。没装模型就退回 `pipeline::add_final_punct`（只补句末「。」）。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sherpa_onnx::{OfflinePunctuation, OfflinePunctuationConfig};

/// 模型目录名（与 models.rs 的 `punct` 条目一致）
pub const DIR_NAME: &str = "sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8";

/// 模型文件放在 `models/<DIR_NAME>/model.int8.onnx`
pub fn model_path(config_dir: &Path) -> PathBuf {
    config_dir
        .join("models")
        .join(DIR_NAME)
        .join("model.int8.onnx")
}

pub struct Punctuator {
    inner: OfflinePunctuation,
}

impl Punctuator {
    pub fn load(model: &Path) -> Option<Self> {
        if !model.exists() {
            return None;
        }
        let mut cfg = OfflinePunctuationConfig::default();
        cfg.model.ct_transformer = Some(model.to_string_lossy().to_string());
        cfg.model.num_threads = 1;
        let inner = OfflinePunctuation::create(&cfg)?;
        Some(Self { inner })
    }

    /// 加标点；模型返回空/None 时调用方应退回兜底
    pub fn apply(&self, text: &str) -> Option<String> {
        if text.trim().is_empty() {
            return None;
        }
        let out = self.inner.add_punctuation(text)?;
        if out.trim().is_empty() {
            return None;
        }
        Some(out)
    }
}

/// 懒加载的标点器：没装模型 / 加载失败都保持 None（每次都试一次代价是 99ms，所以失败后记住）
pub struct Cache {
    state: Mutex<Option<Option<Punctuator>>>,
}

impl Cache {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(None),
        }
    }

    /// 直接在内部完成「加载 + 加标点」，调用方拿不到 Guard，避免生命周期麻烦
    pub fn apply(&self, model: &Path, text: &str) -> Option<String> {
        let mut g = self.state.lock().unwrap();
        if g.is_none() {
            *g = Some(Punctuator::load(model));
        }
        g.as_ref()
            .and_then(|p| p.as_ref())
            .and_then(|p| p.apply(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_is_none_not_panic() {
        let c = Cache::new();
        assert!(c
            .apply(Path::new("/tmp/根本没有这个标点模型.onnx"), "你好")
            .is_none());
    }

    #[test]
    fn path_shape() {
        let p = model_path(Path::new("/x"));
        assert!(p.ends_with("model.int8.onnx"));
        assert!(p.to_string_lossy().contains(DIR_NAME));
    }
}
