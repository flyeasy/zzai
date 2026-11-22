# Windows 1:1 实现计划（M3）

> 目标：Windows 版在**能力和细节上与 macOS 版一比一**——同一套交互（按住说话、浮层流式、松手上屏、
> 失败可见、可重试、可审计），同一套自测与发版闸门，只在"平台怎么做"上不同。
>
> 现状：**未开工**。本机只有 `aarch64-apple-darwin` 目标，没有 mingw/MSVC/Windows SDK，也没有 Windows 机器，
> 所以任何"已支持 Windows"的说法在此之前都不成立（I-028）。

---

## 1. "一比一"的判定标准

不是"能跑就行"，而是四层都对得上：

| 层 | 判定 | 对应 macOS 现状 |
| --- | --- | --- |
| 功能 | 每条 R 需求在 Windows 上有等价实现 | `docs/REQUIREMENTS.md` R-001…R-076 |
| 交互细节 | 按住语义、浮层出现/跟随/收起时机、提示文案、失败后的三种去处（剪贴板/重试/历史）一致 | `docs/USAGE.md`、`docs/ERRORS.md` 24 个码 |
| 失败可见 | 每个错误有码、有中文文案、有下一步；不允许静默丢字 | `docs/ERRORS.md` |
| 自动化 | 同一套命令能自测：`--self-test` / `--contract` / `--acceptance-synthetic` / `--simulate` / `--soak` / `--bench` | `scripts/smoke.sh` 30 项、`scripts/release_gate.sh` |

---

## 2. 架构：core 全复用，只重写壳

```
core/  (Rust, 已有) ── cdylib eryu_core.dll ──┐
                                              ├── C ABI（eryu_core.h，v1.13）
shell/ (Windows 壳, 新建) ────────────────────┘
    ├── WinUI 3 + C#（.NET 8）  设置窗口 / 历史 / 统计 / 诊断
    ├── Win32 薄层（P/Invoke）  托盘、浮层、热键、注入
    └── 或者全 C# 实现，Rust 只做听写内核
```

- `core/Cargo.toml` 的 `crate-type` 加 **`cdylib`**（现在只有 `staticlib`+`rlib`；macOS 上刻意删过 cdylib，
  因为 Swift 链 dylib 会引入 `/Users` 绝对路径——Windows 侧相反，`cdylib` 更合适）。
