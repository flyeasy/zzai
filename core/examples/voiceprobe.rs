//! 声纹实验：给定一个「我的声音」样本和若干待判断片段，算 embedding 余弦相似度。
//!
//! 用法：
//!   cargo run --release --example voiceprobe -- <模型.onnx> <我的样本.wav> <片段1.wav> [片段2.wav ...]
//!
//! 目的：在把「只录我的声音」接进主流程之前，先量清楚阈值——
//! 同一个人的不同句子应该高分，不同人（哪怕说的同一句话）应该明显更低。

use sherpa_onnx::{SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig, Wave};

fn embed(ex: &SpeakerEmbeddingExtractor, pcm: &[f32], sample_rate: i32) -> Option<Vec<f32>> {
    let stream = ex.create_stream()?;
    stream.accept_waveform(sample_rate, pcm);
    stream.input_finished();
    if !ex.is_ready(&stream) {
        return None;
    }
    ex.compute(&stream)
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na * nb)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("用法：voiceprobe <模型.onnx> <我的样本.wav> <片段.wav>...");
        std::process::exit(2);
    }
    let cfg = SpeakerEmbeddingExtractorConfig {
        model: Some(args[1].clone()),
        num_threads: 2,
        debug: false,
        provider: Some("cpu".into()),
    };
    let Some(ex) = SpeakerEmbeddingExtractor::create(&cfg) else {
        eprintln!("加载声纹模型失败：{}", args[1]);
        std::process::exit(1);
    };
    let t0 = std::time::Instant::now();
    let load_ms = t0.elapsed().as_millis();

    let Some(reference) = Wave::read(&args[2]).and_then(|w| {
        let pcm = w.samples().to_vec();
        embed(&ex, &pcm, w.sample_rate()).map(|e| (e, pcm.len() as f32 / w.sample_rate() as f32))
    }) else {
        eprintln!("读/嵌入失败：{}", args[2]);
        std::process::exit(1);
    };
    println!(
        "VOICEPROBE 模型加载 {}ms 维度 {} 参考={} ({:.1}s)",
        load_ms,
        reference.0.len(),
        args[2],
        reference.1
    );

    for path in &args[3..] {
        let Some(w) = Wave::read(path) else {
            eprintln!("读不出：{path}");
            continue;
        };
        let pcm = w.samples().to_vec();
        let secs = pcm.len() as f32 / w.sample_rate() as f32;
        let t = std::time::Instant::now();
        let Some(e) = embed(&ex, &pcm, w.sample_rate()) else {
            println!("VOICEFRAME {} 嵌入失败", path);
            continue;
        };
        let ms = t.elapsed().as_millis();
        println!(
            "VOICEFRAME {} 相似度={:.3} 时长={:.1}s 耗时={}ms",
            path,
            cosine(&reference.0, &e),
            secs,
            ms
        );
    }
}
