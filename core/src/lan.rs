//! 局域网听写接口（DESIGN §5 M5 路线②）：别的设备（手机、另一台电脑）把一段 WAV 发过来，
//! 这台机器用本地模型识别，把文字发回去。**不上屏、不写历史**，只做「转文字」这一件事。
//!
//! - 默认关闭；默认只监听 127.0.0.1（`allow_lan = true` 才监听 0.0.0.0）
//! - 可选 token：配了就必须带 `x-eryu-token: <token>`（或 `?token=`），否则 403
//! - 协议刻意做小：`GET /health`、`POST /transcribe`（body = 16k 单声道 16-bit WAV）

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::asr::AsrEngine;
use crate::audio;

const PAGE: &str = r#"
<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>Eryu 听写</title>
<style>
  :root { color-scheme: light dark; }
  * { box-sizing: border-box; -webkit-tap-highlight-color: transparent; }
  body { margin: 0; font: 16px/1.5 -apple-system, "PingFang SC", system-ui, sans-serif;
         background: #f5f5f7; color: #1d1d1f; display: flex; justify-content: center; }
  @media (prefers-color-scheme: dark) { body { background: #1c1c1e; color: #f2f2f7; } main { background:#2c2c2e !important; } }
  main { width: 100%; max-width: 560px; margin: 16px; padding: 20px; border-radius: 18px;
         background: #fff; box-shadow: 0 8px 30px rgba(0,0,0,.08); }
  h1 { font-size: 20px; margin: 0 0 4px; }
  p.sub { margin: 0 0 16px; color: #86868b; font-size: 13px; }
  button { font: inherit; border: 0; border-radius: 12px; padding: 14px 18px; cursor: pointer; }
  #rec { width: 100%; background: #0a84ff; color: #fff; font-weight: 600; }
  #rec.on { background: #ff453a; }
  #rec:disabled { opacity: .45; cursor: default; }
  .row { display: flex; gap: 10px; margin-top: 10px; }
  .row button { flex: 1; background: rgba(120,120,128,.14); color: inherit; }
  textarea { width: 100%; min-height: 140px; margin-top: 14px; padding: 12px; border-radius: 12px;
             border: 1px solid rgba(120,120,128,.3); background: transparent; color: inherit; font: inherit; resize: vertical; }
  input[type=password], input[type=text] { width: 100%; padding: 11px 12px; margin-top: 12px; font: inherit;
             border-radius: 12px; border: 1px solid rgba(120,120,128,.3); background: transparent; color: inherit; }
  .hint { margin-top: 12px; font-size: 12px; color: #86868b; }
  .err { color: #ff453a; }
  #bar { height: 4px; border-radius: 2px; background: rgba(120,120,128,.2); margin-top: 12px; overflow: hidden; }
  #bar i { display: block; height: 100%; width: 0%; background: #0a84ff; transition: width .1s linear; }
  input[type=file] { margin-top: 12px; font-size: 13px; }
</style>
</head>
<body>
<main>
  <h1>Eryu 听写</h1>
  <p class="sub">音频发到这台 Mac，用它的本地模型转文字（不上传任何云端）。</p>

  <button id="rec">按住说话？点一下开始</button>
  <div id="bar"><i></i></div>
  <div class="row">
    <button id="stop" disabled>结束并转文字</button>
    <button id="clear">清空</button>
  </div>

  <input id="token" type="password" placeholder="token（这台 Mac 上设过的那个）" autocomplete="off">
  <textarea id="out" placeholder="识别结果会出现在这里"></textarea>
  <div class="row"><button id="copy">复制结果</button></div>

  <input id="file" type="file" accept="audio/*">
  <div class="hint" id="hint"></div>
  <div class="hint">说明：浏览器录音需要 https 或 localhost 才被允许；用 IP 直接打开时只能选文件（手机上先录好音再选）。</div>
</main>
<script>
const $ = (id) => document.getElementById(id);
let ctx = null, stream = null, src = null, proc = null, chunks = [], recording = false, sampleRate = 48000;

function wavFromFloat32(samples, rate) {
  const buffer = new ArrayBuffer(44 + samples.length * 2);
  const view = new DataView(buffer);
  const writeStr = (off, s) => { for (let i = 0; i < s.length; i++) view.setUint8(off + i, s.charCodeAt(i)); };
  writeStr(0, 'RIFF'); view.setUint32(4, 36 + samples.length * 2, true); writeStr(8, 'WAVE');
  writeStr(12, 'fmt '); view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true);
  view.setUint32(24, rate, true); view.setUint32(28, rate * 2, true); view.setUint16(32, 2, true); view.setUint16(34, 16, true);
  writeStr(36, 'data'); view.setUint32(40, samples.length * 2, true);
  let off = 44;
  for (let i = 0; i < samples.length; i++, off += 2) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(off, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  return new Uint8Array(buffer);
}

// 降到 16k（服务端只吃 16k）：简单的线性重采样足够语音用
function resample(input, from, to) {
  if (from === to) return input;
  const ratio = from / to, out = new Float32Array(Math.floor(input.length / ratio));
  for (let i = 0; i < out.length; i++) {
    const pos = i * ratio, i0 = Math.floor(pos), i1 = Math.min(i0 + 1, input.length - 1), t = pos - i0;
    out[i] = input[i0] * (1 - t) + input[i1] * t;
  }
  return out;
}

$('token').value = localStorage.getItem('eryu_token') || '';
$('token').addEventListener('change', () => localStorage.setItem('eryu_token', $('token').value));

async function start() {
  $('hint').textContent = '';
  try {
    stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true } });
  } catch (e) {
    $('hint').innerHTML = '<span class="err">拿不到麦克风：' + (e.message || e) + '（用 IP 打开时必须走 https；也可以下面选文件）</span>';
    return;
  }
  ctx = new (window.AudioContext || window.webkitAudioContext)();
  sampleRate = ctx.sampleRate;
  src = ctx.createMediaStreamSource(stream);
  proc = ctx.createScriptProcessor(4096, 1, 1);
  chunks = [];
  proc.onaudioprocess = (ev) => { if (recording) chunks.push(new Float32Array(ev.inputBuffer.getChannelData(0))); };
  src.connect(proc); proc.connect(ctx.destination);
  recording = true;
  $('rec').textContent = '正在录…点一下停止';
  $('rec').classList.add('on'); $('stop').disabled = false;
  const t0 = Date.now();
  const timer = setInterval(() => { if (!recording) { clearInterval(timer); return; }
    $('bar').firstElementChild.style.width = Math.min(100, (Date.now() - t0) / 600) + '%'; }, 100);
}

function stop() {
  if (!recording) return;
  recording = false;
  $('rec').textContent = '点一下开始录音'; $('rec').classList.remove('on');
  $('bar').firstElementChild.style.width = '0%';
  if (stream) stream.getTracks().forEach(t => t.stop());
  if (proc) { proc.disconnect(); proc = null; }
  if (src) { src.disconnect(); src = null; }
  if (ctx) { ctx.close(); ctx = null; }
  const total = chunks.reduce((n, c) => n + c.length, 0);
  if (total < sampleRate * 0.3) { $('hint').textContent = '太短了，再录长一点'; return; }
  const all = new Float32Array(total);
  let off = 0;
  for (const c of chunks) { all.set(c, off); off += c.length; }
  chunks = [];
  send(wavFromFloat32(resample(all, sampleRate, 16000), 16000));
}

async function send(bytes) {
  const token = $('token').value.trim();
  $('out').value = '识别中…';
  try {
    const t0 = Date.now();
    const res = await fetch('/transcribe', {
      method: 'POST',
      headers: token ? { 'x-eryu-token': token } : {},
      body: bytes
    });
    const data = await res.json();
    if (!data.ok) throw new Error(data.message || ('HTTP ' + res.status));
    $('out').value = data.text || '（这段没听出内容）';
    $('hint').textContent = '用时 ' + (Date.now() - t0) + 'ms' + (data.chars ? '，' + data.chars + ' 字' : '');
  } catch (e) {
    $('out').value = '';
    $('hint').innerHTML = '<span class="err">失败：' + (e.message || e) + '</span>';
  }
}

// 选文件：解码 → 重采样到 16k → 发出去（任何环境都能用）
$('file').addEventListener('change', async (ev) => {
  const f = ev.target.files[0];
  if (!f) return;
  try {
    const buf = await f.arrayBuffer();
    const ac = new (window.AudioContext || window.webkitAudioContext)();
    const audio = await ac.decodeAudioData(buf);
    const mono = new Float32Array(audio.length);
    for (let c = 0; c < audio.numberOfChannels; c++) {
      const d = audio.getChannelData(c);
      for (let i = 0; i < d.length; i++) mono[i] += d[i] / audio.numberOfChannels;
    }
    const rate = audio.sampleRate;
    ac.close();
    send(wavFromFloat32(resample(mono, rate, 16000), 16000));
  } catch (e) {
    $('hint').innerHTML = '<span class="err">这个文件解不开：' + (e.message || e) + '</span>';
  }
});

$('rec').onclick = () => (recording ? stop() : start());
$('stop').onclick = () => { stop(); $('stop').disabled = true; };
$('clear').onclick = () => { $('out').value = ''; $('hint').textContent = ''; };
$('copy').onclick = async () => {
  const text = $('out').value;
  if (!text) return;
  try { await navigator.clipboard.writeText(text); $('hint').textContent = '已复制'; }
  catch { $('out').select(); document.execCommand('copy'); $('hint').textContent = '已复制'; }
};
</script>
</body>
</html>
"#;

/// 请求体上限：25MB（16k/16bit/单声道 ≈ 13 分钟）
pub const MAX_BODY: usize = 25 * 1024 * 1024;
/// 单次请求最长处理时间（超过就断开，避免占住引擎）
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

/// 最近一次请求的摘要（**不记内容**，只记时间/来源/状态/耗时/字节）
#[derive(Clone)]
pub struct LogEntry {
    pub at: u64,
    pub src: String,
    pub path: String,
    pub code: u16,
    pub ms: u64,
    pub bytes: usize,
}

const LOG_CAP: usize = 40;
static LOG: Mutex<Vec<LogEntry>> = Mutex::new(Vec::new());
/// 监听线程的停止信号：设置页改了开关/端口后要能不用重启整个 App 就重来
static STOP: AtomicBool = AtomicBool::new(false);
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
/// 同时在处理的请求上限：引擎本身串行，这个上限是为了让多余的请求**快速拿到 503**，
/// 而不是排队等到浏览器超时。
pub const MAX_IN_FLIGHT: usize = 2;
const BUSY_JSON: &str =
    "{\"ok\":false,\"code\":\"busy\",\"message\":\"Eryu 正在处理别的请求，稍等一下再试\"}";

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn record(src: &str, path: &str, code: u16, ms: u64, bytes: usize) {
    if let Ok(mut log) = LOG.lock() {
        log.push(LogEntry {
            at: now_secs(),
            src: src.to_string(),
            path: path.to_string(),
            code,
            ms,
            bytes,
        });
        if log.len() > LOG_CAP {
            let drop = log.len() - LOG_CAP;
            log.drain(0..drop);
        }
    }
}

/// 最近请求（新的在后），给诊断页和 `--lan-log` 用。
pub fn recent() -> Vec<LogEntry> {
    LOG.lock().map(|l| l.clone()).unwrap_or_default()
}

pub fn recent_json() -> String {
    let mut out = String::from("[");
    for (i, e) in recent().iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"at\":{},\"src\":\"{}\",\"path\":\"{}\",\"code\":{},\"ms\":{},\"bytes\":{}}}",
            e.at,
            json_escape(&e.src),
            json_escape(&e.path),
            e.code,
            e.ms,
            e.bytes
        ));
    }
    out.push(']');
    out
}

/// 停掉当前监听（没有在听就什么也不做）
pub fn stop() {
    STOP.store(true, Ordering::SeqCst);
    // 等监听循环看到标志退出，避免「刚停就绑同一端口」时 binder 还没释放
    std::thread::sleep(Duration::from_millis(160));
}

pub struct Options {
    pub port: u16,
    pub token: String,
    pub allow_lan: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            port: 8765,
            token: String::new(),
            allow_lan: false,
        }
    }
}

