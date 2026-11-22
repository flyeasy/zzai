//! 离线降噪：把任意 16k WAV 过一遍 GTCRN，写出 `<in>-denoised.wav`。
//! 用途：A/B 表（scripts/denoise_table.py）——产品链路的降噪在采集侧，离线对比必须先降噪再解码。
//!
//! `cargo run --release --example denoise_file -- <gtcrn.onnx> <wav16>`

use eryu_core::{audio, denoise::Gtcrn};
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("用法: denoise_file <gtcrn.onnx> <wav16>");
        std::process::exit(2);
    }
    let model = Path::new(&args[1]);
    let wav = PathBuf::from(&args[2]);
    let pcm = audio::read_wav16(&wav).expect("读 wav");
    let d = Gtcrn::load(model).expect("加载 GTCRN");
    let out = d.run(&pcm);
    let path = with_suffix(&wav, "-denoised.wav");
    audio::write_wav16(&path, &out).expect("写 wav");
    println!(
        "DENOISED {} ({} 采样 → {})",
        path.display(),
        pcm.len(),
        out.len()
    );
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    p.with_file_name(format!("{stem}{suffix}"))
}
