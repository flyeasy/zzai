//! 模型下载自测：`cargo run --example pull_model -- <id> <目标目录>`
//!
//! 用来验证 ureq 下载 + tar 解包 + 标志文件校验这条链路（不经过 UI）。

use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let id = args.next().unwrap_or_else(|| "vad".to_string());
    let dir = args
        .next()
        .unwrap_or_else(|| "/tmp/eryu-pull-test".to_string());
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("建目录");

    let Some(entry) = eryu_core::models::find(&id) else {
        eprintln!("未知模型：{id}");
        std::process::exit(2);
    };
    println!("下载 {} → {}", entry.id, dir.display());
    println!("  地址 {}", entry.url);
    println!(
        "  预估 ~{} MB，解包后标志文件 {}",
        entry.approx_mb, entry.marker
    );
    let started = Instant::now();
    let mut last_print = Instant::now();
    let result = eryu_core::models::fetch(
        &dir,
        entry,
        "",
        "",
        |received, total| {
            if last_print.elapsed().as_millis() >= 500 {
                let pct = if total > 0 {
                    format!("{:.1}%", received as f64 / total as f64 * 100.0)
                } else {
                    "?".to_string()
                };
                println!(
                    "  {:.1}s 已下载 {:.1} MB / {} MB ({pct})",
                    started.elapsed().as_secs_f64(),
                    received as f64 / 1_048_576.0,
                    total / 1_048_576
                );
                last_print = Instant::now();
            }
        },
        || false,
    );
    match result {
        Ok(path) => {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            println!(
                "PULL OK {} 用时 {:.1}s 标志文件 {}（{:.1} MB）",
                entry.id,
                started.elapsed().as_secs_f64(),
                path.display(),
                size as f64 / 1_048_576.0
            );
        }
        Err(e) => {
            println!("PULL FAIL {} {e}", entry.id);
            std::process::exit(1);
        }
    }
}
