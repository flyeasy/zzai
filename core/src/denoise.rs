//! 背景降噪（DESIGN §9.7）。两档实现：
//!
//! - `Gtcrn`（**默认**，sherpa-onnx 自带模型 535KB）：流式、原生 16k、RTF≈0.03。
//!   实测 0dB 白噪声下把「今天下午三点在会议室评审设计文档…」整个救回来（不降噪只剩「评审受聘文档…」），
//!   -5dB 也救回句首，-10dB 两者都对。
//! - `Denoiser`（RNNoise，48k 480 样本/帧）：保留给离线实验与 48k 路径。
//!   它的判断依赖 8k 以上真实频带，16k 上采样路径会把字听错（D-035），所以只在原生 48k 流上可用。

use nnnoiseless::DenoiseState;

const FRAME: usize = 480; // 10ms @ 48kHz
const UP: usize = 3; // 16k -> 48k

pub struct Denoiser {
    st: Box<DenoiseState<'static>>,
}

impl Default for Denoiser {
    fn default() -> Self {
        Self::new()
    }
}

impl Denoiser {
    pub fn new() -> Self {
        Self {
            st: DenoiseState::new(),
        }
    }

    /// 采集侧：直接吃 48kHz 原声的 480 样本一帧（最理想，无上采样损失）
    pub fn process_frame_48k(&mut self, frame: &[f32]) -> Vec<f32> {
        let mut fin = vec![0f32; FRAME];
        let mut fout = vec![0f32; FRAME];
        for i in 0..FRAME {
            fin[i] = frame.get(i).copied().unwrap_or(0.0) * 32768.0;
        }
        self.st.process_frame(&mut fout, &fin);
        for v in fout.iter_mut() {
            *v /= 32768.0;
        }
        fout
    }

    /// 处理一段 16kHz 单声道；返回同样长度的 16kHz 数据
    pub fn process(&mut self, pcm16k: &[f32]) -> Vec<f32> {
        if pcm16k.is_empty() {
            return Vec::new();
        }
        // 1) 线性插值 ×3 → 48k
        let mut up: Vec<f32> = Vec::with_capacity(pcm16k.len() * UP + FRAME);
        for i in 0..pcm16k.len() {
            let a = pcm16k[i];
            let b = *pcm16k.get(i + 1).unwrap_or(&a);
            for k in 0..UP {
                let t = k as f32 / UP as f32;
                up.push(a + (b - a) * t);
            }
        }
        // 补零到整帧
        let frames = up.len().div_ceil(FRAME);
        up.resize(frames * FRAME, 0.0);

        // 2) 逐帧降噪（RNNoise 期望 i16 量级输入）
        let mut out = vec![0f32; frames * FRAME];
        let mut fin = vec![0f32; FRAME];
        let mut fout = vec![0f32; FRAME];
        for f in 0..frames {
            for i in 0..FRAME {
                fin[i] = up[f * FRAME + i] * 32768.0;
            }
            self.st.process_frame(&mut fout, &fin);
            for i in 0..FRAME {
                out[f * FRAME + i] = fout[i] / 32768.0;
            }
        }

        // 3) 抽回 16k（取每帧第 0 个点，等价 3 倍抽取）
        let want = pcm16k.len();
        let mut back = Vec::with_capacity(want);
        for i in 0..want {
            back.push(out.get(i * UP).copied().unwrap_or(0.0));
        }
        back
    }
}

/// 噪声段能量比：粗略衡量「压了多少」（测试与诊断用）
pub fn rms(pcm: &[f32]) -> f32 {
    if pcm.is_empty() {
        return 0.0;
    }
    (pcm.iter().map(|s| (s * s) as f64).sum::<f64>() / pcm.len() as f64).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize) -> Vec<f32> {
        // 确定性伪随机（不用额外依赖）
        let mut x: u32 = 0x1234_5678;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                ((x as f32 / u32::MAX as f32) - 0.5) * 0.2
            })
            .collect()
    }

    #[test]
    fn keeps_length_and_roundtrips_silence() {
        let mut d = Denoiser::new();
        let silence = vec![0f32; 16_000];
        let out = d.process(&silence);
        assert_eq!(out.len(), silence.len());
        assert!(rms(&out) < 0.01);
    }

    #[test]
    fn attenuates_pure_noise() {
        let mut d = Denoiser::new();
        let n = noise(16_000 * 2);
        let before = rms(&n);
        let after = rms(&d.process(&n));
        // RNNoise 在纯噪声上应该压得比输入明显更低
        assert!(after < before * 0.8, "before={before} after={after}");
    }
}

/// GTCRN 流式降噪（sherpa-onnx `OnlineSpeechDenoiser`，16kHz 原生）。
pub struct Gtcrn {
    d: sherpa_onnx::OnlineSpeechDenoiser,
}

/// 模型放在 `<config_dir>/models/gtcrn_simple.onnx`（由 `models.rs` 下载）。
pub const GTCRN_FILE: &str = "gtcrn_simple.onnx";

pub fn gtcrn_path(config_dir: &std::path::Path) -> std::path::PathBuf {
    config_dir.join("models").join(GTCRN_FILE)
}

impl Gtcrn {
    pub fn load(model: &std::path::Path) -> Option<Self> {
        use sherpa_onnx::{
            OfflineSpeechDenoiserGtcrnModelConfig, OfflineSpeechDenoiserModelConfig,
            OnlineSpeechDenoiser, OnlineSpeechDenoiserConfig,
        };
        let cfg = OnlineSpeechDenoiserConfig {
            model: OfflineSpeechDenoiserModelConfig {
                gtcrn: OfflineSpeechDenoiserGtcrnModelConfig {
                    model: Some(model.display().to_string()),
                },
                ..Default::default()
            },
        };
        OnlineSpeechDenoiser::create(&cfg).map(|d| Self { d })
    }

    /// 吃任意长度的 16k 单声道，返回同样长度的降噪结果（内部保留流式状态）
    pub fn run(&self, samples: &[f32]) -> Vec<f32> {
        if samples.is_empty() {
            return Vec::new();
        }
        self.d.run(samples, 16_000).samples
    }
}
