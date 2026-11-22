//! 降噪模型的处理速度：按 100ms 分块跑一遍，量 RTF 与峰值内存。
use eryu_core::audio;
use sherpa_onnx::{
    OfflineSpeechDenoiserGtcrnModelConfig, OfflineSpeechDenoiserModelConfig, OnlineSpeechDenoiser,
    OnlineSpeechDenoiserConfig,
};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model = &args[1];
    let wav = &args[2];
    let pcm = audio::read_wav16(std::path::Path::new(wav)).expect("wav");
    let t0 = Instant::now();
    let cfg = OnlineSpeechDenoiserConfig {
        model: OfflineSpeechDenoiserModelConfig {
            gtcrn: OfflineSpeechDenoiserGtcrnModelConfig {
                model: Some(model.clone()),
            },
            ..Default::default()
        },
    };
    let d = OnlineSpeechDenoiser::create(&cfg).expect("load");
    let load = t0.elapsed().as_millis();
    let t = Instant::now();
    let mut out = 0usize;
    for c in pcm.chunks(1600) {
        out += d.run(c, audio::TARGET_RATE as i32).samples.len();
    }
    out += d.flush().samples.len();
    let ms = t.elapsed().as_millis();
    let secs = pcm.len() as f64 / 16000.0;
    println!(
        "降噪 加载 {load}ms 处理 {ms}ms RTF {:.3} 样本 {out}",
        ms as f64 / 1000.0 / secs
    );
}
