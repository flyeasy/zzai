//! 上屏：剪贴板 + SendInput Ctrl+V（W1 只做 paste；逐字与 UIA 校验属于 W2）。

use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
const CF_UNICODETEXT: u32 = 13;

fn open_clipboard() -> bool {
    unsafe {
        for _ in 0..10 {
            if OpenClipboard(HWND(std::ptr::null_mut())).is_ok() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }
}

fn set_clipboard_text(text: &str) -> bool {
    if !open_clipboard() {
        return false;
    }
    unsafe {
        let _ = EmptyClipboard();
        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = utf16.len() * 2;
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes);
        let ok = match h {
            Ok(h) => {
                let p = GlobalLock(HGLOBAL(h.0)) as *mut u16;
                if p.is_null() {
                    false
                } else {
                    std::ptr::copy_nonoverlapping(utf16.as_ptr(), p, utf16.len());
                    let _ = GlobalUnlock(h);
                    let r = SetClipboardData(CF_UNICODETEXT, HANDLE(h.0));
                    r.is_ok()
                }
            }
            Err(_) => false,
        };
        let _ = CloseClipboard();
        ok
    }
}

fn get_clipboard_text() -> Option<String> {
    if !open_clipboard() {
        return None;
    }
    let out = unsafe {
        let h = GetClipboardData(CF_UNICODETEXT).ok();
        let s = h.and_then(|h| {
            let p = GlobalLock(HGLOBAL(h.0)) as *const u16;
            if p.is_null() {
                return None;
            }
            // 畸形剪贴板可能没有结尾 0：按 GlobalSize 封顶，别越界读崩进程
            let size = windows::Win32::System::Memory::GlobalSize(HGLOBAL(h.0)) as usize;
            let cap = (size / 2).max(1);
            let mut len = 0usize;
            while len < cap && *p.add(len) != 0 {
                len += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            let _ = GlobalUnlock(HGLOBAL(h.0));
            Some(s)
        });
        let _ = CloseClipboard();
        s
    };
    out
}

fn send_ctrl_v() {
    // 一次塞四个事件，很多应用（Win11 记事本的 WinUI 编辑框）会漏掉 V 或 Ctrl 的配对，
    // 表现为「粘贴了但什么都没进去」。分开发、留 10~25ms 间隔最稳。
    unsafe {
        let _ = SendInput(&[key_input(VK_CONTROL.0, false)], std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(std::time::Duration::from_millis(15));
        let _ = SendInput(&[key_input(0x56 /* V */, false)], std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(std::time::Duration::from_millis(25));
        let _ = SendInput(&[key_input(0x56, true)], std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(std::time::Duration::from_millis(15));
        let _ = SendInput(&[key_input(VK_CONTROL.0, true)], std::mem::size_of::<INPUT>() as i32);
    }
}

/// 优先直接给焦点控件发 WM_PASTE（Edit/RichEdit 都认），比模拟 Ctrl+V 少一层竞态。
fn send_paste_message() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, WM_PASTE};
    let Some(ctl) = focused_control() else { return false };
    let mut cls = [0u16; 64];
    let n = unsafe { GetClassNameW(ctl, &mut cls) } as usize;
    let class = String::from_utf16_lossy(&cls[..n]);
    let editable = class.eq_ignore_ascii_case("Edit")
        || class.eq_ignore_ascii_case("RichEdit20A")
        || class.eq_ignore_ascii_case("RichEdit20W")
        || class.eq_ignore_ascii_case("RichEdit50W")
        || class.eq_ignore_ascii_case("RichEditD2DPT")
        || class.starts_with("RichEdit");
    if !editable {
        return false;
    }
    send_timeout(ctl, WM_PASTE, WPARAM(0), LPARAM(0)).is_some()
}

fn key_input(vk: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

pub fn set_text_only(text: &str) -> bool {
    set_clipboard_text(text)
}

/// 返回 (status, detail)：ok / undelivered / unknown（与 macOS 三态同语义）。
pub fn paste(text: &str, mode: &str) -> (&'static str, String) {
    paste_to(text, mode, 0)
}

/// `target`：按下热键时的前台窗口。录音期间浮层可能把前台抢走，
/// 那就先把它设回前台再注入（I-113）。
pub fn paste_to(text: &str, mode: &str, target: isize) -> (&'static str, String) {
    if mode != "paste" {
        return type_text(text);
    }
    if text.trim().is_empty() {
        return ("undelivered", "没有可上屏的文本".into());
    }
    if target != 0 {
        unsafe {
            use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
            use windows::Win32::UI::WindowsAndMessaging::{
                GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
            };
            let want = windows::Win32::Foundation::HWND(target as *mut std::ffi::c_void);
            if GetForegroundWindow() != want {
                // 录音期间我们的浮层可能把前台抢走；自家进程是前台时 SetForegroundWindow 通常还能成功，
                // 失败就 attach 到前台线程再设一次（I-113）。
                if SetForegroundWindow(want).0 == 0 {
                    let fg = GetForegroundWindow();
                    let fg_tid = GetWindowThreadProcessId(fg, None);
                    let mine = GetCurrentThreadId();
                    let _ = AttachThreadInput(mine, fg_tid, true);
                    let _ = SetForegroundWindow(want);
                    let _ = AttachThreadInput(mine, fg_tid, false);
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        }
    }
    // 首选 UIA：直接写焦点元素，不依赖模拟键，写进后立刻回读（I-113）
    if let Some(r) = uia_set_value(text) {
        if r.0 == "ok" {
            return r;
        }
        if std::env::var("ERYU_INJECT_DEBUG").is_ok() {
            eprintln!("[eryu][inject] UIA 没成功（{}），退回剪贴板路径", r.1);
        }
    }
    // 前台是我们自己的窗口（设置/引导/托盘弹窗）时绝不往里打：既打不进去，也会把用户刚说的话粘到设置框里
    if foreground_is_self() && !is_noactivate_self() {
        if std::env::var("ERYU_INJECT_DEBUG").is_ok() {
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowTextW};
                let fg = GetForegroundWindow();
                let mut cls = [0u16; 128];
                let n = GetClassNameW(fg, &mut cls) as usize;
                let mut ttl = [0u16; 128];
                let m = GetWindowTextW(fg, &mut ttl) as usize;
                eprintln!(
                    "[eryu][inject] self-fg class={} title={} target={:#x}",
                    String::from_utf16_lossy(&cls[..n]),
                    String::from_utf16_lossy(&ttl[..m]),
                    target
                );
            }
        }
        return ("undelivered", "焦点在 Eryu 自己的窗口上，这段话留在剪贴板里了".into());
    }
    let before = get_clipboard_text();
    if !set_clipboard_text(text) {
        return ("undelivered", "写剪贴板失败（可能被别的程序占用）".into());
    }
    if std::env::var("ERYU_INJECT_DEBUG").is_ok() {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                GetClassNameW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
            };
            let fg = GetForegroundWindow();
            let mut cls = [0u16; 128];
            let n = GetClassNameW(fg, &mut cls) as usize;
            let mut ttl = [0u16; 128];
            let m = GetWindowTextW(fg, &mut ttl) as usize;
            let mut pid = 0u32;
            let tid = GetWindowThreadProcessId(fg, Some(&mut pid));
            eprintln!(
                "[eryu][inject] fg={:?} pid={pid} tid={tid} class={} title={}",
                fg.0,
                String::from_utf16_lossy(&cls[..n]),
                String::from_utf16_lossy(&ttl[..m])
            );
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(60));
    // 先用 Ctrl+V：它对 WinUI 记事本、浏览器输入框都有效（同机 PowerShell 已验证）。
    // WM_PASTE 只当兜底：WinUI 记事本内层是 RichEditD2DPT，收下 WM_PASTE 却不处理，
    // 先试它会「看起来粘贴了但什么都没进去」（I-113）。
    send_ctrl_v();
    std::thread::sleep(std::time::Duration::from_millis(400));
    let mut landed = read_focus_text().map(|cur| cur.contains(text)).unwrap_or(false);
    if !landed && send_paste_message() {
        std::thread::sleep(std::time::Duration::from_millis(300));
        landed = read_focus_text().map(|cur| cur.contains(text)).unwrap_or(false);
    }
    std::thread::sleep(std::time::Duration::from_millis(120));
    if std::env::var("ERYU_INJECT_DEBUG").is_ok() {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow};
            let fg = GetForegroundWindow();
            let mut cls = [0u16; 128];
            let n = GetClassNameW(fg, &mut cls) as usize;
            eprintln!("[eryu][inject] after-paste fg={:?} class={}", fg.0, String::from_utf16_lossy(&cls[..n]));
        }
    }
    // 不还原剪贴板：很多目标（WinUI 记事本、浏览器输入框）是延迟读剪贴板的，
    // 650ms 就还原会把还没被读走的内容换掉，表现为「粘贴了但什么都没进去」（I-113）。
    // 文本留在剪贴板里也正好是失败时的兜底。
    // 注入后校验：读一眼焦点控件里的文字。
    // 读得到且包含这段文字 → ok；读得到但没有 → unknown（绝不二次注入）；
    // 读不到（自绘控件/终端）→ 按 ok 处理，不能把「无法校验」当成失败。
    match wait_contains(text) {
        Landed::Yes => ("ok", String::new()),
        Landed::Unreadable => (
            "unknown",
            "已发送粘贴，但读不到目标输入框，无法确认是否落进去（文字在剪贴板里）".into(),
        ),
        Landed::No => {
            // 只落了一半是最常见的半成功：提示里给「可能没粘全」，用户自己去补。
            let head: String = text.chars().take(8).collect();
            let half = read_focus_text()
                .map(|cur| !head.is_empty() && cur.contains(&head))
                .unwrap_or(false);
            if half {
                ("unknown", "只看到这段文字的一部分落进输入框，可能没粘全".into())
            } else {
                ("unknown", "已发送粘贴，但输入框里没看到这段文字（目标可能不吃 Ctrl+V）".into())
            }
        }
    }
}

