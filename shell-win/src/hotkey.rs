//! 热键配置解析：与 macOS 侧同一套写法（`ALT_R` / `CTRL+ALT+SPACE` / `F13` / `ESC`…）。
//!
//! Windows 用 Raw Input：`VKey` 对左右修饰键不区分，靠 `RI_KEY_E0` 位补齐，
//! 所以先把「原始事件」归一化成有左右之分的虚拟键，再和配置比对。

use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

pub const VK_LMENU: u16 = 0xA4;
pub const VK_RMENU: u16 = 0xA5;
pub const VK_LCONTROL: u16 = 0xA2;
pub const VK_RCONTROL: u16 = 0xA3;
pub const VK_LSHIFT: u16 = 0xA0;
pub const VK_RSHIFT: u16 = 0xA1;
pub const VK_LWIN: u16 = 0x5B;
pub const VK_RWIN: u16 = 0x5C;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Token {
    Mod(u16),
    Key(u16),
}

#[derive(Clone, Debug)]
pub struct Spec {
    /// 组合键里的修饰键部分（单键时为空）
    pub mods: Vec<u16>,
    /// 主键：单修饰键热键时就是那个修饰键本身
    pub main: Token,
    /// 原始写法，用于日志/界面展示
    pub raw: String,
}

impl Spec {
    pub fn is_single_modifier(&self) -> bool {
        matches!(self.main, Token::Mod(_)) && self.mods.is_empty()
    }
}

fn parse_token(tok: &str) -> Option<Token> {
    let t = tok.trim().to_ascii_uppercase();
    let t = t.as_str();
    let m = |vk: u16| Some(Token::Mod(vk));
    let k = |vk: u16| Some(Token::Key(vk));
    match t {
        "ALT_R" | "RALT" | "RIGHT_ALT" | "OPTION_R" | "RIGHT_OPTION" => m(VK_RMENU),
        "ALT_L" | "LALT" | "LEFT_ALT" | "OPTION_L" | "OPTION" | "ALT" => m(VK_LMENU),
        "CTRL_R" | "RCTRL" | "RIGHT_CTRL" => m(VK_RCONTROL),
        "CTRL_L" | "LCTRL" | "CTRL" | "CONTROL" | "LEFT_CTRL" => m(VK_LCONTROL),
        "SHIFT_R" | "RSHIFT" => m(VK_RSHIFT),
        "SHIFT_L" | "LSHIFT" | "SHIFT" => m(VK_LSHIFT),
        "WIN_R" | "RWIN" | "CMD_R" => m(VK_RWIN),
        "WIN_L" | "LWIN" | "CMD" | "WIN" | "META" => m(VK_LWIN),
        "SPACE" => k(0x20),
        "TAB" => k(0x09),
        "ENTER" | "RETURN" => k(0x0D),
        "ESC" | "ESCAPE" => k(0x1B),
        "BACKSPACE" => k(0x08),
        "UP" => k(0x26),
        "DOWN" => k(0x28),
        "LEFT" => k(0x25),
        "RIGHT" => k(0x27),
        _ => {
            if let Some(rest) = t.strip_prefix('F') {
                if let Ok(n) = rest.parse::<u16>() {
                    if (1..=20).contains(&n) {
                        return k(0x70 + n - 1); // VK_F1 = 0x70
                    }
                }
            }
            if t.len() == 1 {
                let c = t.chars().next().unwrap();
                if c.is_ascii_uppercase() {
                    return k(c as u16); // VK_A..VK_Z 就是 ASCII
                }
                if c.is_ascii_digit() {
                    return k(c as u16); // VK_0..VK_9 就是 ASCII
                }
            }
            None
        }
    }
}

pub fn parse(spec: &str) -> Option<Spec> {
    // 分隔符两边的空段一律算写错：`CTRL+` 会被静默当成「单个 Ctrl」，用户以为
    // 配了组合键、实际按一下 Ctrl 就开始录音（I-162）。
    let raw_parts: Vec<&str> = spec.split('+').map(|p| p.trim()).collect();
    if raw_parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let parts: Vec<&str> = raw_parts.into_iter().filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    let mut mods = Vec::new();
    let mut main = None;
    for (i, p) in parts.iter().enumerate() {
        let tok = parse_token(p)?;
        let last = i == parts.len() - 1;
        match (tok, last) {
            (Token::Mod(vk), true) => main = Some(Token::Mod(vk)),
            (Token::Mod(vk), false) => mods.push(vk),
            (Token::Key(vk), true) => main = Some(Token::Key(vk)),
            (Token::Key(_), false) => return None, // 主键必须在最后
        }
    }
    Some(Spec { mods, main: main?, raw: spec.to_string() })
}