- 目录：`%APPDATA%\Eryu\`（`config.toml` / `hotwords.toml` / `profiles.toml` / `history.json` / `stats.json` /
  `models\` / `pending\`）；`ERYU_CONFIG_DIR` 继续可用于自测隔离。
- 文件权限：Windows 没有 0600，`fsutil::restrict` 已是 no-op；改为**用户级 ACL**（`%APPDATA%` 本身仅本用户可读），
  敏感文件（`voice_print.bin`、`pending\*.txt`）额外用 `CryptProtectData`（DPAPI，用户绑定）加密落盘。
- 单实例：`CreateMutex("Global\\Eryu.Single")`；观测型 CLI 同样不参与（与 macOS 对齐）。

---

## 3. 键盘：Windows 没有"按住语义"的现成 API，这是最大的差异

macOS 用 `CGEventTap(listenOnly)` 拿 `flagsChanged` 判按住/松开。Windows 三条路：

| 方案 | 能拿到 keyUp | 能区分左右 Alt | 需要管理员 | 代价 |
| --- | --- | --- | --- | --- |
| `RegisterHotKey` | ❌ 只有按下 | ❌ | 否 | 不能做"按住说话"，只能做触发 |
| `WH_KEYBOARD_LL` 低级钩子 | ✅ | ✅（`vkey==VK_MENU` + `scancode==0x38`/`E0 38`） | 否 | 回调必须 <10ms，否则系统静默摘掉钩子；会看到所有按键（隐私敏感） |
| **Raw Input（`WM_INPUT`）** | ✅ | ✅（`RAWKEYBOARD.MakeCode` + `Flags.E0`） | 否 | 需要一个消息循环窗口；**不吞键**（与 macOS listenOnly 行为一致） |

**选择：Raw Input 为主（`RIDEV_INPUTSINK` 让后台也能收），LL 钩子作为兜底（仅当 Raw Input 拿不到时）。**

细节必须逐条对齐 macOS：

- 按下即进入 `starting`，浮层立刻出现（不等第一个音频包）；
- 松开即 `Finish`（`pending_stop` 语义、抖动合并 300ms、冷却 450ms 全部在 core 里，与平台无关）；
- **忽略 auto-repeat**（`RAWKEYBOARD.Flags` 里 `RI_KEY_BREAK` 缺失而重复到达的，用时间/状态位去重）；
- 修饰键按下/松开的**左右区分**：`MakeCode 0x38` + `E0` = 右 Alt；`0x1D`+`E0` = 右 Ctrl；
  单键热键因此可以绑"右 Alt / 右 Ctrl / F13"等；
- **不吞键**：Raw Input 只观察，所以「右 Option/右 Alt」仍会被前台应用看到——这与 macOS 现状一致
  （我们从未吞键）；如果想吞，只有 LL 钩子能做到，首版不做（记为差异，写进 USAGE）；
- Esc 取消：同一条 Raw Input 通道收 `VK_ESCAPE`，仅 `starting|listening` 生效；
- 组合键槽位（润色档 `CTRL+ALT+1..3`）：Raw Input 侧维护修饰键状态位，匹配到即触发；不注册系统热键，
  避免与别的软件抢（与 macOS ProfileHotkeys 一致）；
- **冲突检测**：`RegisterHotKey` 试注册再注销可用于探测"这个组合被系统/别的软件占了"，给出中文原因
  （对应 macOS 的 `eventHotKeyExistsErr` 文案：「⌘Space 被聚焦搜索占用」→ Windows「Win+Space 被输入法切换占用」）。

---

## 4. 权限：没有 TCC，但有五道不同的门

| 门 | 表现 | 处理 |
| --- | --- | --- |
| **麦克风隐私开关** | 设置 → 隐私 → 麦克风；桌面应用受"允许桌面应用访问麦克风"总开关控制 | 启动时 `capabilities` 探测 + 试开 0.2s 流；失败给「去设置」深链 `ms-settings:privacy-microphone` |
| **UIPI（完整性级别）** | 目标窗口比我们权限高（管理员进程）时，`SendInput` **静默丢弃** | 注入前比较 `TokenIntegrityLevel`；不给管理员权限就降级为「已复制，请手动粘贴」并说明原因（绝不静默丢字） |
| **Secure Desktop** | UAC 弹窗、锁屏时无法注入/收键 | 检测 `OpenInputDesktop` 失败 → 提示「当前在安全桌面，无法上屏」，录音结果留在剪贴板/历史 |
| **SmartScreen / Defender** | 未签名 exe 首次运行被拦、"未知发布者" | 自签证书 + 文档说明；不诱导用户关杀软；提供 sha256 校验 |
| **防火墙** | 局域网听写接口首次监听会弹窗 | 首次开启 LAN 时明确提示「Windows 会问是否允许网络访问，选专用网络即可」；仅绑 `0.0.0.0` 时申请规则，不允许时降级只监听 127.0.0.1 |

> 与 macOS 的关键差别：macOS 的辅助功能/输入监控是**一次性授权后长期有效**（按路径记），
> Windows 没有等价物——Raw Input 不需要授权，注入不需要授权，只有 UIPI 是硬边界。这反而是好事：
> 用户不用点三次授权，但**必须**在文档里写清"对管理员窗口无效"。

---

## 5. 上屏：三态校验必须一致

- **粘贴（默认）**：快照剪贴板（枚举所有格式 + `GlobalLock` 拷贝；延迟渲染格式放弃并记为"无法保全"）→
  `SetClipboardData` 写文本 + 记 `GetClipboardSequenceNumber` → `SendInput` 发 `Ctrl+V`
  （`KEYEVENTF_SCANCODE` 更兼容游戏/远程桌面）→ 350ms 后用 **UI Automation** 读焦点控件
  `TextPattern`/`ValuePattern`，判定 `ok / unknown / undelivered` 三态（与 `Injector.verify` 同语义）→
  `ok` 才在 750ms 后还原剪贴板（并要求序列号未变），否则**保留**。
- **逐字（keystroke）**：`SendInput` + `KEYEVENTF_UNICODE`（每 16 个 UTF-16 单元一批，代理对不劈开），
  安全桌面/UIPI 下明确报失败；IME 组合态会干扰，文档写明"中文输入法组合中请用粘贴模式"。
- **失败兜底**：`undelivered`/`unknown` → 写剪贴板 + 托盘提示「没上屏（原因）：已复制到剪贴板，也可从菜单取回」
  （与 D-085 完全一致）；**绝不二次注入**（终审①）。
- **单段重试结果不自动上屏**（D-040）：只进剪贴板 + 提示。

---

## 6. 浮层：Win32 分层窗口 + DWM

- 窗口样式：`WS_POPUP` + `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST`，
  `SetLayeredWindowAttributes`（或 `UpdateLayeredWindow` 做逐像素 alpha）；
- 圆角/材质：Win11 `DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE, ROUND)`，
  背景直接画实色（与 D-107 同一结论：**不要依赖系统材质**，颜色会随主题变）；
- 位置：UI Automation `TextPattern2.GetCaretRange` → `GetBoundingRectangles`（屏幕坐标）；拿不到退
  `GetGUIThreadInfo().hCaret` + `GetCaretPos` + `ClientToScreen`；再退前台窗口底边中点；
  多屏/DPI 用 `MonitorFromPoint` + `WM_DPICHANGED`；
- 行为逐条对齐：进 `starting` 立刻显示、宽度只增不减 24pt 台阶、中心不动、0.16s 缓动、
  整段录音位置锁定、底部塞不下翻到光标上方、`_` 闪烁游标、空文本居中、电平条 14–24pt 浅橘黄、
  档位图标 18pt；
- 点击穿透：`WS_EX_TRANSPARENT` + `SetWindowLong` 的 `WS_EX_NOACTIVATE`；
- 收尾：`processing → inserting → idle` 延迟 0.6s 收起，`showError` 3s 自动收。

---

## 7. 托盘 / 设置 / 历史

- 托盘：`Shell_NotifyIcon` + `TrackPopupMenu`，菜单项与 macOS 的 12 项一一对应（状态行、详情、设置、
  统计与历史、复制上次结果、重试上次失败、选区改写、诊断、退出）——测麦/权限/自测这类排障口都在设置里，不进托盘；
  `NIF_TIP` 显示阶段；图标随状态换（闲置/录音/识别/上屏四态，等价 D-047）。
- 设置窗口：**WinUI 3 + C#**（.NET 8）——与 SwiftUI 的心智最接近、中文排版好、支持深浅色与 DPI；
  六个分页（通用/引擎/音频/热词/润色/历史/诊断）与 macOS 同结构、同文案、同开关顺序。
  （不选 egui：中文质感与控件细节达不到"一比一"；不选 WebView：与"无 WebView、包体小"的定位冲突。）
- 历史/统计：直接复用 core 的 `history.json` / `stats.json`；列表、搜索、删除、清空、p50/p95 卡片一致。

---

## 8. 音频 / 模型 / 网络

- 采集：`cpal` → WASAPI 共享模式（任意采样率/声道，core 的重采样器已与输入率解耦）；
- 设备热拔插：`WM_DEVICECHANGE`（`DBT_DEVICEQUERYREMOVE`）→ 走与 macOS `mic_unplugged` **同一条**收尾路径
  （保住已录音频、立即解码）；
- 默认设备切换：`IMMNotificationClient` 或每次开流时重新查默认设备（macOS 是每次开流重查，保持一致）；
- 降噪：GTCRN 16k（`gtcrn_simple.onnx` 535KB），采集链顺序一致：混单声道 → 重采样 16k → GTCRN → 入列；
- 模型下载：Win10 1803+ 自带 `curl.exe` 与 `tar.exe`，与 macOS 的 `models.rs` 命令等价；
  若企业策略禁用，退回"手动下载 + 放进 `%APPDATA%\Eryu\models`"的说明；
- 局域网听写：`std::net::TcpListener` 直接复用；mDNS（`.local` 免记 IP）用 `mdns-sd` crate 广播
  `_eryu._tcp`；手机端网页版 HTML 原样复用。

---

## 9. 媒体暂停 / 睡眠 / 开机启动

- 暂停其他播放器：WinRT `GlobalSystemMediaTransportControlsSessionManager` 枚举会话 → `TryPauseAsync`；
  失败退 `SendInput(VK_MEDIA_PLAY_PAUSE)`；只恢复自己暂停过的（与 `MediaPause.swift` 同语义）。
- 睡眠/唤醒：`WM_POWERBROADCAST`（`PBT_APMSUSPEND` 取消录音并保住已录、`PBT_APMRESUMEAUTOMATIC` 重挂热键+刷新设备）。
- 开机启动：`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，值为带引号的 exe 全路径；
  开关状态以注册表实际值为准（避免"设置里开着但没生效"）。

---

## 10. 功能对照表（1:1 清单）