/// 给别的进程发同步消息：**必须带超时**。
/// 目标窗口的 UI 线程一挂，`SendMessageW` 会把我们自己的 UI 线程也永久拖住
/// （I-104 审计 P0-3），所以统一走 `SendMessageTimeoutW`（无响应就放弃校验）。
fn send_timeout(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<usize> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, SMTO_ABORTIFHUNG, SMTO_BLOCK,
    };
    let mut out: usize = 0;
    let r = unsafe {
        SendMessageTimeoutW(
            hwnd,
            msg,
            wp,
            lp,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            300,
            Some(&mut out as *mut usize),
        )
    };
    if r.0 != 0 {
        Some(out)
    } else {
        None
    }
}

/// 读焦点控件里的文本：先 WM_GETTEXT（对 EDIT/RichEdit 可靠）。
/// 读不到就返回 None（自绘控件/终端/对方卡住），由调用方按 ok 处理——不能把「无法校验」当失败。
/// 焦点控件的 HWND（给「编辑观察」用：上屏后要回头读同一个控件）
pub fn focused_control_hwnd() -> Option<isize> {
    focused_control().map(|h| h.0 as isize)
}

/// 取窗口类名（判断「焦点是不是落在桌面/任务栏」这种非输入控件上）
fn window_class(h: windows::Win32::Foundation::HWND) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
    unsafe {
        let mut buf = [0u16; 128];
        let n = GetClassNameW(h, &mut buf);
        if n <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

/// 取窗口所属进程的 exe 名（小写，如 "explorer.exe"）
fn window_process_name(h: windows::Win32::Foundation::HWND) -> String {
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    unsafe {
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(h, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }
        let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(p, PROCESS_NAME_FORMAT(0), windows::core::PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = windows::Win32::Foundation::CloseHandle(p);
        if ok.is_err() {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit('\\').next().unwrap_or("").to_lowercase()
    }
}

/// 取前台窗口的类名（桌面/任务栏有自己的一套类名，判断用）
fn foreground_window_class() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let fg = unsafe { GetForegroundWindow() };
    if fg.0.is_null() {
        return None;
    }
    window_class(fg)
}

/// 读某个控件当前文本（带超时；读不到返回 None）
pub fn read_text(hwnd: isize) -> Option<String> {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::{WM_GETTEXT, WM_GETTEXTLENGTH};
    let h = HWND(hwnd as *mut core::ffi::c_void);
    let n = send_timeout(h, WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0))?;
    if n == 0 || n >= 1_000_000 {
        return None;
    }
    let mut buf: Vec<u16> = vec![0; n + 1];
    let got = send_timeout(
        h,
        WM_GETTEXT,
        WPARAM(buf.len()),
        LPARAM(buf.as_mut_ptr() as isize),
    )?;
    if got == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..got.min(n)]))
}

