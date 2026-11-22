//! 「音源锁定」（DESIGN §9.7 的原创实验特性）：不用注册声纹，直接锁住**这次录音里最先说话的那个人**。
//!
//! 规则（D-065）：
//! 1. 前 600ms 只建基线，不做判断；
//! 2. 第一段足够长的语音（≥0.6s）算出 embedding → 成为目标声纹（锁定）；
//! 3. 之后每段跟目标比：相似度 ≥ 阈值就留下，否则丢掉（记为「不是开头那个人」）；
//! 4. 如果连续丢掉的音频累计超过 3s，说明「开头那个人可能一直没说话」或声纹不靠谱 →
//!    解锁、改用新的目标（宁可放宽，也不要把用户自己的话全吞掉）。
//!
//! 与 `voicelock`（只录我注册的声音）互补：这个是零配置的临时版，适合「旁边有人插话」的场景。

use std::path::Path;
use std::time::Duration;

/// 一段至少这么长才算「够长的语音」，能用来建基线
pub const MIN_SEG_SECS: f32 = 0.6;
/// 连续丢掉这么多音频后解锁（避免把用户自己的声音全滤掉）。
/// 注意：单个长段（比如别人说了一整句 8s）**不该**因为「够长」就解锁——那样永远滤不掉长插话。
/// 所以解锁要求「累计 ≥8s **且** 至少丢了 2 段」（D-065）。
pub const UNLOCK_AFTER: Duration = Duration::from_secs(8);
pub const UNLOCK_MIN_SEGMENTS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// 这一段建立了目标声纹
    Locked,
    /// 像开头那个人
    Keep,
    /// 不像开头那个人（会被丢掉）
    Drop,
}

pub struct Lock {
    target: Option<Vec<f32>>,
    dropped_secs: f32,
    pub kept: usize,
    pub dropped: usize,
    pub locked_at: Option<usize>,
}

impl Default for Lock {
    fn default() -> Self {
        Self::new()
    }
}

impl Lock {
    pub fn new() -> Self {
        Self {
            target: None,
            dropped_secs: 0.0,
            kept: 0,
            dropped: 0,
            locked_at: None,
        }
    }

    pub fn target_ready(&self) -> bool {
        self.target.is_some()
    }

    /// 判定一段音频；`index` 只用于记录「第几段锁定的」
    pub fn check(&mut self, pcm: &[f32], model: &Path, threshold: f32, index: usize) -> Decision {
        let secs = pcm.len() as f32 / 16_000.0;
        if secs < MIN_SEG_SECS {
            // 太短算不出可靠声纹：锁定前跳过，锁定后放行（短段通常是「嗯」「对」这类应答）
            return if self.target.is_some() {
                Decision::Keep
            } else {
                Decision::Drop
            };
        }
        let Some(vec) = crate::voicelock::embed(model, pcm) else {
            return Decision::Keep; // 算不出就放行，绝不吞话
        };
        match &self.target {
            None => {
                self.target = Some(vec);
                self.locked_at = Some(index);
                self.kept += 1;
                Decision::Locked
            }
            Some(t) => {
                let sim = crate::voicelock::cosine(t, &vec);
                if sim >= threshold {
                    self.kept += 1;
                    self.dropped_secs = 0.0;
                    Decision::Keep
                } else if self.dropped_secs + secs >= UNLOCK_AFTER.as_secs_f32()
                    && self.dropped + 1 >= UNLOCK_MIN_SEGMENTS
                {
                    // 连着 3s 都不像：换目标（也许开头那段是别人插话，或者一开始就锁错了）
                    eprintln!(
                        "[eryu][sourcelock] 连续 {:.1}s 不像目标声纹，解锁并改用这一段",
                        self.dropped_secs + secs
                    );
                    self.target = Some(vec);
                    self.locked_at = Some(index);
                    self.dropped_secs = 0.0;
                    Decision::Locked
                } else {
                    self.dropped_secs += secs;
                    self.dropped += 1;
                    Decision::Drop
                }
            }
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "锁定第 {} 段；留下 {} 段、跳过 {} 段",
            self.locked_at.map(|i| i + 1).unwrap_or(0),
            self.kept,
            self.dropped
        )
    }
}
