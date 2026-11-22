//! 局域网听写接口（M5 路线②）：不启真引擎，用 StubEngine 验证协议与边界。
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use eryu_core::asr::StubEngine;

fn http(port: u16, method: &str, path: &str, body: &[u8], token: Option<&str>) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(t) = token {
        req.push_str(&format!("x-eryu-token: {t}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).unwrap();
    // 服务端可能先回 403 再断开（不读 body），写失败是正常的
    let _ = s.write_all(body);
    let mut buf = Vec::new();
    // 服务端在 403 后直接关连接，客户端可能读到最后是 RST；已读到的字节仍然可用
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).to_string();
    let code: u16 = text
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (code, body)
}

fn wav_silence(secs: f32) -> Vec<u8> {
    let n = (16000.0 * secs) as u32;
    let mut data = Vec::new();
    for _ in 0..n {
        data.extend_from_slice(&0i16.to_le_bytes());
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes());
    out.extend_from_slice(&32000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

#[test]
fn health_and_transcribe_over_http() {
    let engine: Arc<Mutex<Box<dyn eryu_core::asr::AsrEngine>>> =
        Arc::new(Mutex::new(Box::new(StubEngine::default())));
    let vad = std::env::temp_dir().join("eryu-lan-missing-vad.onnx");
    let port = eryu_core::lan::spawn(
        engine,
        vad,
        eryu_core::lan::Options {
            port: 0,
            token: String::new(),
            allow_lan: false,
        },
    )
    .expect("spawn");

    let (code, body) = http(port, "GET", "/health", b"", None);
    assert_eq!(code, 200, "{body}");
    assert!(body.contains("\"ok\":true"), "{body}");
    assert!(body.contains("stub"), "{body}");

    let wav = wav_silence(1.0);
    let (code, body) = http(port, "POST", "/transcribe", &wav, None);
    assert_eq!(code, 200, "{body}");
    assert!(body.contains("测试文本"), "{body}");

    let (code, body) = http(port, "POST", "/transcribe", b"not a wav", None);
    assert_eq!(code, 400, "{body}");
    assert!(body.contains("bad_wav"), "{body}");

    let (code, _) = http(port, "GET", "/nope", b"", None);
    assert_eq!(code, 404);

    // 网页版（手机/浏览器用）：GET / 返回 HTML
    let (code, body) = http(port, "GET", "/", b"", None);
    assert_eq!(code, 200, "页面不需要 token 也要能打开");
    assert!(
        body.contains("Eryu 听写"),
        "{}",
        &body[..body.len().min(120)]
    );
    assert!(body.contains("/transcribe"));
    assert!(!body.contains("{{"), "HTML 里不该有模板残留");
}

#[test]
fn token_required_when_configured() {
    let engine: Arc<Mutex<Box<dyn eryu_core::asr::AsrEngine>>> =
        Arc::new(Mutex::new(Box::new(StubEngine::default())));
    let vad = std::env::temp_dir().join("eryu-lan-missing-vad.onnx");
    let port = eryu_core::lan::spawn(
        engine,
        vad,
        eryu_core::lan::Options {
            port: 0,
            token: "s3cret".into(),
            allow_lan: false,
        },
    )
    .expect("spawn");

    let (code, body) = http(port, "GET", "/health", b"", None);
    assert_eq!(code, 403, "{body}");
    let (code, body) = http(port, "GET", "/health", b"", Some("s3cret"));
    assert_eq!(code, 200, "{body}");
    let (code, _) = http(port, "GET", "/health?token=s3cret", b"", None);
    assert_eq!(code, 200);
    // 页面例外：不带 token 也能打开（要在页面上填 token）
    let (code, body) = http(port, "GET", "/", b"", None);
    assert_eq!(code, 200, "{body}");
    // 但转写仍然要 token
    let (code, _) = http(port, "POST", "/transcribe", &wav_silence(0.6), None);
    assert_eq!(code, 403);
}
