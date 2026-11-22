//! 采音（DESIGN §4.5 / 审计 P1：按设备原生格式开流 → 下游混单声道 → 有状态重采样）
//!
//! 音频回调只做三件事：混单声道 → 有状态重采样 → try_send 到有界通道。
//! 不编码 JSON、不持锁、不打印、不分配大内存。

use std::sync::atomic::{AtomicU64, Ordering};

thread_local! {
    /// 采集侧降噪开关（由 start_with 在开流前设置；cpal 回调里读不到 Config，故用线程局部传递）
    pub static DENOISE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use sherpa_onnx::LinearResampler;

pub const TARGET_RATE: u32 = 16_000;

/// 低于这个峰值就当作「没收到声音」（不是「声音小」）。
/// 经验值：正常说话的 16k 单声道峰值在 0.05–0.6；0.003 以下基本是底噪。
pub const SILENCE_PEAK: f32 = 0.01;

/// 峰值幅度，用来区分「没声音」和「声音小」
pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
}
/// 会话上限（DESIGN §4.5）
pub const MAX_SECONDS: usize = 120;
/// 有界通道：256 个回调块，满了就丢并计数（绝不阻塞音频线程）
const CHANNEL_CHUNKS: usize = 256;

pub struct Capture {
    stream: cpal::Stream,
    pub rx: Receiver<Vec<f32>>,
    pub dropped_samples: Arc<AtomicU64>,
    /// 流错误（多半是设备被拔掉/被别的应用独占）；worker 看到就提前收尾
    pub failed: Arc<AtomicU64>,
    pub device_name: String,
    pub input_rate: u32,
    pub channels: u16,
}

// cpal 的 Stream 在 macOS 上不算 Send（内部持有 AudioUnit 回调句柄）。
// Eryu 的用法：Capture 只在 worker 线程创建、排空、drop，绝不跨线程共享，
// 因此这里的 unsafe impl 是安全的（仅用于把 Worker 整体 move 进线程）。
unsafe impl Send for Capture {}

impl Capture {
    /// 把已到达的音频块全部取走（不阻塞）
    pub fn drain(&self, into: &mut Vec<f32>) {
        while let Ok(chunk) = self.rx.try_recv() {
            into.extend_from_slice(&chunk);
        }
    }

    /// 流是否报过错（设备拔掉等）
    pub fn has_failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed) > 0
    }

    pub fn stopped(self) {}

    /// 松键时收尾：先宽限收齐在途的音频块，再停流取最后一批。
    /// 顺序反了（先 drop 流再收）会丢掉还没回调上来的那一小段——长句的最后几个字常在这里。
    pub fn finish(self, into: &mut Vec<f32>) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(120);
        loop {
            let mut got = false;
            while let Ok(chunk) = self.rx.try_recv() {
                into.extend_from_slice(&chunk);
                got = true;
            }
            if !got {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(4));
            }
        }
        drop(self.stream);
        while let Ok(chunk) = self.rx.try_recv() {
            into.extend_from_slice(&chunk);
        }
    }
}

/// 打开系统默认输入设备
pub fn start() -> Result<Capture, String> {
    start_with(None)
}

/// 按名字打开输入设备（cfg.audio.device 非空时用）；找不到就报错，不静默换默认设备
pub fn start_with(device_name: Option<&str>) -> Result<Capture, String> {
    start_with_opts(device_name, None)
}

