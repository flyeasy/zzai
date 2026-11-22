//! 长听写流水线（DESIGN §4.4）
//!
//! 给定整段 16k 单声道 PCM：VAD 切段（每段 ≤30s）→ 逐段识别 → 拼接。
//! 首版在松开热键后一次性跑完（51s 音频 VAD+识别实测 ~700ms）；
//! 「边录边解」是 M1a 的后续优化，接口不变。

use crate::asr::AsrEngine;
use crate::vad::Segmenter;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone)]
pub struct SegmentInfo {
    pub index: usize,
    pub samples: usize,
    pub text: String,
    pub error: Option<String>,
    /// 识别失败时保留这一段音频，供「只重试这一段」用（成功段不占内存）
    pub audio: Option<Vec<f32>>,
}

#[derive(Debug, Clone, Default)]
pub struct PipelineResult {
    pub text: String,
    pub segments: Vec<SegmentInfo>,
    /// 识别失败的段数（>0 时 text 只含成功部分，UI 要提示「部分成功」）
    pub failed: usize,
    /// 被「只录我的声音」丢掉的段数（这些段没送识别）
    pub skipped: usize,
}

/// VAD 可用时按语音段切；VAD 不可用时按固定窗口切（仍保证 ≤30s）
pub fn run(
    engine: &mut dyn AsrEngine,
    segmenter: Option<&Segmenter>,
    pcm: &[f32],
    sample_rate: u32,
) -> Result<PipelineResult, String> {
    run_cancellable(engine, segmenter, pcm, sample_rate, None)
}

/// 段过滤（声纹门控）：返回 false 的段直接跳过、不送识别
pub type SegmentFilter<'a> = &'a dyn Fn(&[f32]) -> bool;

/// 带取消开关的版本：准流式预览用（松手后立刻叫停，别占着引擎让最终识别排队）
pub fn run_cancellable(
    engine: &mut dyn AsrEngine,
    segmenter: Option<&Segmenter>,
    pcm: &[f32],
    sample_rate: u32,
    cancel: Option<&AtomicBool>,
) -> Result<PipelineResult, String> {
    run_filtered(engine, segmenter, pcm, sample_rate, cancel, None)
}

/// 带段过滤的版本（「只录我的声音」用）：被过滤掉的段不识别、也不计入失败
pub fn run_filtered(
    engine: &mut dyn AsrEngine,
    segmenter: Option<&Segmenter>,
    pcm: &[f32],
    sample_rate: u32,
    cancel: Option<&AtomicBool>,
    filter: Option<SegmentFilter>,
) -> Result<PipelineResult, String> {
    if pcm.is_empty() {
        return Err("没有录到音频".to_string());
    }
    let chunks: Vec<Vec<f32>> = match segmenter {
        Some(seg) => {
            let mut out = seg.accept(pcm);
            out.extend(seg.flush());
            if out.is_empty() {
                // 整段没检测到语音：还是送一次，避免把轻声说话误判为静音
                vec![pcm.to_vec()]
            } else {
                out
            }
        }
        None => fixed_windows(pcm, sample_rate),
    };

    let mut result = PipelineResult::default();
    let mut parts: Vec<String> = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        if let Some(c) = cancel {
            if c.load(Ordering::Relaxed) {
                return Err("cancelled".to_string());
            }
        }
        if let Some(f) = filter {
            if !f(chunk) {
                result.skipped += 1;
                continue;
            }
        }
        match engine.transcribe(chunk, sample_rate) {
            Ok(text) => {
                if !text.is_empty() {
                    parts.push(text.clone());
                }
                result.segments.push(SegmentInfo {
                    index: i,
                    samples: chunk.len(),
                    text,
                    error: None,
                    audio: None,
                });
            }
            Err(e) => {
                result.failed += 1;
                result.segments.push(SegmentInfo {
                    index: i,
                    samples: chunk.len(),
                    // 失败段留一份音频：用户可以对单段重试（DESIGN §10.2 部分成功保留）
                    audio: Some(chunk.clone()),
                    text: String::new(),
                    error: Some(e),
                });
            }
        }
    }
    // 首版拼接策略：不动字（审计：禁止最长公共子串删字），段间用空格/标点自然衔接
    result.text = join_segments(&parts);
    Ok(result)
}

/// 段间拼接：中文之间不加空格，中英边界加一个空格
/// 句末标点兜底：中文听写没标点时补一个句号。
/// 只处理「以 CJK 结尾且长度 ≥6」的文本——代码、命令、单个词不动，避免把 `int main` 变成 `int main。`
pub fn add_final_punct(text: &str) -> String {
    let t = text.trim_end();
    if t.chars().count() < 6 {
        return t.to_string();
    }
    let Some(last) = t.chars().last() else {
        return String::new();
    };
    if matches!(
        last,
        '。' | '！' | '？' | '!' | '?' | '.' | ',' | '，' | '…' | '；' | ';' | '：' | ':'
    ) {
        return t.to_string();
    }
    // 结尾必须是中日韩文字（字母/数字/右括号结尾的情况保持原样）
    let is_cjk = ('\u{4e00}'..='\u{9fff}').contains(&last)
        || ('\u{3400}'..='\u{4dbf}').contains(&last)
        || matches!(last, '了' | '的' | '吗' | '呢' | '吧');
    if is_cjk {
        let mut out = t.to_string();
        out.push('。');
        out
    } else {
        t.to_string()
    }
}