| # | macOS 能力 | Windows 实现 | 验收 |
| --- | --- | --- | --- |
| 1 | 单键按住说话（右 Option） | Raw Input 观察 `VK_MENU`+`E0`，含 keyUp | `--hotkey-selftest` |
| 2 | 任意组合键 | Raw Input 修饰位 + 主键匹配 | `--slot-test` |
| 3 | Esc 取消 | Raw Input `VK_ESCAPE` | `ERYU_SIM_ESC=1` 等价 |
| 4 | 浮层（胶囊 + 电平 + 流式 + 游标） | 分层窗口 + DWM，逐条对齐视觉/时序 | `--demo-overlay` 截图 |
| 5 | 粘贴上屏 + 三态校验 | 剪贴板快照 + `SendInput` + UIA | `--verify-test` |
| 6 | 逐字上屏 | `KEYEVENTF_UNICODE` 分批 | `--type-event-test` |
| 7 | 失败兜底（剪贴板 + 提示） | 同 | `--fallback-test` |
| 8 | 结果先落盘、崩溃取回 | core 已有 + DPAPI 加密 | `--print-last` |
| 9 | 渐进封口（长听写 ≤1.5s） | core 已有 | `--simulate` 长语料 |
| 10 | 标点 / 热词 / 润色档 / 选区改写 | core 已有；选区用 UIA `TextPattern` 取选区与写回 | `--selection-test` |
| 11 | 声纹门控 / 音源锁定 | core 已有 | `switch_matrix` |
| 12 | 首启缺模型引导下载 | 同 | `--pull-model` |
| 13 | 设置/历史/统计/诊断 | WinUI 3 六分页 | `--diagnose` |
| 14 | 局域网听写 + 网页版 | 同 core + 防火墙提示 + mDNS | `lan.sh` |
| 15 | 单实例 / 睡眠 / 开机启动 / 媒体暂停 | 互斥体 / 电源消息 / Run 键 / SMTC | 各自脚本 |
| 16 | 自测与发版闸门 | 同一批 CLI（PowerShell 版脚本） | `gate.ps1` |

---

## 11. 里程碑与顺序

| 阶段 | 内容 | 出口标准 |
| --- | --- | --- |
| **W0**（1–2 天） | core 加 `cdylib`、`cfg(windows)` 审计、APPDATA 路径、DPAPI、GitHub Actions `windows-latest` 跑通 `cargo test` | CI 绿 + `--self-test` 在 Windows 上过 |
| **W1**（3–5 天） | 最小壳：托盘 + Raw Input 按住说话 + 粘贴上屏 + 浮层（实色）+ 单实例 | 按住 → 浮层 → 松手 → 目标应用出字 |
| **W2**（3–5 天） | 对齐 M1b/M2：设置窗口六分页、历史/统计、失败三分支、模型下载、热词、润色档、标点 | `--contract` + 手动逐项 |
| **W3**（2–4 天） | Windows 特有：UIPI 降级、安全桌面、防火墙、媒体暂停、开机启动、DPI/多屏 | 在管理员窗口里得到"已复制"而不是丢字 |
| **W4**（2–3 天） | 闸门脚本 `gate.ps1`、soak、崩溃恢复、LAN、签名与打包 | 与 macOS 同等级的自测覆盖 |

> 前提：**必须有一台 Windows 机器**（或 Parallels/VM + 真麦克风）。没有机器就不开工——这与 I-028 一致。

---

## 12. 风险与对策

| 风险 | 对策 |
| --- | --- |
| 低级钩子被系统摘掉/杀软报警 | 主路径用 Raw Input（不挂钩子）；钩子只作兜底并带健康检查 |
| UIPI 静默丢字 | 注入前判完整性级别，判定不可注入就「已复制」，并在设置页给出说明 |
| 浮层在游戏/全屏下不显示 | 试 `WS_EX_TOPMOST` + `SetWindowDisplayAffinity` 限制；文档说明全屏独占游戏可能遮不住 |
| DPI 变化导致浮层错位 | 全程用物理像素 + `WM_DPICHANGED` 重算；截图自测在 100%/150%/200% 各跑一次 |
| 中文输入法干扰逐字模式 | 文档主推粘贴模式；逐字模式检测 IME 组合态并提示 |
| 未签名被 SmartScreen 拦 | 自签 + sha256 + 文档；不引导关安全中心 |
| Windows 上 `curl`/`tar` 被策略禁 | 提供手动放置模型的路径与校验命令 |

## 13. 刻意不做

- **TSF 输入法**：我们是"注入文本"，不做候选窗/编码上屏（与 macOS 不做 IME 一致）；
- **驱动级虚拟麦克风 / AEC**：与 macOS 同理（I-046），不做 process loopback 的实时 AEC；
- **ARM64 首版**：先 x64（用户面最大），ARM64 视 Windows on ARM 份额再补；
- **多用户/服务化**：单用户桌面应用。

---

## 14. 参考

- `docs/DECISIONS.md` D-039（core 便携化）、D-108（本计划）
- `docs/ISSUES.md` I-028（无 Windows 机器无法验证）、I-045（未公证）
- 行为对照表（逐条 case）：见本文件 §9 与 `docs/USAGE.md`

## W1 实机验证记录（2026-11-19，家里那台 Windows 11）

真机：Windows 11 专业版 10.0.26200 / Intel Ultra 7 265K（20 核）/ 47.4GB RAM / 主机名 `dreammaker-win`。
通过 SSH 远程驱动，会话 1（console，用户 flyea）执行 GUI。