/// 启动监听线程，返回实际端口（传 0 时由系统分配，测试用）。
pub fn spawn(
    engine: Arc<Mutex<Box<dyn AsrEngine>>>,
    vad_model: PathBuf,
    opts: Options,
) -> Result<u16, String> {
    let host = if opts.allow_lan {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    };
    let listener = TcpListener::bind((host, opts.port))
        .map_err(|e| format!("监听 {host}:{} 失败：{e}", opts.port))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("取端口失败：{e}"))?
        .port();
    let token = opts.token.clone();
    STOP.store(false, Ordering::SeqCst);
    let _ = listener.set_nonblocking(true);
    std::thread::Builder::new()
        .name("eryu-lan".into())
        .spawn(move || loop {
            if STOP.load(Ordering::SeqCst) {
                break;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Err(e) = stream.set_nonblocking(false) {
                        eprintln!("[eryu][lan] 连接设为阻塞失败：{e}");
                    }
                    let stream = stream;
                    let engine = engine.clone();
                    let vad_model = vad_model.clone();
                    let token = token.clone();
                    let src = stream
                        .peer_addr()
                        .map(|a| a.ip().to_string())
                        .unwrap_or_default();
                    // 先窥一眼请求行拿 path（不出错就是 GET/POST 里第一个空格后面那段）
                    let path = {
                        let mut buf = [0u8; 256];
                        match stream.peek(&mut buf) {
                            Ok(n) if n > 0 => {
                                let s = String::from_utf8_lossy(&buf[..n]).to_string();
                                s.lines()
                                    .next()
                                    .unwrap_or("")
                                    .split(' ')
                                    .nth(1)
                                    .map(|p| p.split('?').next().unwrap_or("").to_string())
                                    .unwrap_or_default()
                            }
                            _ => String::new(),
                        }
                    };
                    let started = std::time::Instant::now();
                    let inflight = IN_FLIGHT.fetch_add(1, Ordering::SeqCst) + 1;
                    if inflight > MAX_IN_FLIGHT {
                        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
                        let mut s2 = stream;
                        // 先把请求体读掉再回：直接关连接会让客户端拿到 RST（curl 显示 000）而不是 503
                        drain_request(&mut s2);
                        let _ = respond(&mut s2, 503, BUSY_JSON);
                        // 客户端可能还在发 body：先半关写、把残余读掉再丢，否则它会拿到 RST（curl 显示 000）
                        let _ = s2.shutdown(std::net::Shutdown::Write);
                        drain_request(&mut s2);
                        record(
                            &src,
                            &path,
                            503,
                            started.elapsed().as_millis() as u64,
                            BUSY_JSON.len(),
                        );
                        // 必须是 continue：这里 return 会连**监听循环**一起退出，后面的连接全被挂死
                        continue;
                    }
                    // 每个请求一个线程：识别是串行的（引擎有锁），但慢客户端读 body 不挡住别人
                    std::thread::Builder::new()
                        .name("eryu-lan-req".into())
                        .spawn(move || {
                            let ms_start = std::time::Instant::now();
                            let (code, bytes) =
                                handle(stream, engine, &vad_model, &token).unwrap_or((0, 0));
                            IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
                            record(
                                &src,
                                &path,
                                code,
                                ms_start.elapsed().as_millis() as u64,
                                bytes,
                            );
                        })
                        .ok();
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(80));
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(80));
                }
            }
        })
        .map_err(|e| format!("起监听线程失败：{e}"))?;
    Ok(port)
}