/// 读焦点控件里的文字：先用 `WM_GETTEXT`（经典 Edit 一次到位），
/// 读不到再走 UIA（WinUI 记事本的内层 RichEditD2DPT、终端、浏览器输入框都只认 UIA）。
/// 只用于注入后校验，失败就当「无法确认」，绝不据此二次注入。
fn read_focus_text() -> Option<String> {
    focused_text().or_else(uia_focused_text)
}

/// UIA 读焦点元素的文字：`ValuePattern` 优先，其次 `TextPattern`（先光标所在行，再整篇）。
/// 整篇最多读 40 万字，避免超长文档把校验拖住。
pub fn uia_focused_text() -> Option<String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationValuePattern,
        TextUnit_Line, UIA_TextPatternId, UIA_ValuePatternId,
    };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let uia: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let focused = uia.GetFocusedElement().ok()?;
        if let Ok(vp) = focused.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
            if let Ok(v) = vp.CurrentValue() {
                let s = v.to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        let tp = focused
            .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
            .ok()?;
        if let Ok(sel) = tp.GetSelection() {
            if let Ok(r) = sel.GetElement(0) {
                if r.ExpandToEnclosingUnit(TextUnit_Line).is_ok() {
                    if let Ok(t) = r.GetText(-1) {
                        let s = t.to_string();
                        if !s.is_empty() {
                            return Some(s);
                        }
                    }
                }
            }
        }
        let doc = tp.DocumentRange().ok()?;
        doc.GetText(400_000).ok().map(|t| t.to_string())
    }
}

