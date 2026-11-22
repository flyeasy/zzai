//! 真实麦克风端到端小样（不经过 UI）：
//!   cargo run --release --example record_once -- 4
//! 录 N 秒 → VAD 切段 → SenseVoice 识别 → 打印结果与各阶段耗时。
//! 用于在打包 App 之前验证「采音 → 重采样 → 识别」链路本身。

use std::time::{Duration, Instant};

use eryu_core::{asr, audio, config, pipeline, vad};

fn main() {
    let secs: f32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4.0);
    let cfg = config::Config::load(None);

    let t = Instant::now();
    let engine = asr::SenseVoiceEngine::load(&cfg.model_dir).expect("加载 SenseVoice 失败");
    println!("引擎加载 {}ms", t.elapsed().as_millis());
    let mut engine: Box<dyn asr::AsrEngine> = Box::new(engine);

    let cap = audio::start().expect("打开麦克风失败");
    println!(
        "设备 {} · {}Hz · {}ch",
        cap.device_name, cap.input_rate, cap.channels
    );
    let mut recorded: Vec<f32> = Vec::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs_f32(secs) {
        std::thread::sleep(Duration::from_millis(50));
        cap.drain(&mut recorded);
    }
    let dropped = cap
        .dropped_samples
        .load(std::sync::atomic::Ordering::Relaxed);
    cap.finish(&mut recorded);
    audio::cap_len(&mut recorded);
    println!(
        "录音 {:.2}s（{} 采样，丢帧 {}）",
        recorded.len() as f32 / audio::TARGET_RATE as f32,
        recorded.len(),
        dropped
    );
    if recorded.is_empty() {
        eprintln!("没有采到音频：检查麦克风权限/设备");
        std::process::exit(1);
    }

    let segmenter = vad::Segmenter::new(&cfg.vad_model).ok();
    let t = Instant::now();
    let r = pipeline::run(
        engine.as_mut(),
        segmenter.as_ref(),
        &recorded,
        audio::TARGET_RATE,
    )
    .expect("识别失败");
    println!(
        "识别 {}ms（{} 段，失败 {}）",
        t.elapsed().as_millis(),
        r.segments.len(),
        r.failed
    );
    for s in &r.segments {
        println!(
            "  #{} {:.2}s → {}",
            s.index,
            s.samples as f32 / audio::TARGET_RATE as f32,
            s.error.clone().unwrap_or_else(|| s.text.clone())
        );
    }
    println!("结果: {}", r.text);
}
