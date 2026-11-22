//! 采集侧降噪的离线验证：48kHz WAV → RNNoise 逐帧 → 16kHz WAV
//!
//! 用法：cargo run --release --example denoise48 -- in48.wav out16.wav

use eryu_core::denoise::Denoiser;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("用法：denoise48 <48k 输入.wav> <16k 输出.wav>");
        std::process::exit(2);
    }
    let wave = match sherpa_onnx::Wave::read(&args[1]) {
        Some(w) => w,
        None => {
            eprintln!("读不到 {}", args[1]);
            std::process::exit(1);
        }
    };
    if wave.sample_rate() != 48000 {
        eprintln!("需要 48kHz 输入，当前 {}", wave.sample_rate());
        std::process::exit(1);
    }
    let samples: Vec<f32> = wave.samples().to_vec();
    let mut d = Denoiser::new();
    let mut clean = Vec::with_capacity(samples.len());
    for frame in samples.chunks(480) {
        clean.extend(d.process_frame_48k(frame));
    }
    // 3 倍抽取 → 16k（离线验证够用；线上走 sherpa 的有状态重采样）
    let out: Vec<f32> = clean.iter().step_by(3).copied().collect();
    match eryu_core::audio::write_wav16(std::path::Path::new(&args[2]), &out) {
        Ok(()) => println!(
            "DENOISE48 OK {} -> {}（{} 采样）",
            args[1],
            args[2],
            out.len()
        ),
        Err(e) => {
            eprintln!("写 WAV 失败：{e}");
            std::process::exit(1);
        }
    }
}