**跑通的**
- core 原生编译：`rustc 1.100.0` + MSVC BuildTools，`cargo build --release` 11 秒，产出 `eryu_core.dll` 21.5MB。
- sherpa 静态库：`sherpa-onnx-v2.0.1-win-x64-static-MD-Release-lib`，`SHERPA_ONNX_LIB_DIR` 必须指到解包目录下的 `lib\`。
- 采音：WASAPI 拿到「麦克风 (Steam Streaming Microphone)」，3 秒录音正常。
- 识别：`--decode-wav` 跑 16k 单声道样本 → `明天下午三点会议室评审设计文档。`（16 字全对，识别 76ms）。
- 上屏：剪贴板 + `SendInput` Ctrl+V 粘进记事本成功（截图确认）。
- 浮层：分层窗口 + 预乘 alpha + 圆角 14 + 橙色电平条 + 中文文字，截图确认可见。
- 热键：Raw Input `RIDEV_INPUTSINK` 收到右 Alt 按下/松开，驱动完整录音流程。
- 托盘：`Shell_NotifyIconW` 图标 + 右键菜单（复制上次结果 / 诊断 / 退出）。

**真机上踩到的坑（都已修，写进 I-061…I-064）**
- 路径反斜杠：`config.toml` 里 `dir = "~\AppData\…"` 未转义 → 整份配置解析失败（I-061）。
- `~` 缩略：Windows 上不该把 `C:\Users\x` 缩成 `~`，`expand()` 也只认 `~/`（I-062）。
- 分层窗口画中文：ClearType 在透明面上会糊成双影 → 先用不透明 DIB 画字，再按亮度转 alpha 合成（I-063）。
- `windows` 0.58 的句柄类型：`HGLOBAL(h.0)` / `HANDLE(h.0)` 显式构造；`DefWindowProcW` 是 Rust ABI，必须自定义 `extern "system"` 转调。
- SSH 进程在 session 0，看不到桌面：GUI 测试必须 `schtasks /Create /IT` 在会话 1 跑（I-064）。

## W2 实机验证记录：浮层跟随输入光标（2026-11-19）

目标：按下热键时浮层出现在**插入光标正下方**（不是屏幕底部），并在光标靠近屏幕下沿时翻到上方。

**实现的**
- `shell-win/src/caret.rs`：`caret_anchor()` 走 `GetForegroundWindow` → `GetWindowThreadProcessId` → `GetGUIThreadInfo`（读 `hwndCaret` + `rcCaret`）→ `ClientToScreen`；读不到光标时回退「前台窗口底边中心」，再退「鼠标位置」。
- `overlay.rs` 在 `starting`/`listening` 时算一次锚点并锁死（同一轮不再漂），水平居中对齐光标 x、垂直放光标下 8pt，超出屏幕则夹紧 / 翻到光标上方 28pt。
- `main.rs` 启动即 `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`；实测那台机器是 100% 缩放（`--caret-probe` 打印 `DPI scale=1.00`）。
- 胶囊宽度改成 **GDI 实测字形宽度**（`GetTextExtentPoint32W`，17px Microsoft YaHei Semibold）：`w = clamp(measure+76, 170, 420)`；超长时二分找最长「尾部片段」，保证看到的是最新几个字。

**真机读数（WinForms 窗口，文本框内容「这是一行测试文字，光标应该在后面」）**
- `PROBE fg_thread=159652 gti_ok=true err=0 caret_hwnd=330006 rc=(308,1,309,26) screen=(1248,635) conv=true win=(900,536,1660,856)`
- `CARET x=1248 y=635 flip=false` → 浮层出现在 (1248, 643)，截图 `docs/overlay-win-caret.png` 与光标同行对齐。

**这一轮修掉的三个 bug**
1. `GetWindowThreadProcessId` 的**返回值**才是线程 id，第二个参数是进程 id；一开始把进程 id 当线程 id 传进 `GetGUIThreadInfo` → 一直 `err=87 ERROR_INVALID_PARAMETER`、`gti_ok=false`（I-065）。
2. 高 DPI 下不声明感知，GDI 按逻辑坐标画字 → 文字被压缩到一半宽、和量出来的宽度对不上（I-066）。
3. 在**预乘 alpha 的透明位图**上画 ClearType 文字 → 彩边/发糊（上一版用亮度转 alpha 补救，仍有 1px 虚边）。改成「先当不透明位图整块画（背景+电平条+文字），最后统一乘 alpha 转预乘」→ 字实、圆角干净（I-063 的最终解）。

## W2 实机验证记录：逐字打字上屏（2026-11-19）

- `shell-win/src/inject.rs::type_text()`：先取前台线程的**焦点控件**（`GetGUIThreadInfo(...).hwndFocus`，跨进程可读，读前 `AttachThreadInput` 再解绑），然后逐 UTF-16 单元 `PostMessageW(hwnd, WM_CHAR, unit, 1)`，间隔 3ms；`hwndFocus` 读不到时退回 `SendInput + KEYEVENTF_UNICODE`（每批 8 单元）。
- 路由：`submit_request` 事件里 core 已经按配置算好 `mode`（`paste` / `keystroke`，含按应用覆盖），壳只是执行；非 `paste` 直接走 `type_text`。
- 实机验证（WinForms 窗口 + TextBox 聚焦）：`--inject-type "你好 Eryu 42 中文标点测试"` → TextBox 回读 `typed=[你好 Eryu 42 中文标点测试]`，中英混排、空格、中文标点全部正确。
- 踩到的两个坑写进 I-068：①`SendInput + KEYEVENTF_UNICODE` 在这台机器上只送到前两个字，且不报错（`SendInput` 返回值仍是请求数量）；②PowerShell `-ArgumentList` 会把含空格的参数拆开，测试脚手架必须自己加引号。

## W2 实机验证记录：热键可配置（2026-11-19）

macOS 侧的热键写法（`ALT_R` / `CTRL+ALT+SPACE` / `F13` / `ESC`）在 Windows 壳里同样生效，**从 core 的配置读**，不再是写死右 Alt。

- `shell-win/src/hotkey.rs`：解析写法 → `Spec { mods, main }`，主键必须在最后；单修饰键（`ALT_R`）走「单键按住说话」，功能键/字母/数字走「单键」，组合键在按下主键时用 `GetAsyncKeyState` 查修饰键是否都按着（松开只认主键，不要求修饰键还按着）。
- **热键来源换成低级键盘钩子**（`shell-win/src/hotkey_hook.rs`，`WH_KEYBOARD_LL`）：实测 `RegisterRawInputDevices(RIDEV_INPUTSINK)` 返回成功，但合成键一条 `WM_INPUT` 都不来（自测不可行）；钩子对合成键同样回调、带 `LLKHF_EXTENDED`（分左右 Alt/Ctrl）与 `LLKHF_UP`（松开），正好是「按住说话」需要的语义。代价：回调里只做判定 + 投递，绝不阻塞。
- 实机验证：把 `[hotkey] keys` 改成 `CTRL+ALT+SPACE` → `--hotkey-report` 输出 `HOTKEY raw=CTRL+ALT+SPACE mods=[162, 164] main=Key(32)`；合成 `Ctrl+Alt+Space` 后日志依次收到 `state starting → listening → meter{rms,secs}`，再松开正常收尾；把热键改成 `F13` → 报告 `main=Key(124)`。
- 新增 CLI：`--hotkey-report`（打印当前解析结果，改配置后一眼确认）。
- 调试开关：环境变量 `ERYU_RAW_DEBUG=1` 时把每次按键、每条 core 事件写 `%APPDATA%\Eryu\raw.log`（GUI 模式没有控制台，stderr 会丢）。

**W2 交付物**：`Eryu-Windows-W2.zip`（exe 295KB + dll 21.5MB + 使用说明，8.3MB，sha256 `8e459ad005290d7823a0d3af668b34770ff1d5f7af67bada9972218b1ce579d6`），2026-11-19 已发 [redacted-email] 与 [redacted-email]。

**W2 实机验证：按应用覆盖上屏方式**

- 壳在按下热键时把前台进程名（`Notepad.exe` 这种）经 `eryu_note_foreground_app` 交给 core，core 用 `config::mode_for()` 决定这一次用 `paste` 还是 `keystroke`，再随 `submit_request` 的 `mode` 字段回到壳。
- 真机验证（`eryu-shell-win.exe --decode-wav C:\win-hello.wav --as-app Notepad.exe`，配置里 `per_app = {"Notepad.exe" = "keystroke"}`）：
  基线（没传 `--as-app`）→ `submit_request: app="", mode="paste"`；
  传了 `--as-app Notepad.exe` → `submit_request: app="Notepad.exe", mode="keystroke"`。
- 顺手修掉 **I-070**：`per_app` 以前恒被写成 `{}`，覆盖项重启即丢。

**还没做（W2 起）**
- UIA 注入校验、图形设置界面（现在只能改 TOML）、安装包。
- 逐字打字上屏、按应用覆盖、UIA 注入校验。
- 设置界面（现在只能改 `%APPDATA%\Eryu\config.toml`）、热键自定义、润色档管理。
- 安装包（zip 免安装已可用）、代码签名、开机启动开关。
- 交付物：`Eryu-Windows-W1.zip`（exe + dll + 使用说明，8.7MB，sha256 `4e8f1acc28c10e0a91e6b50b9674b9b226729c42c79ae5e76d4c6306249670e3`），2026-11-19 已发到 [redacted-email] 与 [redacted-email]。

## W3 实机验证记录：图形设置窗口（2026-11-19）

`shell-win/src/settings.rs`（约 800 行），左侧 LISTBOX 分页 + 右侧原生控件，全部 Win32 子控件（不引 WinUI/WebView，保持「无 WebView」红线）。

- 五页：**通用**（按住说话的键、最短录音 200–1500ms、单次上限 5–600s、上屏方式 paste/keystroke、隐私模式、开机启动）｜**引擎**（三个模型、✓ 使用中 / 已安装 / 未安装（N MB）、使用选中 / 下载选中）｜**音频**（输入设备下拉、背景降噪开关）｜**热词**（每行 `错误 -> 正确`，保存后下一条录音生效，留空保存=清空）｜**诊断**（`config.toml` 全文只读 + 复制全文 + 打开配置目录）。
- 控件 id 必须放进 `HMENU(id as *mut c_void)` 传进 `CreateWindowExW`（传 `None` 会让所有控件 id 都是 0，`WM_COMMAND` 分不清哪个按钮）。
- 尺寸全部走 `px(k, pt)`（k = dpi/96），字体 `Microsoft YaHei` + `GB2312_CHARSET` + ClearType；窗口 820×600pt。
- 真机验证：`--settings-save-test`（进程内写 `min_record_ms=500` → 真实 `save_general` → 重读配置）通过，`%APPDATA%\Eryu\config.toml` 里落盘 `min_record_ms = 500`。五页截图 `docs/settings-win-engine.png`、`docs/settings-win-hotwords.png` 等。
- 托盘菜单加「设置…」（也能 `--open-settings` / `--open-settings-page <n>` 直接打开某一页）。
- **远程操作教训（I-071）**：`schtasks /IT` 起的是非提权令牌 → 写 `C:\` 根目录静默失败（日志必须写 `C:\eryu-shots\`）；任务结束会杀掉它启动的子进程；PowerShell 5.1 的 here-string 要 CRLF、脚本要 UTF-8 BOM，否则静默不执行；跨进程 `EnumChildWindows` 受 UIPI 影响点不到按钮，改用进程内自测。

**W3 交付物**：`Eryu-Windows-W3.zip`（exe 457KB + dll 22.5MB + 使用说明，8.4MB，sha256 `ce71992aeff061ec18b30117ecbd69034521269ed5982759295209249a3eef7b`），2026-11-19 已发 [redacted-email] 与 [redacted-email]。


## W3.5 设置窗口按设计稿重画（2026-11-20）

用户反馈「字体破碎、分辨率差、界面土、按钮卡死、说好的对齐呢」。逐条查下来是三件事：

1. **没按设计稿做**：W3 的窗口是裸 `STATIC/EDIT/COMBOBOX/CHECKBOX/BUTTON`。W3.5 按 `design/settings.png` 重画（`shell-win/src/settings.rs`）：
   - `WM_PAINT` 自绘：`#1b1c1f` 底、`#24262c` 圆角卡片、左侧标题+副标题、7 项导航（选中态 `#3b82f6` 圆角 9、图标 `⚙◈◎✎≡✦⌘`）、底部配置文件路径。
   - 按钮 `BS_OWNERDRAW`：主操作（保存/使用/下载）蓝底白字，次操作深底+1px 边框。
   - 开关 `BS_OWNERDRAW`：44×24 轨道 + 20pt 白 knob，点击由我们翻转 `BM_SETCHECK`。
   - 列表 `LBS_OWNERDRAWFIXED|LBS_HASSTRINGS`：选中行蓝底白字，其余 `#2e3038`。
   - `EDIT/COMBOBOX` 关掉系统视觉样式（`SetWindowTheme(h, " ", " ")`）+ `WM_CTLCOLOREDIT` 返回深色 brush，否则主题会把浅色底强行画回来。