/// 读掉一个请求（请求行 + 头 + body），最多 1MB，用于「忙」时干净地回 503。
fn drain_request(stream: &mut TcpStream) {
    use std::io::Read;
    stream.set_read_timeout(Some(Duration::from_secs(1))).ok();
    let mut buf = [0u8; 8192];
    let mut got = 0usize;
    let mut head = Vec::new();
    while got < 1 << 20 {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                got += n;
                head.extend_from_slice(&buf[..n]);
                if head.windows(4).any(|w| w == b"\r\n\r\n") {
                    // 头部到了：按 Content-Length 把剩下的读完
                    let text = String::from_utf8_lossy(&head).to_lowercase();
                    let want: usize = text
                        .split("content-length:")
                        .nth(1)
                        .and_then(|t| t.split('\r').next())
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    if let Some(pos) = head.windows(4).position(|w| w == b"\r\n\r\n") {
                        let body_got = head.len().saturating_sub(pos + 4);
                        if body_got >= want {
                            break;
                        }
                    }
                }
            }
            Err(_) => break,
        }
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn reply(stream: &mut TcpStream, code: u16, body: &str) -> std::io::Result<(u16, usize)> {
    respond(stream, code, body)?;
    Ok((code, body.len()))
}

fn respond(stream: &mut TcpStream, code: u16, body: &str) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        _ => "Internal Server Error",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

