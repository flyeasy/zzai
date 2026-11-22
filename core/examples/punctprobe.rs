//! 标点模型实验：给一段没有标点的转写文本加标点，看看值不值得为它多下 65MB。
//!
//! 用法：cargo run --release --example punctprobe -- <model.int8.onnx> "文本1" "文本2" ...
//!
//! 对照：现在的兜底只有 `pipeline::add_final_punct`（句末补一个「。」）。

use sherpa_onnx::{OfflinePunctuation, OfflinePunctuationConfig};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("用法：punctprobe <model.int8.onnx> \"文本\"...");
        std::process::exit(2);
    }
    let mut cfg = OfflinePunctuationConfig::default();
    cfg.model.ct_transformer = Some(args[1].clone());
    cfg.model.num_threads = 1;
    let t0 = std::time::Instant::now();
    let Some(punct) = OfflinePunctuation::create(&cfg) else {
        eprintln!("加载标点模型失败：{}", args[1]);
        std::process::exit(1);
    };
    println!("PUNCTPROBE 模型加载 {}ms", t0.elapsed().as_millis());

    for raw in &args[2..] {
        let t = std::time::Instant::now();
        match punct.add_punctuation(raw) {
            Some(out) => println!(
                "PUNCT 原文={raw}\n      加标点={out}\n      耗时={}ms",
                t.elapsed().as_millis()
            ),
            None => println!("PUNCT 失败：{raw}"),
        }
    }
}