2. **DPI 只在建窗时算一次**：改为处理 `WM_DPICHANGED`——重建 `font/font_title`、重跑 `build()`，窗口尺寸用系统给的 `RECT`。
3. **「点按钮卡死」真因**：`section()` 在 `build()` 持 `WIN` 锁时调 `with_win()`，`std::sync::Mutex` 不可重入 → 自锁死。改成直接读 `w.font_title`；`NAV` 可点区域挪出 `Window` 用独立 `Mutex`（`WM_PAINT` 里再取 `WIN` 会和「持 `WIN` 时 `SendMessage` 回本窗口」互等）。

验收（真机，2560×1440，DPI 100%）：
- 四页截图：`docs/settings-win-general.png`、`-engine.png`、`-hotwords.png`、`-history.png`。
- 点击链路：`WM_COMMAND` 依次 101(保存)/120(历史刷新)/131(润色保存)/105(诊断复制)，每步 700ms，进程全程 `alive=True`，无卡死、无崩溃。
- `--settings-perf-test`：七页重建 3–6ms、各 FFI 读取 ≤5ms（说明卡死不是慢调用）。
- **缩放验证（代理手段）**：`ERYU_SETTINGS_DPI=150|200` 覆盖窗口 k 值重跑同一套布局，截图 `docs/settings-win-dpi150.png`（1350×840 = 900×560pt）、`docs/settings-win-dpi200.png`（1800×1120）——文字清晰、无裁切、无重叠，说明按 pt 布局 + `px(k,·)` 换算这条链是对的；`WM_DPICHANGED` 的真机跨屏切换仍未验（远程机只有 100% 缩放，且改系统缩放会影响整机）。
- 五个分页统一了 `section()` 标题（语音模型 / 麦克风 / 热词表 / 统计与历史 / 润色服务+润色档 / 配置全文），内容统一从 x=196 起、宽 560。
- 焦点外框：EDIT/COMBO 拿焦点时由父窗口在 `WM_PAINT` 里补一圈 2px 强调色圆角框（`draw_focus_ring()`，靠 `WM_COMMAND` 的 `EN_SETFOCUS/EN_KILLFOCUS/CBN_*` 触发重绘）；截图 `docs/settings-win-focus.png`。系统自带的焦点虚线仍在框内，本轮没消。

### 交付包 W4

- 结构：`eryu-shell-win.exe`(515KB) + `eryu_core.dll`(22.6MB) + `安装.ps1` + `使用说明.txt` → `Eryu-Windows-W4.zip`（8.4MB，sha256 `e47946d9f6a0f546ad95ea789c3131f383ba48fcdc3444354376eb4dd442d04f`），已邮件发 `[redacted-email]` + `[redacted-email]`。
- `安装.ps1`：装到 `%LOCALAPPDATA%\Eryu\bin`、建开始菜单快捷方式、可选写 Run 键开机自启、装完直接启动；不需要管理员（例外：要在管理员窗口里注入必须提权运行）。

### 开机自启（W4）

