//! 离线验证：读取一条 WAV，走与线上完全相同的 VAD + 引擎 + 拼接流水线。
//!
//! cargo run --release --example recognize_wav -- /path/to.wav
//!
//! 打印：设备/引擎、分段数、每段耗时、拼接结果、峰值 RSS 参考值。

use std::time::Instant;

use eryu_core::{asr, audio, config, hotwords, pipeline, vad};

fn main() {
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("用法: recognize_wav <16k mono wav>");
            std::process::exit(2);
        }
    };
    let cfg = config::Config::load(None);
    println!(
        "引擎: {}  模型: {}",
        cfg.engine,
        if cfg.engine == "qwen3" {
            cfg.qwen3_dir.display().to_string()
        } else {
            cfg.model_dir.display().to_string()
        }
    );

    let t = Instant::now();
    let engine = match asr::make_engine(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("加载失败: {e}");
            std::process::exit(1);
        }
    };
    println!("加载耗时: {}ms", t.elapsed().as_millis());
    let mut engine: Box<dyn asr::AsrEngine> = engine;
    let _ = engine.name();

    let wave = match sherpa_onnx::Wave::read(&path) {
        Some(w) => w,
        None => {
            eprintln!("读取 WAV 失败: {path}");
            std::process::exit(1);
        }
    };
    let mut pcm = wave.samples().to_vec();
    if wave.sample_rate() != audio::TARGET_RATE as i32 {
        eprintln!(
            "提示: 采样率 {} 不是 16000，本例不做重采样",
            wave.sample_rate()
        );
    }
    audio::cap_len(&mut pcm);
    let secs = pcm.len() as f32 / audio::TARGET_RATE as f32;
    println!("音频: {:.2}s", secs);

    let segmenter = vad::Segmenter::new(&cfg.vad_model).ok();
    println!(
        "VAD: {}",
        if segmenter.is_some() {
            "开"
        } else {
            "关（固定窗口）"
        }
    );

    let t = Instant::now();
    // 线上降噪在 48k 采集侧做（D-035）；WAV 已经是 16k，再降到 16k 只会引入插值空频带，
    // 反而把字吃掉（实测 5dB 噪声下「下午五点」→「下午九点」）。所以默认不降噪，
    // 只有显式 ERYU_BENCH_DENOISE=1 时才做离线对比。
    let pcm = if cfg.denoise && std::env::var("ERYU_BENCH_DENOISE").ok().as_deref() == Some("1") {
        let t = std::time::Instant::now();
        let out = eryu_core::denoise::Denoiser::new().process(&pcm);
        println!("降噪: 开（16k 实验路径，{}ms）", t.elapsed().as_millis());
        out
    } else {
        println!("降噪: 关（线上在采集侧做）");
        pcm
    };
    match pipeline::run(
        engine.as_mut(),
        segmenter.as_ref(),
        &pcm,
        audio::TARGET_RATE,
    ) {
        Ok(r) => {
            let ms = t.elapsed().as_millis() as f64;
            println!("分段: {}（失败 {}）", r.segments.len(), r.failed);
            for s in &r.segments {
                let d = s.samples as f32 / audio::TARGET_RATE as f32;
                match &s.error {
                    Some(e) => println!("  #{} {:.2}s 失败: {e}", s.index, d),
                    None => println!("  #{} {:.2}s → {}", s.index, d, s.text),
                }
            }
            println!("总耗时: {ms}ms  RTF {:.3}", ms / 1000.0 / secs as f64);
            println!("识别原文: {}", r.text);
            let hw = hotwords::Hotwords::load(&cfg.dir);
            let replaced = hw.apply(r.text.trim());
            println!("热词表: {} 条", hw.len());
            if replaced != r.text.trim() {
                println!("热词后: {replaced}");
            }
        }
        Err(e) => {
            eprintln!("识别失败: {e}");
            std::process::exit(1);
        }
    }
}
