//! 离线 A/B：给干净语音加白噪声，再用 sherpa-onnx 的流式降噪模型（GTCRN / DPDFNet）处理。
//!
//! `cargo run --release --example denoise_ab -- <gtcrn.onnx> <wav16> <噪声 dB>`
//! 产出 `<wav>-noisy.wav` 与 `<wav>-denoised.wav`，再各自走 `Eryu --decode-wav` 比 CER。

use eryu_core::audio;
use sherpa_onnx::{
    OfflineSpeechDenoiserGtcrnModelConfig, OfflineSpeechDenoiserModelConfig, OnlineSpeechDenoiser,
    OnlineSpeechDenoiserConfig,
};
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("用法: denoise_ab <model.onnx> <wav16> <noise_db>");
        std::process::exit(2);
    }
    let model = &args[1];
    let wav = PathBuf::from(&args[2]);
    let db: f32 = args[3].parse().unwrap_or(0.0);

    let clean = audio::read_wav16(&wav).expect("读 wav");
    let rms = (clean.iter().map(|s| s * s).sum::<f32>() / clean.len() as f32)
        .sqrt()
        .max(1e-9);
    let noise_rms = rms * 10f32.powf(db / 20.0);
    let mut seed: u32 = 0x1234_5678;
    let mut noisy: Vec<f32> = clean
        .iter()
        .map(|s| {
            // xorshift → 近似均匀白噪声，转成单位方差再按目标 RMS 缩放
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let u = (seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
            (s + u * noise_rms * 1.732).clamp(-1.0, 1.0)
        })
        .collect();

    let noisy_path = with_suffix(&wav, "-noisy.wav");
    audio::write_wav16(&noisy_path, &noisy).expect("写 noisy");

    let cfg = OnlineSpeechDenoiserConfig {
        model: OfflineSpeechDenoiserModelConfig {
            gtcrn: OfflineSpeechDenoiserGtcrnModelConfig {
                model: Some(model.to_string()),
            },
            ..Default::default()
        },
    };
    let Some(denoiser) = OnlineSpeechDenoiser::create(&cfg) else {
        eprintln!("降噪模型加载失败: {model}");
        std::process::exit(1);
    };
    let mut out: Vec<f32> = Vec::with_capacity(noisy.len());
    for chunk in noisy.chunks(1600) {
        let d = denoiser.run(chunk, audio::TARGET_RATE as i32);
        out.extend_from_slice(&d.samples);
    }
    let tail = denoiser.flush();
    out.extend_from_slice(&tail.samples);

    let dn_path = with_suffix(&wav, "-denoised.wav");
    audio::write_wav16(&dn_path, &out).expect("写 denoised");
    let peak = noisy.iter().fold(0f32, |a, s| a.max(s.abs()));
    println!(
        "AB 干净 {} 样本；噪声 {db}dB（峰值 {:.2}）→ {} / {}",
        clean.len(),
        peak,
        noisy_path.display(),
        dn_path.display()
    );
    let _ = Path::new(model);
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    p.with_file_name(format!("{stem}{suffix}"))
}