- `shell-win/src/autostart.rs`：`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 下写 `Eryu` = `"<exe 全路径>"`；启动时 `reconcile(config.launch_at_login)` 对齐，设置页保存后 `sync(launch)` 立即生效（失败弹框，不静默）。
- 真机验证：`--autostart-test` → `AUTOSTARTTEST before=None on=Ok(()) after_on=Some("\"C:\\eryu\\shell-win\\target\\release\\eryu-shell-win.exe\"") off=Ok(()) after_off=None`。
- 不用计划任务/服务：个人工具登录即启动足够，卸载删值即可；macOS 侧对应 `SMAppService`。

### 按应用覆盖：只验到决策层（诚实记录）

- `core/src/config.rs::mode_for()` 新增单测 `per_app_mode_matches_and_falls_back`：大小写不敏感、双向包含（`notepad.exe` ↔ `notepad`）、未命中回落全局、空前台名不误命中。
- 壳侧已把前台进程名传给 core（`raw.log` 里 `hotkey_down app=Notepad.exe`）。
- **这台机器没有可用麦克风**（默认 `Steam Streaming Microphone` 虚拟设备，Realtek 阵列也采不到声音：`meter` 全程 `peak=0.0 silent=true`），所以「换应用后真的改用逐字」的端到端只能到「决策层单测 + 逐字引擎已验证（`WM_CHAR` 中文全对）」两段，缺中间那一段真实录音。记 I-074，等 VM 或用户机器补。

## W5 实机验证记录：用户本机的 VMware Windows 11 ARM 虚拟机（2026-11-20）

之前的验证都在家里那台 x86 机器的远程桌面上做。这台是用户自己的 VMware Fusion 虚拟机，
Windows 11 专业版 ARM64（1024×768、DPI 100%），目的是**不依赖远程机器**也能回归。

跑通的部分（都在 VM 里实测，截图在 `docs/vm-*.png`）：

| 项目 | 结果 |
|---|---|
| core 加载 | `Eryu core 0.1.0 (ABI 1.13)`，x64 二进制在 ARM Windows 上由系统模拟运行 |
| 模型识别 | 把 Mac 上那份 SenseVoice 拷进 `%APPDATA%\Eryu\models`，`--decode-wav` 输出「明天下午三点在会议室评审设计文档。」全对 |
| 统计 | 历史页显示「累计 2 次 · 30 字 · 松手→出字 p50 829ms / p95 829ms（2 个样本）」 |
| 设置窗口 | 七页全部渲染正确：深色卡片、左侧图标导航、蓝色选中条、深色输入框与下拉、pill 开关、蓝色主按钮 |
| 浮层 | `--overlay-demo` 在 1024×768 桌面上正常显示：深灰胶囊 + 5 根橘黄电平条 + 档位徽标 + 正文 + 下方提示「松开上屏 · Esc 取消」 |

没验成的部分（都要真机/真麦克风，别当成"已通过"）：

- **VM 没有声卡**（`Get-CimInstance Win32_SoundDevice` 为空，core 报「输入设备：（无）」）→
  按住说话的录音链路没法在 VM 里跑；要测只能先给虚拟机加一块声卡并让它用 Mac 的麦克风。
- **合成按键没被低级钩子收到**：用 `SendInput` 造右 Alt（`VK_RMENU` + `EXTENDEDKEY`）在 VM 里
  没能触发会话（`raw.log` 里只有鼠标事件，没有 `vk=0xA5`）。推测与「从 PowerShell 注入的输入 +
  LL 钩子在不同完整性/会话边界」有关，远程 x86 机器上用 `keybd_event` 是能触发的。
  → 结论：VM 适合验证**渲染、配置、识别**，不适合验证**热键与注入**；后者仍要真机手按。

远程验证的操作方式（可复用）：VMware 的 guest 操作要账号密码，
`vmrun -gu <user> -gp <pass> runProgramInGuest <vmx> -noWait -interactive powershell.exe -NoProfile -EncodedCommand <base64>`。
PowerShell 里截屏用 `CopyFromScreen`；截屏前要 `keybd_event` 敲一下唤醒屏幕，否则整张图全黑。
复制文件用 `copyFileFromHostToGuest` / `copyFileFromGuestToHost`。

## W5 视觉修正（用户反馈）

用户反馈（原话要点）：「字体丑爆了，用微软系统的默认字体」「icon 做好大小控制」「橙色动态部分明显比 mac 版大很多」「润色 Icon 又没有」「app 本身的 logo 也没有显示」。

逐条对应：

| 反馈 | 根因 | 修法 |
|---|---|---|
| 字体丑 | 硬写 `Microsoft YaHei` + `GB2312_CHARSET`，西文也走雅黑的点阵感 | 统一改成 `Segoe UI` + `DEFAULT_CHARSET`，中文由系统字体链接自动回退 |
| 橘黄条偏大 / 图标压字 | 条改成 5 根后，`icon_x`/`text_x` 仍按 `3.0 * (BAR_W + BAR_GAP)` 算 | 引入 `const BARS: usize = 5`，偏移都按 `BARS` 算；波形公式与 macOS 0.6.1 对齐 |
| 润色图标没有 | 只画了「档名首字」的方块 | `icon_glyph()` 把 SF Symbol 名映射成图标字形（💬 ✉ 📝 ✨ …），用 `Segoe UI Emoji` 画 |
| app logo 没有 | 托盘用 `LoadIconW(None, IDI_APPLICATION)`，窗口类没有 `hIcon` | `shell-win/assets/eryu.ico`（16/24/32/48/64/128/256）`include_bytes!`，启动落到 `%APPDATA%\Eryu\eryu.ico`，托盘与 `WNDCLASSW.hIcon` 都用它 |
| 焦点蓝框圈住整个列表 | `open_at` 把焦点给了第一个控件（列表） | `is_input()` 只给 `Edit`/`ComboBox` 画框，初始焦点也只落在输入框 |
| 浮层偶尔不合成 | 分层窗口在 DWM direct-flip 状态下只 `ShowWindow` 不够 | `UpdateLayeredWindow` 后再 `SetWindowPos(HWND_TOPMOST, SWP_SHOWWINDOW)` + `RedrawWindow` |

交叉编译（本机，不依赖远程 Windows）：`rustup target add x86_64-pc-windows-msvc` + `pip install --break-system-packages cargo-xwin`，
`SHERPA_ONNX_LIB_DIR` 指 Windows 静态库，`cargo xwin build --release --target x86_64-pc-windows-msvc`（壳 5s 级）。
core 的 `cdylib` 还需要 `llvm-lib`（`brew install llvm`），这台机器装完就能连 core 一起在 Mac 上出 dll。

## W6 首次启动引导

用户要求：「对于新用户，要有引导页面，引导用户做最基本的设置，比如输入方式、润色、按键、模型」。

- `shell-win/src/onboarding.rs`：640×500 深色窗口，四步（模型 / 按键 / 上屏方式 / 润色）+ 步骤点 + 卡片。
  说明文字用 GDI 画（`Segoe UI`），交互只有 EDIT / COMBOBOX / 自绘按钮。
- 完成后的标记写 `%APPDATA%\Eryu\onboarding.done`（不依赖 core 版本，老 dll 也能用）；
  同时尽力把 `onboarding_done` 写进 core 配置（新 dll 才认）。
- 命令行：`--onboarding` 强制重看、`--no-onboarding` 跳过、`--onboarding-step 0..3` 直落某步（截图用）。
- 踩坑：EDIT / COMBOBOX 必须 `SetWindowTheme(h, " ", " ")` 关掉视觉主题，并且 `WM_CTLCOLOREDIT` 要返回**画刷**（只设颜色不返回 brush 仍是白底）；
  自绘按钮的圆角四角会露出系统按钮底色，先按所在区域（卡片/窗口底）填一次再 `RoundRect`。
- 真机（VMware Win11 ARM 1024×768）截图核对：模型页、按键页（深色输入框 `ALT_R`）、上屏页（下拉框深色）。

## 模型镜像服务器（2026-11-20 上线）

- 地址：`https://officeace.harmonyos.fun/eryu-models/`（华为云那台 `huawei-vm`，`/var/www/eryu-models`）。
- nginx：`officeace.conf` 里加 `location ^~ /eryu-models/ { alias /var/www/eryu-models/; autoindex on; }`（原文件备份 `officeace.conf.bak-eryu-*`），`nginx -t` + `systemctl reload nginx`。
- 已上传：`sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09.tar.bz2`（158MB）、`sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2`（838MB）、`silero_vad.onnx`（629KB）。
- core 默认 `[models] mirror` 指向它；`models::source_url()` = 镜像 + 文件名。实测本机 `--pull-model sense` 走镜像能下（progress 事件正常）。
- 速率实测：单连接约 270–290 KB/s（华为云出网限速），SenseVoice 233MB 约 10–14 分钟；需要更快再考虑拆分/多线程或换出口。

