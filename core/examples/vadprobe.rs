use sherpa_onnx::{VadModelConfig, VoiceActivityDetector};

fn main() {
    let v = sherpa_onnx::Wave::read(&std::env::args().nth(1).unwrap()).unwrap();
    println!("sr={} n={}", v.sample_rate(), v.samples().len());
    let mut cfg = VadModelConfig::default();
    cfg.silero_vad.model = Some(std::env::var("ERYU_VAD_MODEL").unwrap_or_else(|_| {
        format!(
            "{}/.eryu/models/silero_vad.onnx",
            std::env::var("HOME").unwrap()
        )
    }));
    cfg.silero_vad.threshold = 0.5;
    cfg.silero_vad.min_silence_duration = 0.4;
    cfg.silero_vad.min_speech_duration = 0.25;
    cfg.silero_vad.window_size = 512;
    cfg.silero_vad.max_speech_duration = 30.0;
    cfg.sample_rate = 16000;
    cfg.num_threads = 1;
    cfg.debug = std::env::var("VAD_DEBUG").is_ok();
    let vad = VoiceActivityDetector::create(&cfg, 60.0).unwrap();
    // 喂入方式：整段 vs 逐窗（环境变量 VAD_WHOLE=1 用整段）
    let w: usize = if std::env::var("VAD_WHOLE").is_ok() {
        v.samples().len()
    } else {
        512
    };
    let s = v.samples();
    let mut segs = 0;
    for chunk in s.chunks(w) {
        vad.accept_waveform(chunk);
        while vad.front().is_some() {
            let seg = vad.front().unwrap();
            segs += 1;
            println!("seg {} start={} n={}", segs, seg.start(), seg.n());
            vad.pop();
        }
    }
    vad.flush();
    while let Some(seg) = vad.front() {
        segs += 1;
        println!("flush seg {} start={} n={}", segs, seg.start(), seg.n());
        vad.pop();
    }
    println!("total segs={}", segs);
}