fn focused_text() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{WM_GETTEXT, WM_GETTEXTLENGTH};
    let hwnd = focused_control()?;
    let n = send_timeout(hwnd, WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0))?;
    if n == 0 || n >= 1_000_000 {
        return None;
    }
    let mut buf: Vec<u16> = vec![0; n + 1];
    let got = send_timeout(
        hwnd,
        WM_GETTEXT,
        WPARAM(buf.len()),
        LPARAM(buf.as_mut_ptr() as isize),
    )?;
    if got == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..got.min(n)]))
}

/// 逐字打字：优先 `PostMessageW(WM_CHAR)` 直接投给**有焦点的控件**
/// （SendInput + KEYEVENTF_UNICODE 实测在高 DPI WinForms 里只到前两个字就丢，
///  而且它依赖前台/输入队列状态；WM_CHAR 只要拿到 hwndFocus 就稳定）。
/// 拿不到 hwndFocus 时退回 SendInput，再不行返回 undelivered。
pub fn type_text(text: &str) -> (&'static str, String) {
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CHAR};

    if text.trim().is_empty() {
        return ("undelivered", "没有可上屏的文本".into());
    }

    // 前台是桌面/任务栏时，打字只会打空或打进搜索框（VM 里实测焦点会落到任务栏的
    // InputSite 输入框，看起来「成功」其实用户什么也看不见）。直接说没送出去。
    if let Some(fg_cls) = foreground_window_class() {
        if matches!(
            fg_cls.as_str(),
            "Shell_TrayWnd" | "TrayNotifyWnd" | "Progman" | "WorkerW" | "FolderView" | "SysListView32"
        ) {
            return (
                "undelivered",
                "焦点在桌面或任务栏上：先把光标放进要写字的输入框，再说一次".into(),
            );
        }
    }
    if std::env::var("ERYU_INJECT_DEBUG").is_ok() {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowTextW};
            let fg = GetForegroundWindow();
            let mut cbuf = [0u16; 128];
            let cn = GetClassNameW(fg, &mut cbuf) as usize;
            let mut tbuf = [0u16; 128];
            let tn = GetWindowTextW(fg, &mut tbuf) as usize;
            let fch = focused_control().and_then(window_class).unwrap_or_default();
            eprintln!(
                "[eryu][inject] type fg_class={} fg_title={} focus_class={fch}",
                String::from_utf16_lossy(&cbuf[..cn]),
                String::from_utf16_lossy(&tbuf[..tn])
            );
        }
    }
    if let Some(hwnd) = focused_control() {
        // 桌面的 FolderView / WorkerW 也会「有焦点」，但 WM_CHAR 打进去只会消失。
        // 这种时候明确说没送出去，别让用户以为已经上屏了（也避免历史里留一条莫名 pending）。
        if let Some(cls) = window_class(hwnd) {
            // Win11 的桌面图标层是 explorer 的 DirectUIHWND（在 Progman/WorkerW 里），
            // 所以这一类要连「它是不是 explorer 的窗口」一起看，避免误伤普通程序的 DirectUI。
            let shell_owned = cls == "DirectUIHWND" && window_process_name(hwnd).eq_ignore_ascii_case("explorer.exe");
            if shell_owned
                || matches!(
                    cls.as_str(),
                    "FolderView" | "Progman" | "WorkerW" | "SysListView32" | "Shell_TrayWnd" | "TrayNotifyWnd"
                )
            {
                return (
                    "undelivered",
                    "焦点在桌面或任务栏上：先把光标放到要写字的输入框，再说一次".into(),
                );
            }
        }
        let mut sent = 0usize;
        for unit in text.encode_utf16() {
            let ok = unsafe { PostMessageW(hwnd, WM_CHAR, windows::Win32::Foundation::WPARAM(unit as usize), windows::Win32::Foundation::LPARAM(1)).is_ok() };
            if ok {
                sent += 1;
            }
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
        if sent == 0 {
            return ("undelivered", "按键没能送出去（目标窗口可能是管理员权限）".into());
        }
        return verify_typed(text);
    }

    let r = send_unicode_input(text);
    if r.0 == "ok" {
        return verify_typed(text);
    }
    r
}