## W7：功能补齐（2026-11-20 晚）

这一版把 macOS 上除了「待复制卡片」之外的能力都搬了过来，并在用户的 VMware 虚拟机（Windows 11 ARM）里逐项验过。

| 能力 | 落点 | 验证方式 |
|---|---|---|
| 选区改写 Ctrl+Alt+E | `shell-win/src/selection.rs`（哨兵剪贴板抓选区 → 深色指令面板 → `eryu_run_command` → 粘回） | `--selection-demo` 截图 `docs/settings-win-selection.png` |
| 注入后校验 | `inject.rs::focused_text()`（`WM_GETTEXT` → `GetWindowTextW`） | 粘贴后读回焦控件；看不到就报 unknown，不二次注入 |
| 录音时暂停播放 | `media.rs`（GSMTC `TryPauseAsync/TryPlayAsync`）+ `main.rs` 的 `PAUSE_MEDIA` | 编译通过；VM 无声卡，未做真实播放验证 |
| 档位快捷键槽 | `main.rs` 的 `static SLOTS` + `reload_slots()`；设置页「档位快捷键」编辑框（`ID_POLISH_SLOT`） | 只收组合键，按下沿切档并更新浮层图标 |
| 重识别失败的段 | 托盘动态子菜单（`1200+idx`）→ `core.retry_segment` | 结果只进剪贴板，不自动注入（与 macOS 同一策略） |
| 首次引导四步 | `shell-win/src/onboarding.rs` + `--onboarding/--onboarding-step` | 截图核对；完成写 `%APPDATA%\Eryu\onboarding.done` |
| 模型镜像 | core `DEFAULT_MODEL_MIRROR`（`officeace.harmonyos.fun/eryu-models`） | VM `--dump-config` 里能看到 `mirror = ...` |

**本机交叉编译（不再依赖远程 Windows）**

```bash
# 一次准备
rustup target add x86_64-pc-windows-msvc
pip install --break-system-packages cargo-xwin
# 需要 llvm-lib（brew install llvm 后把 bin 加进 PATH）
export WINLIB=/tmp/winlibs/sherpa-onnx-v2.0.1-win-x64-static-MD-Release-lib/lib

cd core     && PATH=/opt/homebrew/opt/llvm/bin:$PATH SHERPA_ONNX_LIB_DIR=$WINLIB cargo xwin build --release --target x86_64-pc-windows-msvc
cd shell-win && SHERPA_ONNX_LIB_DIR=$WINLIB cargo xwin build --release --target x86_64-pc-windows-msvc
# 注意：exe 用 LoadLibraryW("eryu_core.dll") 找的是 **exe 同目录** 的 dll（I-076）
```

**VM 里验证到的（Windows 11 ARM，1024×768）**

- `--diagnose`：`Eryu core 0.1.0 (ABI 1.13)`、SenseVoice 就绪、VAD 有、输入设备（无）
- `--decode-wav`：`明天下午三点在会议室评审设计文档。`
- `--settings-live-test`：11 项配置写读全 OK（I-075 的版本号修完才可靠）
- 设置七页 + 选区面板截图：`docs/settings-win-audio.png`、`docs/settings-win-polish.png`、`docs/settings-win-selection.png`
- 未验：真实录音（VM 没声卡，I-080）、注入后校验的真实粘贴、暂停播放的真实音乐

## W8：绿色单文件 exe（2026-11-20）

目标：**一个 exe，双击就跑，不用装。**

做法：
- core 的 `eryu_core.dll`（22MB）用 `include_bytes!` 编进 `eryu-shell-win.exe`（`shell-win/src/core.rs` 的 `EMBEDDED_CORE` + `ensure_core_dll()`）
- 启动时先看 **exe 同目录** 有没有 `eryu_core.dll`（开发/替换调试用这一份），没有就解到 `%APPDATA%\Eryu\bin\eryu_core.dll`，用 `eryu_core.stamp`（长度+首尾字节）判断要不要重写，避免每次启动都写 22MB
- 产物 22MB 单文件，`LoadLibraryW(全路径)` 加载解出来的 dll；`--diagnose` / `--dump-config` / `--decode-wav` 全部照旧可用

绿色便携（数据跟着文件夹走）：
- exe 旁边放 `portable.txt`（或建一个 `EryuData` 目录）→ `config_dir()` 返回 `<exe>\EryuData`
- 配置、历史、统计、日志、内置 dll 全落在这里，`%APPDATA%` 不碰；拷走整个文件夹就带走全部状态

构建（`scripts/build-win.sh`）：
```bash
# 需要 llvm-lib（brew install llvm）与 sherpa 的 win-x64 静态库
./scripts/build-win.sh          # 产出 dist-win/Eryu.exe（单文件）
```

真机/VM 验证（Windows 11 ARM 虚拟机）：
- 只有 `Eryu.exe` 一个文件时 `--diagnose` → `CORE 内置 dll 已解出：…\Eryu\bin\eryu_core.dll`、`Eryu core 0.1.0 (ABI 1.13)`、SenseVoice 就绪
- 放 `portable.txt` 后 `--dump-config` → 配置落在 `C:\eryu-green\EryuData\config.toml`，dll 解到 `EryuData\bin`
- 交付包：`Eryu-Windows-Green.zip`（`Eryu.exe` + 使用说明，8.5MB，zip 后）

## W9 验证记录（Windows 11 ARM 虚拟机，2026-11-20）