fn respond_html(stream: &mut TcpStream, code: u16, body: &str) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {code} OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

fn err_json(code: &str, message: &str) -> String {
    format!(
        "{{\"ok\":false,\"code\":\"{code}\",\"message\":\"{}\"}}",
        json_escape(message)
    )
}

fn handle(
    mut stream: TcpStream,
    engine: Arc<Mutex<Box<dyn AsrEngine>>>,
    vad_model: &PathBuf,
    token: &str,
) -> std::io::Result<(u16, usize)> {
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let parts: Vec<&str> = request_line.trim_end().split(' ').collect();
    if parts.len() < 2 {
        return reply(&mut stream, 400, &err_json("bad_request", "请求行看不懂"));
    }
    let method = parts[0].to_string();
    let path = parts[1].split('?').next().unwrap_or("").to_string();
    let query_token = parts[1]
        .split('?')
        .nth(1)
        .and_then(|q| {
            q.split('&')
                .find_map(|kv| kv.strip_prefix("token=").map(|v| v.to_string()))
        })
        .unwrap_or_default();

    let mut content_length = 0usize;
    let mut header_token = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        let lower = l.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
        if let Some(v) = lower.strip_prefix("x-eryu-token:") {
            header_token = v.trim().to_string();
        }
    }

    // 页面本身不含数据，且浏览器里没 token 就打不开页面 -> 没法在页面上填 token（先有鸡还是先有蛋）
    let is_page = method == "GET" && (path == "/" || path == "/index.html");
    if !token.is_empty() && !is_page {
        let got = if header_token.is_empty() {
            query_token
        } else {
            header_token
        };
        if got != token {
            return reply(
                &mut stream,
                403,
                &err_json("forbidden", "token 不对：请在请求头加 x-eryu-token"),
            );
        }
    }

    match (method.as_str(), path.as_str()) {
        ("GET", "/health") => {
            let name = engine
                .lock()
                .map(|e| e.name().to_string())
                .unwrap_or_default();
            let ready = vad_model.exists();
            let body = format!(
                "{{\"ok\":true,\"engine\":\"{}\",\"vad_ready\":{},\"protocol\":1}}",
                json_escape(&name),
                ready
            );
            reply(&mut stream, 200, &body)
        }
        ("POST", "/transcribe") => {
            if content_length == 0 {
                return reply(
                    &mut stream,
                    400,
                    &err_json("empty_body", "请求体是空的：请把 WAV 文件内容放在 body 里"),
                );
            }
            if content_length > MAX_BODY {
                return reply(
                    &mut stream,
                    413,
                    &err_json(
                        "too_large",
                        &format!(
                            "太长了（{} 字节）：上限 25MB（约 13 分钟 16k 音频）",
                            content_length
                        ),
                    ),
                );
            }
            let mut body = vec![0u8; content_length];
            let started = Instant::now();
            let mut read = 0usize;
            while read < content_length {
                let n = reader.read(&mut body[read..])?;
                if n == 0 {
                    break;
                }
                read += n;
                if started.elapsed() > REQUEST_TIMEOUT {
                    return reply(
                        &mut stream,
                        408,
                        &err_json("timeout", "收数据太慢了，已断开"),
                    );
                }
            }
            body.truncate(read);

            let path_tmp =
                std::env::temp_dir().join(format!("eryu-lan-{}.wav", std::process::id()));
            let pcm = match write_and_read(&path_tmp, &body) {
                Ok(p) => {
                    let _ = std::fs::remove_file(&path_tmp);
                    p
                }
                Err(e) => return reply(&mut stream, 400, &err_json("bad_wav", &e)),
            };
            if pcm.len() < 16000 / 4 {
                return reply(
                    &mut stream,
                    400,
                    &err_json("too_short", "音频太短了：至少 0.25 秒"),
                );
            }
            let t0 = Instant::now();
            let result = {
                let guard = engine.lock();
                match guard {
                    Ok(mut e) => {
                        let segmenter = crate::vad::Segmenter::new(vad_model).ok();
                        crate::pipeline::run(&mut **e, segmenter.as_ref(), &pcm, 16000)
                    }
                    Err(_) => Err("引擎忙（上一个请求还没做完），稍后重试".to_string()),
                }
            };
            let ms = t0.elapsed().as_millis();
            match result {
                Ok(r) => {
                    let text = r.text.trim();
                    if text.is_empty() {
                        return reply(
                            &mut stream,
                            200,
                            "{\"ok\":true,\"text\":\"\",\"ms\":0,\"empty\":true}",
                        );
                    }
                    let body = format!(
                        "{{\"ok\":true,\"text\":\"{}\",\"ms\":{},\"segments\":{},\"chars\":{}}}",
                        json_escape(text),
                        ms,
                        r.segments.len(),
                        text.chars().count()
                    );
                    reply(&mut stream, 200, &body)
                }
                Err(e) => reply(&mut stream, 500, &err_json("decode_failed", &e)),
            }
        }
        ("GET", "/") | ("GET", "/index.html") => {
            respond_html(&mut stream, 200, PAGE)?;
            Ok((200, PAGE.len()))
        }
        _ => reply(
            &mut stream,
            404,
            &err_json("not_found", "只支持 GET /health 和 POST /transcribe"),
        ),
    }
}

fn write_and_read(path: &std::path::Path, bytes: &[u8]) -> Result<Vec<f32>, String> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" {
        return Err("不是 WAV 文件（要从 RIFF 开头）".into());
    }
    std::fs::write(path, bytes).map_err(|e| format!("写临时文件失败：{e}"))?;
    audio::read_wav16(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_json() {
        assert_eq!(json_escape("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
    }

    #[test]
    fn rejects_non_wav() {
        assert!(write_and_read(std::path::Path::new("/tmp/eryu-lan-test.wav"), b"hello").is_err());
    }
}