pub fn join_segments(parts: &[String]) -> String {
    let mut out = String::new();
    for p in parts {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        if out.is_empty() {
            out.push_str(p);
            continue;
        }
        let prev = out.chars().last().unwrap();
        let next = p.chars().next().unwrap();
        // 段间加空格：两侧都是 ASCII 字符时补一个空格（含句末标点的情况，如 "by myself." + "Je suis"）
        let ascii_prev = prev.is_ascii();
        let ascii_next = next.is_ascii();
        let no_space_needed =
            matches!(prev, ',' | '.' | '!' | '?' | ';' | ':') && next.is_ascii_uppercase() == false;
        if ascii_prev && ascii_next && !no_space_needed {
            out.push(' ');
        }
        out.push_str(p);
    }
    out
}

/// 拼接有重叠的两块文本：去掉重叠区里重复的字（渐进解码切块时前后各留 300ms 音频，
/// 接缝处模型会把同一句话识别两遍）。只在末尾 12 个字以内找重叠，避免误删真实重复。
pub fn join_overlap(prev: &str, next: &str) -> String {
    let prev_t = prev.trim();
    let next_t = next.trim();
    if prev_t.is_empty() {
        return next_t.to_string();
    }
    if next_t.is_empty() {
        return prev_t.to_string();
    }
    let a: Vec<char> = prev_t.chars().collect();
    let b: Vec<char> = next_t.chars().collect();
    let max_k = 12.min(a.len()).min(b.len());
    let mut k = 0;
    for kk in (2..=max_k).rev() {
        if a[a.len() - kk..] == b[..kk] {
            k = kk;
            break;
        }
    }
    // 重叠只认「尾巴就是开头」；不同标点（。 / ，）不算重复，交给标点模型
    if k > 0 {
        let rest: String = b[k..].iter().collect();
        return format!("{}{}", prev_t, rest);
    }
    join_segments(&[prev_t.to_string(), next_t.to_string()])
}

#[cfg(test)]
mod overlap_tests {
    use super::join_overlap;

    #[test]
    fn drops_duplicated_seam() {
        // 模型把接缝那句识别了两遍时，重复部分只留一份
        let a = "第一条说的是会议室和投影设备";
        let b = "投影设备。另外请大家提前准备材料。";
        assert_eq!(
            join_overlap(a, b),
            "第一条说的是会议室和投影设备。另外请大家提前准备材料。"
        );
    }

    #[test]
    fn keeps_real_repetition() {
        // 「谢谢谢谢」这种真的说了两遍的，不能被当成接缝去重
        let a = "大家辛苦了谢谢谢谢";
        let b = "谢谢大家";
        assert!(join_overlap(a, b).contains("谢谢谢谢"));
    }

    #[test]
    fn no_overlap_falls_back_to_join() {
        assert_eq!(join_overlap("abc", "def"), "abc def"); // 两侧都是 ASCII 时沿用 join_segments 的空格规则
    }
}

fn fixed_windows(pcm: &[f32], sample_rate: u32) -> Vec<Vec<f32>> {
    let window = (sample_rate as usize * 25).max(1); // 25s，留 5s 余量
    pcm.chunks(window).map(|c| c.to_vec()).collect()
}

#[cfg(test)]
mod tests {
    use crate::asr::AsrEngine;

    /// 永远失败的引擎：只在「失败段保留音频」测试里用
    struct FailingEngine;
    impl AsrEngine for FailingEngine {
        fn name(&self) -> &str {
            "failing"
        }
        fn transcribe(&mut self, _pcm: &[f32], _sr: u32) -> Result<String, String> {
            Err("模型炸了".to_string())
        }
    }

    #[test]
    fn failed_segment_keeps_audio_for_retry() {
        // 无 VAD：固定窗口切，2.5s 音频 → 一段；引擎失败 → 该段应带音频样本
        let pcm = vec![0.1f32; 40_000];
        let mut engine = FailingEngine;
        let r = super::run(&mut engine, None, &pcm, 16000).unwrap();
        assert_eq!(r.failed, 1);
        assert_eq!(r.segments.len(), 1);
        let seg = &r.segments[0];
        assert!(seg.error.is_some());
        let audio = seg.audio.as_ref().expect("失败段必须保留音频");
        assert_eq!(audio.len(), seg.samples, "保留的样本数应与段长一致");
    }

    #[test]
    fn joins_ascii_segments_with_space_after_period() {
        let out = super::join_segments(&["I'm alone.".into(), "Je suis seul.".into()]);
        assert_eq!(out, "I'm alone. Je suis seul.");
        // 中文段之间不加空格
        let zh = super::join_segments(&["开放时间".into(), "早上九点".into()]);
        assert_eq!(zh, "开放时间早上九点");
        // ASCII 之间仍补空格
        let en = super::join_segments(&["boy".into(), "and".into()]);
        assert_eq!(en, "boy and");
    }

    use super::*;
    use crate::asr::StubEngine;

    #[test]
    fn joins_chinese_without_space_and_english_with_space() {
        let parts = vec![
            "今天我们来".to_string(),
            "聊一下 Rust".to_string(),
            "Rust 很好用".to_string(),
        ];
        assert_eq!(join_segments(&parts), "今天我们来聊一下 Rust Rust 很好用");
    }

    #[test]
    fn final_punct_only_for_cjk_sentences() {
        assert_eq!(
            super::add_final_punct("我们明天下午三点开会"),
            "我们明天下午三点开会。"
        );
        assert_eq!(super::add_final_punct("我们三点开会。"), "我们三点开会。");
        assert_eq!(super::add_final_punct("int main"), "int main");
        assert_eq!(super::add_final_punct("好的"), "好的");
        assert_eq!(super::add_final_punct("check the log"), "check the log");
    }

    #[test]
    fn stub_pipeline_returns_text() {
        let pcm = vec![0.01f32; 16_000];
        let mut engine = StubEngine {
            reply: "你好".into(),
        };
        let r = run(&mut engine, None, &pcm, 16_000).unwrap();
        assert_eq!(r.text, "你好");
        assert_eq!(r.failed, 0);
    }
}