/// 把 Raw Input 的一条记录归一化成「带左右的虚拟键」
pub fn normalize(vkey: u32, is_e0: bool) -> u16 {
    match vkey as u16 {
        0x12 => if is_e0 { VK_RMENU } else { VK_LMENU },
        0x11 => if is_e0 { VK_RCONTROL } else { VK_LCONTROL },
        0x10 => VK_LSHIFT, // Raw Input 不带左右信息，统一按左 Shift（右 Shift 需要 MapVirtualKey + 扫描码）
        0x5B => VK_LWIN,
        0x5C => VK_RWIN,
        other => other,
    }
}

pub fn debug_down(vk: u16) -> bool {
    down(vk)
}

fn down(vk: u16) -> bool {
    unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

/// 组合键里的修饰键按「左右都算」判断：mac 侧的 `CTRL+ALT+SPACE` 也是不分左右的，
/// 否则用户录的是右 Ctrl、配的却是左 Ctrl 词，按下去永远不触发。
fn mod_down(vk: u16) -> bool {
    match vk {
        VK_LCONTROL | VK_RCONTROL => down(VK_LCONTROL) || down(VK_RCONTROL),
        VK_LMENU | VK_RMENU => down(VK_LMENU) || down(VK_RMENU),
        VK_LSHIFT | VK_RSHIFT => down(VK_LSHIFT) || down(VK_RSHIFT),
        VK_LWIN | VK_RWIN => down(VK_LWIN) || down(VK_RWIN),
        other => down(other),
    }
}

/// 单修饰键热键（ALT_R / CTRL_L …）按「同一类修饰键」判断：
/// 物理键盘和合成键报上来的左右码不一定一致（VM 里 RAlt 实际到 0xA4），
/// 用户也不关心是左还是右 —— 只要按的是那个修饰键就算。
fn same_modifier(spec: u16, vk: u16) -> bool {
    match (spec, vk) {
        (VK_LMENU | VK_RMENU, VK_LMENU | VK_RMENU) => true,
        (VK_LCONTROL | VK_RCONTROL, VK_LCONTROL | VK_RCONTROL) => true,
        (VK_LSHIFT | VK_RSHIFT, VK_LSHIFT | VK_RSHIFT) => true,
        (VK_LWIN | VK_RWIN, VK_LWIN | VK_RWIN) => true,
        (a, b) => a == b,
    }
}

impl Spec {
    /// 这条按键事件是否命中本热键；`is_break` 为 true 表示松开
    pub fn matches(&self, vk: u16, is_break: bool) -> bool {
        let is_main = match self.main {
            Token::Mod(m) => same_modifier(m, vk),
            Token::Key(k) => k == vk,
        };
        if !is_main {
            return false;
        }
        if self.mods.is_empty() {
            return true;
        }
        if is_break {
            // 松开主键就算结束（不要求修饰键还按着）
            return true;
        }
        self.mods.iter().all(|m| mod_down(*m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_modifier() {
        let s = parse("ALT_R").unwrap();
        assert!(s.is_single_modifier());
        assert_eq!(s.main, Token::Mod(VK_RMENU));
    }

    #[test]
    fn parses_combo() {
        let s = parse("CTRL+ALT+SPACE").unwrap();
        assert_eq!(s.mods, vec![VK_LCONTROL, VK_LMENU]);
        assert_eq!(s.main, Token::Key(0x20));
    }

    #[test]
    fn parses_function_key_and_letter() {
        assert_eq!(parse("F13").unwrap().main, Token::Key(0x7C));
        assert_eq!(parse("H").unwrap().main, Token::Key(b'H' as u16));
    }

    #[test]
    fn normalizes_right_alt() {
        assert_eq!(normalize(0x12, true), VK_RMENU);
        assert_eq!(normalize(0x12, false), VK_LMENU);
        assert_eq!(normalize(0x11, true), VK_RCONTROL);
    }

    #[test]
    fn single_modifier_matches_either_side() {
        let s = parse("ALT_R").unwrap();
        assert!(s.matches(VK_RMENU, false));
        assert!(s.matches(VK_LMENU, false));
        assert!(!s.matches(VK_RCONTROL, false));
    }

    #[test]
    fn rejects_main_key_not_last() {
        assert!(parse("SPACE+ALT").is_none());
        assert!(parse("").is_none());
    }

    /// 设置页保存前会拿这组输入做校验：看不懂的组合键必须被拒，别静默退回 ALT_R
    #[test]
    fn rejects_garbage_specs() {
        for bad in ["CTRL+", "CTRL+FOO", "SHIFT++A", "   ", "ALT_R+", "CTRL+ALT+"] {
            assert!(parse(bad).is_none(), "应当拒绝：{bad:?}");
        }
        for good in ["ALT_R", "CTRL+ALT+SPACE", "F13", "ALT+SPACE", "SHIFT+F5"] {
            assert!(parse(good).is_some(), "应当接受：{good:?}");
        }
    }
}
