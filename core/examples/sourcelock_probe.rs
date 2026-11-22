//! 音源锁定验证：把两段不同人的语音交替拼起来，看锁定机制能不能只留下第一个人。
//!
//! `cargo run --release --example sourcelock_probe -- <a.wav> <b.wav>`
//! 期望：第 1 段锁定 A，之后 A 的段留下、B 的段丢掉；连续 3s 都丢不掉时应当解锁。

use eryu_core::sourcelock::{Decision, Lock};
use eryu_core::voicelock::{model_path, VoicePrint};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("用法: sourcelock_probe <a.wav> <b.wav>");
        std::process::exit(2);
    }
    let cfg_dir = std::env::var("ERYU_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| dirs_home());
    let model = model_path(&cfg_dir);
    println!("声纹模型：{}", model.display());
    let a = eryu_core::audio::read_wav16(Path::new(&args[1])).expect("a.wav");
    let b = eryu_core::audio::read_wav16(Path::new(&args[2])).expect("b.wav");

    // 交替：A B A B A B（每段用 2s 窗口，保证够长又不会太长）
    // 段序列：默认交替 ABABAB，可传 "BBBB" 之类验证「连续不像就解锁」
    let pattern: Vec<char> = args
        .get(3)
        .map(|p| p.chars().collect())
        .unwrap_or_else(|| "ABABAB".chars().collect());
    let seg = 32_000;
    let mut lock = Lock::new();
    for (i, who_ch) in pattern.iter().enumerate() {
        let src = if *who_ch == 'A' { &a } else { &b };
        let chunk = &src[..src.len().min(seg)];
        let who = if *who_ch == 'A' {
            "A(目标)"
        } else {
            "B(插话)"
        };
        let d = lock.check(chunk, &model, 0.62, i);
        let tag = match d {
            Decision::Locked => "锁定",
            Decision::Keep => "留下",
            Decision::Drop => "丢掉",
        };
        println!("第 {} 段 {who} → {tag}", i + 1);
    }
    println!("{}", lock.summary());
    let _ = VoicePrint {
        model: String::new(),
        dim: 0,
        created: String::new(),
        vec: Vec::new(),
    };
}

fn dirs_home() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".eryu")
}