/// 开流；`denoise` 是 `<config>/models/gtcrn_simple.onnx` 的路径，给了就在 16k 上做流式降噪（D-063）
pub fn start_with_opts(
    device_name: Option<&str>,
    denoise: Option<std::path::PathBuf>,
) -> Result<Capture, String> {
    DENOISE.with(|d| d.set(denoise.is_some()));
    let host = cpal::default_host();
    let device = match device_name.filter(|s| !s.trim().is_empty()) {
        Some(want) => {
            let devices = host
                .input_devices()
                .map_err(|e| format!("枚举输入设备失败：{e}"))?;
            let names: Vec<String> = devices.filter_map(|d| d.name().ok()).collect();
            names
                .iter()
                .find(|n| n.as_str() == want)
                .map(|n| {
                    host.input_devices()
                        .ok()
                        .and_then(|mut it| {
                            it.find(|d| d.name().map(|x| &x == n).unwrap_or(false))
                        })
                        .unwrap_or_else(|| {
                            // 兜底：拿不到枚举也没关系，下面 default_input_config 会报真实错误
                            host.default_input_device().expect("无可用输入设备")
                        })
                })
                .ok_or_else(|| {
                    let list = if names.is_empty() {
                        "（当前没有任何输入设备）".to_string()
                    } else {
                        names.join(" / ")
                    };
                    // 浮层只显示开头那句（fit_head），完整文案（含可用设备）给日志/托盘/命令行，
                    // 用户照着「改回系统默认」就能自救；scripts/matrix.sh 也断言这里要列出可用设备。
                    format!(
                        "找不到输入设备「{want}」：可能已断开。可用设备：{list}。在设置 → 音频 里改回系统默认即可"
                    )
                })?
        }
        None => host
            .default_input_device()
            .ok_or_else(|| "没有可用的麦克风（检查系统设置 → 声音 → 输入）".to_string())?,
    };
    let device_name = device.name().unwrap_or_else(|_| "未知输入设备".into());
    let supported = device
        .default_input_config()
        .map_err(|e| format!("读取麦克风默认格式失败：{e}"))?;
    let sample_format = supported.sample_format();
    let input_rate = supported.sample_rate().0;
    let channels = supported.channels();
    let config: cpal::StreamConfig = supported.into();

    let (tx, rx) = sync_channel::<Vec<f32>>(CHANNEL_CHUNKS);
    let dropped = Arc::new(AtomicU64::new(0));
    let failed = Arc::new(AtomicU64::new(0));
    let resampler = LinearResampler::create(input_rate as i32, TARGET_RATE as i32)
        .ok_or_else(|| "重采样器创建失败".to_string())?;

    let stream = match sample_format {
        SampleFormat::F32 => build::<f32>(
            &device,
            &config,
            tx,
            dropped.clone(),
            failed.clone(),
            resampler,
            channels,
            input_rate,
            denoise,
            |x| x,
        ),
        SampleFormat::I16 => build::<i16>(
            &device,
            &config,
            tx,
            dropped.clone(),
            failed.clone(),
            resampler,
            channels,
            input_rate,
            denoise,
            |x| x as f32 / 32768.0,
        ),
        SampleFormat::U16 => build::<u16>(
            &device,
            &config,
            tx,
            dropped.clone(),
            failed.clone(),
            resampler,
            channels,
            input_rate,
            denoise,
            |x| (x as f32 - 32768.0) / 32768.0,
        ),
        other => return Err(format!("不支持的麦克风采样格式：{other:?}")),
    }?;

    stream.play().map_err(|e| format!("启动麦克风失败：{e}"))?;

    Ok(Capture {
        stream,
        rx,
        dropped_samples: dropped,
        failed,
        device_name,
        input_rate,
        channels,
    })
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: SyncSender<Vec<f32>>,
    dropped: Arc<AtomicU64>,
    failed: Arc<AtomicU64>,
    resampler: LinearResampler,
    channels: u16,
    input_rate: u32,
    denoise: Option<std::path::PathBuf>,
    to_f32: fn(T) -> f32,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + 'static,
{
    let ch = channels.max(1) as usize;
    // 降噪在重采样之后、16k 上做（D-063）：GTCRN 原生 16k，任何输入采样率都统一
    let mut dn = denoise.and_then(|p| {
        let d = crate::denoise::Gtcrn::load(&p);
        if d.is_none() {
            eprintln!(
                "[eryu][audio] 降噪模型加载失败：{}（本次不降噪）",
                p.display()
            );
        }
        d
    });
    let _ = input_rate;
    device
        .build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _| {
                // 1) 混单声道
                let mut mono = Vec::with_capacity(data.len() / ch + 1);
                for frame in data.chunks(ch) {
                    let mut sum = 0.0f32;
                    for s in frame {
                        sum += to_f32(*s);
                    }
                    mono.push(sum / frame.len() as f32);
                }
                if mono.is_empty() {
                    return;
                }
                // 2) 有状态重采样到 16k
                let out = resampler.resample(&mono, false);
                if out.is_empty() {
                    return;
                }
                // 3) （可选）16k 流式降噪
                let out = if let Some(d) = dn.as_ref() {
                    d.run(&out)
                } else {
                    out
                };
                if out.is_empty() {
                    return;
                }
                // 4) 有界入队，满则丢并计数
                match tx.try_send(out) {
                    Ok(()) => {}
                    Err(TrySendError::Full(chunk)) => {
                        dropped.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                    }
                    Err(TrySendError::Disconnected(_)) => {}
                }
            },
            move |err| {
                // 设备被拔掉/切换/独占时走到这里：记下标志，worker 会提前收尾并保住已录部分
                failed.fetch_add(1, Ordering::Relaxed);
                eprintln!("[eryu][audio] 流错误: {err}");
            },
            None,
        )
        .map_err(|e| format!("打开麦克风流失败：{e}"))
}

/// 环形裁剪：只保留最近 MAX_SECONDS
pub fn cap_len(samples: &mut Vec<f32>) {
    cap_len_to(samples, MAX_SECONDS as u32);
}