/// 逐字打字后回读校验（I-143）：`WM_CHAR`/`SendInput` 都可能被目标程序吞掉却仍返回成功，
/// 那就成了「界面说已上屏、输入框里其实没有」。读不到（返回 None）时不判否——有些控件本来就读不出；
/// 读到了但不含这段文字，就如实报 unknown，让用户自己决定复制还是重试。
fn verify_typed(text: &str) -> (&'static str, String) {
    let want: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if want.is_empty() {
        return ("ok", String::new());
    }
    // 逐字打进去是异步的：WM_CHAR / SendInput 只是排进目标线程的消息队列，
    // UWP/WinUI 的 RichEdit 还要再走一层合成，读太快会「文字明明在，却读不到」→ 判成 pending。
    match wait_contains(text) {
        Landed::Yes => ("ok", String::new()),
        Landed::Unreadable => ("ok", String::new()),
        Landed::No => (
            "unknown",
            "已经逐字打完，但输入框里没看到这段文字（目标程序可能不收模拟键，可以改用「粘贴」）".into(),
        ),
    }
}

/// 注入后回读的三种结果：读到了且包含 / 读到了但不包含 / 读不出（自绘或画布类控件）。
enum Landed {
    Yes,
    No,
    Unreadable,
}

/// 带重试的「目标里现在有这段文字吗」：UWP/WinUI 贴进去/打进去都在合成后才读得到，
/// 一次读不到不代表失败。最多等 ~700ms，读到一次就立刻返回。
fn wait_contains(text: &str) -> Landed {
    let want: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if want.is_empty() {
        return Landed::Yes;
    }
    let head: String = want.chars().take(8).collect();
    let mut saw_some = false;
    for attempt in 0..10 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(70));
        }
        let Some(got) = read_focus_text() else { continue };
        saw_some = true;
        if attempt == 0 && std::env::var("ERYU_INJECT_DEBUG").is_ok() {
            let short: String = got.chars().take(40).collect();
            eprintln!("[eryu][inject] verify #0 got={short:?}");
        }
        let have: String = got.chars().filter(|c| !c.is_whitespace()).collect();
        if have.contains(&want) || (!head.is_empty() && have.contains(&head)) {
            return Landed::Yes;
        }
    }
    if saw_some {
        Landed::No
    } else {
        Landed::Unreadable
    }
}