虚拟机这次恢复了软件声卡（`扬声器 (High Definition Audio Device)` + `麦克风 (High Definition Audio Device)`），所以录音链路第一次在 Windows 侧跑通：

| 项 | 命令/操作 | 结果 |
|---|---|---|
| 采音 + 识别 | `Eryu.exe --mic-test 4` | `MICTEST 录音 4s…` → `MICTEST OK 结果=嗯嗯`（宿主扬声器放 TTS，虚拟麦收到部分） |
| 会话链路 | `Eryu.exe --hotkey-sim --hotkey-sim-secs 5` | 日志 `SIM hotkey_down` / `SIM hotkey_up`，进程 responding=True |
| 逐字上屏 | `--inject-type 你好` | `TYPETEST ok`（是否落到目标控件取决于前台焦点，远程脚本里焦点常被控制台抢走） |
| 设置页 | `--open-settings-page 0` | 字体 15px/19px、无重影、键位框显示 `F13`（录制识别） |
| 引导页 | `--onboarding --onboarding-step 1` → 录键 → 下一步 | responding=True，切到「上屏方式」 |

这轮修掉的问题：I-083（重建控件时 paint/颜色回调二次抢锁 → 未响应）、I-084（字号 + 静态文字重影）、I-085（下载闪黑框、无错误原因）、I-086（按键录制控件）。

仍未在 Windows 侧验证：真机麦克风下的完整「按住→说→松手→粘进目标应用」、真实多显示器 DPI 切换、UIPI 降级提示。

## 诊断包：出问题时一次点击把证据拿出来（R-127 / DESIGN §12.8）

用户那边出问题，最常见的情况是「说不清哪一步不对」。所以 Windows 版有一键诊断包：

- **设置 → 诊断**：第一屏是只读状态（引擎/模型/输入设备/历史/润色档），下面一行两个按钮——
  - **导出诊断包**：写到 `%APPDATA%\Eryu\diag\eryu-diag-<时间>.json`，按钮右侧显示完整路径；
  - **上传诊断包**：先写本地，再 `POST` 到 `[diag] upload_url`（默认模型镜像那台：`https://officeace.harmonyos.fun/eryu-diag/upload`）；15 秒超时，结果显示在同一行，失败也会留本地文件。
- **命令行**（自动化/远程支持用）：`Eryu.exe --diag-bundle`、`Eryu.exe --diag-upload`，输出 `DIAG 已保存 <路径>` / `DIAG 已上传 <url>` / `DIAG 上传失败 rc=… <原因>` 与 `DIAGCODES <条数>`。
- **包里有什么**：`app`（版本/平台/架构）、`host`（Windows 版本、DPI、屏幕、exe 路径）、`config`（脱敏：`api_key`/`token`/`password`/`secret` → `***`）、`diagnose`（核心自检原文）、`errors`（最近 20 条 `{code,msg}`，来自 `eryu-errors.log`）、`history`（只统计条数与状态分布）、`log_tail`（`eryu-cli.log` 尾 200 行）。
- **包里没有什么**：转写正文、历史文字、热词内容、API Key。历史只给条数。
- **错误码**：面向用户的失败都带 `code`（见 `docs/ERRORS.md`），且会落盘到 `%APPDATA%\Eryu\eryu-errors.log`——问题发生在上一次运行也能被下次导出的诊断包带上。

### 服务端契约（给上传端点用）

```
POST <[diag] upload_url>
Content-Type: application/json; charset=utf-8
User-Agent: Eryu/<版本>

<body> = 诊断包 JSON（上面七段，格式版本 format=1）
成功：200 + {"ok":true,"id":"<服务端生成的编号>"}
失败：4xx/5xx（客户端只看状态码；404 会提示「服务端还没有开放诊断上传」，413 提示「太大」，其它显示 HTTP 状态）
```

本地自测（宿主起一个 mock，VM 指向它就够）：

```bash
python3 - <<'PY' &
from http.server import BaseHTTPRequestHandler, HTTPServer
class H(BaseHTTPRequestHandler):
    def do_POST(self):
        n=int(self.headers.get('Content-Length') or 0); open('/tmp/body.json','wb').write(self.rfile.read(n))
        self.send_response(200); self.end_headers(); self.wfile.write(b'{"ok":true,"id":"mock"}')
HTTPServer(('0.0.0.0',18082),H).serve_forever()
PY
```

## 发版包里没有测试开关

安装包里的 `Eryu.exe` 只保留用户会用的入口（`--show-settings`、`--show-history`、`--diag-bundle`、`--diag-upload`、`--self-test`）。凡是**会改用户数据或跳过同意**的开关都收在 `--dev` 后面：`--set-config`、`--set-hotkey`、`--history-delete`、`--decode-wav`、`--inject-test`、`--hotkey-sim*`、`--onboarding-step`、`--dump-config`、`--selection-run`、`--selection-demo`。不带 `--dev` 直接给这些参数会退出码 2 并打印「只在开发版可用」。

`--assume-consent` 也只在 `--dev` 下有效，而且它只跳过「开启润色时这段文字会发到服务端」那一个弹窗——不再顺带把进程里所有对话框降级成 stderr（之前会让「已在运行」「组合键看不懂」这类提示凭空消失）。

## 与 mac 的一处差异：引导是 6 步

mac 的引导是 5 步（模型 → 麦克风 → 试说 → 上屏方式 → 润色），Windows 多一步「热键」：Windows 的热键要在这台机器上真的注册一次（mac 有系统级的「按住」语义），单独一页才能当场验证按住/松开都通。顺序：①模型 ②麦克风 ③热键 ④上屏方式 ⑤试说 ⑥润色。早期文档误写成「5 步、第 3 步试说」，以本节为准。

## 真机安装记录（2026-11-21，FLYEASY_AI）

- 机器：`FLYEASY_AI`（Windows 11 专业版 10.0.26300，AMD64，用户 `flyea`），非虚拟机。
- 包：`dist-win/Eryu-0.8.1-win64.zip`（168,944,363B）→ 解压后 `install-win.ps1 -Desktop`，输出 `INSTALL OK`，装到 `%LOCALAPPDATA%\Programs\Eryu`。
- 校验：`--version` = `Eryu 0.8.1`；`--self-test` = `SELFTEST OK`（SenseVoice 加载 878ms，比 VM 里的 1.8s 快一倍）；`--diag-bundle` 正常导出、`DIAGCODES 0`。
- 只有一份：`%LOCALAPPDATA%\Programs\Eryu` 一个 exe（25,009,664B，SHA256 前 16 `553D90E551E5E698`），开始菜单 + 桌面各一个快捷方式都指向它；全盘按 `Eryu.exe` 扫过，除这份外没有第二份（解压用的临时目录装完即删）。
- 单实例：同一个会话里再点一次图标不会再起第二个进程（第二个实例发现 `EryuMessageWindow` 就把已有实例的设置窗口叫出来、给自己一个托盘提示然后 `exit(0)`，**不弹阻塞式对话框**，见 `WM_ERYU_SECOND`）。