/// 按「用户配置的上限」裁剪（DESIGN §4.5：上限是会话预算，不是模型限制）
pub fn cap_len_to(samples: &mut Vec<f32>, max_seconds: u32) {
    // 保留「前」120s：录音超时后用户最想留下的是开头那段话，
    // 丢掉开头（以前的做法）会让长听写的开头整段消失，也让「到点提示」自相矛盾。
    let max = (max_seconds.max(1) as usize) * TARGET_RATE as usize;
    if samples.len() > max {
        samples.truncate(max);
    }
}

/// 当前长度（秒）
pub fn secs_of(samples: &[f32]) -> f32 {
    samples.len() as f32 / TARGET_RATE as f32
}

/// 把 16k 单声道 f32 写成 16bit PCM WAV（失败音频暂存用）
pub fn write_wav16(path: &std::path::Path, samples: &[f32]) -> Result<(), String> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("建目录失败：{e}"))?;
    }
    let data_len = (samples.len() * 2) as u32;
    let mut f = std::fs::File::create(path).map_err(|e| format!("写 WAV 失败：{e}"))?;
    let hdr = |f: &mut std::fs::File| -> std::io::Result<()> {
        f.write_all(b"RIFF")?;
        f.write_all(&(36 + data_len).to_le_bytes())?;
        f.write_all(b"WAVEfmt ")?;
        f.write_all(&16u32.to_le_bytes())?;
        f.write_all(&1u16.to_le_bytes())?; // PCM
        f.write_all(&1u16.to_le_bytes())?; // mono
        f.write_all(&TARGET_RATE.to_le_bytes())?;
        f.write_all(&(TARGET_RATE * 2).to_le_bytes())?; // byte rate
        f.write_all(&2u16.to_le_bytes())?; // block align
        f.write_all(&16u16.to_le_bytes())?; // bits
        f.write_all(b"data")?;
        f.write_all(&data_len.to_le_bytes())?;
        Ok(())
    };
    hdr(&mut f).map_err(|e| format!("写 WAV 失败：{e}"))?;
    let mut buf = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        buf.extend_from_slice(&v.to_le_bytes());
    }
    f.write_all(&buf).map_err(|e| format!("写 WAV 失败：{e}"))?;
    crate::fsutil::restrict(path);
    Ok(())
}

/// 读 16k 单声道 WAV（暂存的重试用）；其它采样率拒绝，避免悄悄用错采样率
pub fn read_wav16(path: &std::path::Path) -> Result<Vec<f32>, String> {
    let w = sherpa_onnx::Wave::read(&path.to_string_lossy())
        .ok_or_else(|| format!("读取 WAV 失败：{}", path.display()))?;
    if w.sample_rate() as u32 != TARGET_RATE {
        return Err(format!("暂存音频采样率 {} 不是 16000", w.sample_rate()));
    }
    Ok(w.samples().to_vec())
}

/// 可用输入设备名（设置页下拉 + 诊断用）
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let mut names: Vec<String> = host
        .input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    names.sort();
    names.dedup();
    names
}

/// 默认输入设备名（诊断用）
pub fn default_input_name() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .and_then(|d| d.name().ok())
}

#[cfg(test)]
mod cap_tests {
    #[test]
    fn cap_len_to_honours_configured_limit() {
        // 3 秒预算
        let mut v = vec![0f32; 16_000 * 5];
        super::cap_len_to(&mut v, 3);
        assert_eq!(v.len(), 16_000 * 3);
        // 预算比现有数据长：不动
        let mut w = vec![0f32; 16_000];
        super::cap_len_to(&mut w, 30);
        assert_eq!(w.len(), 16_000);
    }
}

/// 在 `end` 之前 1.2s～0.3s 的窗口里找能量最低的 20ms 帧，返回切点（样本下标）。
/// 渐进解码切块时用：切在安静的缝里，比按固定长度硬切少截断词。
pub fn quiet_cut(samples: &[f32], end: usize) -> Option<usize> {
    const FRAME: usize = 320; // 20ms @16k
    let end = end.min(samples.len());
    let lo = end.saturating_sub(8 * TARGET_RATE as usize / 10); // end-0.8s
    let hi = end.saturating_sub(25 * TARGET_RATE as usize / 10); // end-2.5s
    if end < 3 * TARGET_RATE as usize || hi >= lo {
        return None;
    }
    let (mut best, mut best_energy) = (None, f32::MAX);
    let mut total = 0f32;
    let mut frames = 0usize;
    let mut i = hi;
    while i + FRAME <= lo {
        let energy: f32 = samples[i..i + FRAME].iter().map(|s| s * s).sum();
        total += energy;
        frames += 1;
        if energy < best_energy {
            best_energy = energy;
            best = Some(i + FRAME / 2);
        }
        i += FRAME;
    }
    // 只认「明显比这块平均安静」的缝（≈ -10dB 以下）；连续大声说话时宁可不切，
    // 让调用方再等一会儿，而不是切在字中间。
    let mean = if frames > 0 {
        total / frames as f32
    } else {
        0.0
    };
    match best {
        Some(pos) if best_energy <= mean * 0.25 => Some(pos),
        _ => None,
    }
}