/// 前台线程当前有焦点的控件（跨进程可读）
/// 前台窗口是不是我们自己的进程（设置窗、引导窗、托盘弹窗都可能在前台）。
/// 自家的无激活窗口（迷你主窗、胶囊）：它们本来就不该抢焦点，不应因此判定「焦点在 Eryu 上」
fn is_noactivate_self() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
    };
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid != std::process::id() {
            return false;
        }
        let mut cls = [0u16; 64];
        let n = GetClassNameW(fg, &mut cls) as usize;
        let name = String::from_utf16_lossy(&cls[..n]);
        name == "EryuMini" || name == "EryuOverlay"
    }
}

fn foreground_is_self() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        pid == std::process::id()
    }
}

fn focused_control() -> Option<windows::Win32::Foundation::HWND> {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    };
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        let tid = GetWindowThreadProcessId(fg, Some(&mut pid));
        if tid == 0 {
            return None;
        }
        let me = GetCurrentThreadId();
        let _ = AttachThreadInput(me, tid, true);
        let mut gti = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let ok = GetGUIThreadInfo(tid, &mut gti).is_ok();
        let _ = AttachThreadInput(me, tid, false);
        if ok && !gti.hwndFocus.0.is_null() {
            Some(gti.hwndFocus)
        } else {
            None
        }
    }
}

fn send_unicode_input(text: &str) -> (&'static str, String) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY,
    };
    let units: Vec<u16> = text.encode_utf16().collect();
    for chunk in units.chunks(8) {
        let mut inputs: Vec<INPUT> = Vec::with_capacity(chunk.len() * 2);
        for u in chunk {
            for up in [false, true] {
                inputs.push(INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(0),
                            wScan: *u,
                            dwFlags: KEYBD_EVENT_FLAGS(
                                KEYEVENTF_UNICODE.0 | if up { KEYEVENTF_KEYUP.0 } else { 0 },
                            ),
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                });
            }
        }
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent == 0 {
            return ("undelivered", "按键没能送出去（目标窗口可能是管理员权限）".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    std::thread::sleep(std::time::Duration::from_millis(60));
    ("ok", String::new())
}

/// UIA 注入：把文字直接写进焦点元素（ValuePattern.SetValue）。
///
/// 这条路径不依赖模拟键，也不受「合成键被 WinUI 吞掉」影响（I-113），
/// 而且写进去之后能立刻回读校验，比「发完就猜」可靠得多。
/// 返回 None 表示「这个焦点元素不支持 ValuePattern」（比如自绘输入框、终端），
/// 调用方再退回剪贴板 + Ctrl+V。
pub fn uia_set_value(text: &str) -> Option<(&'static str, String)> {
    use windows::core::BSTR;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationValuePattern, UIA_ValuePatternId,
    };

    unsafe {
        // 幂等：已经初始化过会返回 S_FALSE/RPC_E_CHANGED_MODE，都忽略
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let uia: IUIAutomation =
            match CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
                Ok(v) => v,
                Err(e) => return Some(("unknown", format!("UIA 初始化失败：{e}"))),
            };
        let focused = match uia.GetFocusedElement() {
            Ok(f) => f,
            Err(_) => return None,
        };
        let pat: IUIAutomationValuePattern =
            match focused.GetCurrentPatternAs(UIA_ValuePatternId) {
                Ok(p) => p,
                Err(_) => return None,
            };
        if pat.CurrentIsReadOnly().map(|b| b.as_bool()).unwrap_or(true) {
            return None;
        }
        if let Err(e) = pat.SetValue(&BSTR::from(text)) {
            return Some(("unknown", format!("UIA 写入失败：{e}")));
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
        match pat.CurrentValue() {
            Ok(v) => {
                let cur = v.to_string();
                if cur.contains(text) {
                    ("ok", String::new()).into()
                } else {
                    ("unknown", "UIA 写了但回读不一致".to_string()).into()
                }
            }
            Err(_) => ("ok", String::new()).into(),
        }
    }
}