#[cfg(test)]
mod cut_tests {
    use super::*;

    #[test]
    fn quiet_cut_prefers_low_energy_gap() {
        // 4s 全响 + 3.0–3.4s 留静音：应切在静音里
        let n = 4 * 16000;
        let mut pcm = vec![0.5f32; n];
        for s in pcm.iter_mut().skip(3 * 16000).take(6400) {
            *s = 0.0;
        }
        let cut = quiet_cut(&pcm, n).expect("应找到静音缝");
        assert!(cut > 3 * 16000 && cut < 3 * 16000 + 6400, "cut={cut}");
    }

    #[test]
    fn quiet_cut_gives_up_when_continuous() {
        // 一直大声说话：没有明显安静缝，宁可不切
        let pcm = vec![0.3f32; 8 * 16000];
        assert_eq!(quiet_cut(&pcm, pcm.len()), None);
    }

    #[test]
    fn quiet_cut_none_when_too_short() {
        let pcm = vec![0.1f32; 16000];
        assert_eq!(quiet_cut(&pcm, pcm.len()), None);
    }
}

/// 自动增益：麦克风音量很小（离得远、系统输入音量低）时把整段提上来，ASR 才不容易出错。
/// 只在小信号上动手（peak < 0.08），最大 30 倍，避免把正常录音抬爆。
pub fn auto_gain(samples: &[f32]) -> Vec<f32> {
    let peak = peak(samples);
    if peak <= 1e-6 || peak >= 0.08 {
        return samples.to_vec();
    }
    let gain = (0.8 / peak).min(30.0);
    samples
        .iter()
        .map(|s| (s * gain).clamp(-1.0, 1.0))
        .collect()
}

/// 真正「像人对着麦克风说话」的最低峰值：低于它多半是环境底噪/耳机漏音/扬声器回声。
/// 实测（D-072）：安静房间底噪 0.011、扬声器外放被拾到 0.015、正常近场说话 0.1–0.6。
pub const SPEECH_MIN_PEAK: f32 = 0.02;

/// 「这段里到底有没有人说话」：20ms 一帧算 RMS，拿最响的一帧和底噪比。
/// 静音/被静音的麦克风：最响帧 ≈ 底噪，判为没说话；小声说话：最响帧远高于底噪，判为说话。
/// 阈值（D-067）：最响帧 ≥ 0.0005（≈ −66dBFS 的 RMS，几乎听不见的轻声也能过）
/// 且（≥ 底噪 3 倍 或 ≥ 0.05）。
pub fn looks_like_speech(samples: &[f32]) -> bool {
    const FRAME: usize = 320; // 20ms @ 16k
    if samples.len() < FRAME {
        return false;
    }
    let mut rms: Vec<f32> = samples
        .chunks(FRAME)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    rms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let loud = *rms.last().unwrap_or(&0.0);
    let floor = rms[rms.len() / 5]; // 第 20 百分位当底噪
    loud >= 0.0005 && (loud >= floor * 3.0 || loud >= 0.05)
}

#[cfg(test)]
mod gain_tests {
    use super::*;

    fn quiet_speech(scale: f32) -> Vec<f32> {
        // 1k 正弦 + 停顿，模拟「有人在说话」的能量包络
        let mut v = Vec::new();
        for block in 0..30 {
            for i in 0..320 {
                let t = (block * 320 + i) as f32 / 16_000.0;
                let amp = if block % 10 < 6 { 0.3 * scale } else { 0.001 };
                v.push(amp * (2.0 * std::f32::consts::PI * 1000.0 * t).sin());
            }
        }
        v
    }

    #[test]
    fn auto_gain_lifts_quiet_speech_but_keeps_loud_untouched() {
        let quiet = quiet_speech(0.01);
        let lifted = auto_gain(&quiet);
        assert!(peak(&lifted) > peak(&quiet) * 5.0, "小声要被提上来");
        let loud = quiet_speech(0.6);
        assert_eq!(auto_gain(&loud), loud, "正常音量不该被动");
    }

    #[test]
    fn looks_like_speech_rejects_flat_noise() {
        // 被静音的麦克风：均匀小噪声，没有语音包络
        let mut seed: u32 = 12345;
        let noise: Vec<f32> = (0..8000)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                0.0005 * (seed as f32 / u32::MAX as f32 * 2.0 - 1.0)
            })
            .collect();
        assert!(!looks_like_speech(&noise));
        assert!(!looks_like_speech(&vec![0.0f32; 8000]));
        assert!(looks_like_speech(&quiet_speech(0.01)), "轻声说话要能认出来");
    }
}
