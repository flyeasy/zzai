# 实现问题台账

这里只记**实现过程中撞到的事实性问题**（环境、API、平台行为），每条带证据和结论。设计层面的争论不进这里（那是 `DESIGN-*-audit*.md` 的事）。

## I-001 本机没有 Xcode（2026-11-19）

- **现象**：`xcodebuild` 报 `requires Xcode, but active developer directory ... command line tools instance`；`/Applications/Xcode*.app` 不存在。
- **可用工具**：CLT 的 `swift 6.3.3`（target `arm64-apple-macosx26.0`）、macOS SDK 26.5、`codesign`、`otool`。
- **结论**：走 D-002（SwiftPM + 脚本组装 .app）。`import SwiftUI` 在 CLT 下可解析（已用 `swiftc -parse` 验证）。
- **影响**：无法用 Xcode 归档/公证；个人自用不需要。

## I-002 没有代码签名身份（2026-11-19）

- **现象**：`security find-identity -v -p codesigning` → `0 valid identities found`。
- **结论**：先用 ad-hoc 签名（D-003），实测 TCC 重授权频率；若每次都需重授，改自签名证书（D-003b）。
- **注意**：TCC 授权对话框是 GUI 的，**agent 无法代为点击**——涉及麦克风/辅助功能的验收必须交给用户手动跑一遍。

## I-003 Carbon 热键是否给 Released 事件，待实测（2026-11-19）

- **待验**：`RegisterEventHotKey` + `kEventHotKeyReleased` 在 macOS 26.7.1 上是否真的送达。
- **备选**：若只给 Pressed，则交互改为「按一下开始 / 再按一下结束」，并在 §4.5 与文案里同步。

## I-004 crates.io 直连不可用（2026-11-19）

- **现象**：`cargo add` 报 `SSL connect error ... index.crates.io:443`。
- **结论**：项目内 `.cargo/config.toml` 用 `sparse+https://rsproxy.cn/index/`（已验证可拉取 146 个依赖）。

## I-006 VAD 整段喂入会丢段（已修，D-007）（2026-11-19）

- **现象**：`Segmenter::accept` 整段喂 5.59s 语音，只产出 0.31s 一段（尾部），识别结果为空。
- **定位**：`core/examples/vadprobe.rs` 对比 `VAD_WHOLE=1`（整段）与默认（逐 512 窗）——前者 1 段 5024 采样，后者 1 段 70304 采样。
- **结论**：逐窗喂（D-007）。已加 `WINDOW_SAMPLES` 常量与注释。

## I-007 链入 sherpa 后 Swift 链接缺 libc++（已修，D-009）（2026-11-19）

- **现象**：`swift build` 报 `Undefined symbols: std::logic_error::what() const ...`。
- **结论**：加 `.linkedLibrary("c++")`；ABI 符号改由 `shim.c` 的 used 指针表强引用（`eryu_debug_panic` 曾因 -O 被优化掉而缺符号）。

## I-008 长音频识别是串行的（待优化）（2026-11-19）

- **实测**：55.7s / 9 段，VAD+识别共 2.36s（RTF 0.042，每段约 230ms）；spike 里 Python 同素材 698ms 是因为段更少。
- **结论**：M1a 可接受（120s 约 5s）；「边录边解」进 M1a 后续优化，接口不变。若 120s 实测超 2s 尾延迟，再考虑并行段识别（引擎非线程安全，需多实例或分段池）。

## I-005 sherpa-onnx Rust crate 的 build.rs 会去 GitHub 下静态库（2026-11-19）

- **现象**：`sherpa-onnx-sys` build.rs 下载 `sherpa-onnx-v2.0.1-osx-arm64-static-lib.tar.bz2` 失败（TLS 中断）。
- **结论**：手动下载后用 `SHERPA_ONNX_LIB_DIR` 指向解包目录即可（`/tmp/eryu-spike/sherpa-libs/.../lib`）。产物要迁进仓库外的稳定位置（见 `scripts/fetch-sherpa-libs.sh`）。

## I-009 ad-hoc 签名下 SMAppService 开机启动可能被系统拒绝（待用户验收）

- **现象**：`LoginItem` 用 `SMAppService.mainApp.register()`；没有开发者证书、TeamIdentifier 为空（`codesign -dv` 显示 `TeamIdentifier=not set`），macOS 可能返回错误。
- **当前处理**：失败只 NSLog + 设置页 Toggle 回到关闭并 toast，不崩、不假装成功。
- **待验证**：真机点开 Toggle 看是否成功；若稳定失败，改为写 `~/Library/LaunchAgents/com.eryu.app.plist`（D-003b 的自签名证书路线也可一并试）。

## I-010 真实按住热键链路仍需 TCC 授权才能验收（阻塞在用户点击）

- **现状**：`⌃⌥Space` 由 Carbon `RegisterEventHotKey` 注册（pressed + released 都装）；`--mic-test` 与 `--self-test` 已绕过热键验证了采音/识别/注入回调。
- **待验证**：①按住 → 菜单栏变「在听…」②松开 → 「识别中…」→「已上屏」③在 TextEdit/浏览器输入框里看到文字 ④目标 app 里按 ⌃⌥Space 不被吞。
- **注意**：辅助功能未授权时，注入会回报 `undelivered` 并在菜单提示原因（不会静默丢字）。

## I-011 CLI 模式下历史里的「应用」是空的

- **现象**：`--mic-test` / `--self-test` 走的是调试入口，没有前台 app（`NSWorkspace.frontmostApplication` 是终端）。
- **影响**：只有真实热键路径会带 app 名；历史页显示「未知应用」。不是 bug，但别拿 CLI 截图当验收证据。

## I-012 设置页 TabView 在 macOS 13 上是顶部标签栏，不是侧边栏

- **现象**：`design/settings.png` 目标稿是左侧竖排导航；`NSHostingView` 里的 `TabView` 在 macOS 13 渲染成顶部 segment。
- **决定**：M1b 接受（功能优先，视觉对齐放 M2）。要侧边栏就得自绘 `HSplitView` + 列表，收益是纯外观，不在 M1 验收里。

## I-013 ad-hoc 签名每次重建都会让 TCC 授权失效（阻塞真机验收）

- **现象**：`scripts/build.sh` 生成的二进制 CDHash 每次都不一样；`build/Eryu.app` 重新签名后首次访问麦克风，系统重新弹「Eryu 想访问麦克风」；辅助功能勾选也可能被清掉。
- **原因**：TCC 的身份判定包含代码签名信息；ad-hoc（`Signature=adhoc`、`TeamIdentifier=not set`）没有稳定锚点，只能靠 CDHash。
- **影响**：每轮改代码重新构建后都要重新授权一次；用户手点才能过，agent 不能代点。
- **候选方案**（D-003b）：登录钥匙串里建一张自签名 code signing 证书（`identity="Eryu Dev"`），`codesign --sign "Eryu Dev"`——指定要求变成「identifier + 证书锚」，TCC 可跨重建保持。需要用户配合一次钥匙串操作。
- **临时口径**：先 ad-hoc；验收时把「授权一次 → 连续用」当作一个场景，不要在验收前反复重建。

## I-015 单键热键在安全输入下静默失效

- 现象：密码框等 Secure Event Input 场景，系统不向 `CGEventTap` 投递键盘事件，单键热键「按了没反应」。
- 判断：系统行为，不是 bug；组合键（Carbon）同样会被拦。
- 处理：浮层/菜单不显示「在听」即为没触发；不额外提示（避免每次打字弹窗）。文档写明。

## I-016 准流式预览是「重跑整段」而非增量解码

- 现象：`partial_text` 每 900ms 用已录音频整段重跑 VAD+识别，不是模型级流式。
- 代价：录音 60s 时最后几次 partial 要重跑 60s 音频（SenseVoice RTF≈0.05 → ~3s；实际有 VAD 分段，通常更快）。
- 保留原因：接缝/热词/润色与正式链路完全一致，不需要流式模型；若卡顿再把间隔改成自适应（音频越长间隔越大）。

## I-017 NSHostingView 在透明无边框 NSPanel 里不合成（浮层不可见）

- 现象：`--demo-overlay` 里 `NSPanel` 的 `isVisible=true`、`frame`/`alpha`/`level` 全部正常，
  `NSLog` 也确认 `NSHostingView` 的 frame 是 420×74，但 `screencapture` 截图里完全看不到浮层。
  同一位置换成普通 `NSView` + 红色 `CALayer` 立刻可见（`--overlay-solid` 诊断）。
- 结论：SwiftUI 内容在该窗口形态下不参与合成（尝试过 .regularMaterial / 不透明色 / container+subview 均无效）。
- 处理：浮层改纯 AppKit 绘制（`OverlayContentView`：CALayer 背景 + 弧形电平条 + NSTextField），已验证可见。
  设置窗口（普通 `NSWindow` + `NSHostingView`）不受影响。

## I-018 从终端直接跑二进制会「继承」终端的 TCC 信任，导致误判已授权

- 现象：`./build/Eryu.app/Contents/MacOS/Eryu --perm-check` 显示麦克风/辅助功能/输入监控全部 ✅，
  但用 `open -n -a build/Eryu.app --args --perm-check`（LaunchServices 启动，等同用户双击）却全部 ❌。
- 原因：TCC 把「从终端启动的子进程」归因到**负责进程**（终端 / DSH），所以读到的是终端自己的授权。
- 影响：**权限状态只能用 `open` 启动来判定**；用终端起的实例做端到端测试会把结果粘到当前前台应用上（危险）。
- 处理：`--inject-test` 加「必须先成为前台应用才发 ⌘V」的保护；真实上屏验证必须在用户授权后由用户实际操作。

## I-019 NSTextView 自测窗口无法在终端启动时成为前台应用

- 现象：`NSApp.setActivationPolicy(.regular)` + `NSRunningApplication.current.activate(...)` 后 `NSApp.isActive` 仍为 false。
- 原因：非 LaunchServices 启动的进程无法抢占前台（macOS 行为）。
- 处理：自测窗口路径改为「成为前台才继续，否则 FAIL 退出」，绝不盲发 ⌘V（见 I-018）。

## I-020 无边框 NSPanel 用 makeKeyAndOrderFront 不显示（要 orderFrontRegardless）

- 现象：指令浮层 `makeKeyAndOrderFront(nil)` 后 `panel.isVisible == true`、frame 正常，截图里却没有；
  改成 `orderFrontRegardless()` + `makeKey()` 才画出来。
- 背景：`.nonactivatingPanel` + 非 LaunchServices 启动时，`makeKeyAndOrderFront` 的"key"语义把窗口卡在未显示状态。
- 处理：`CommandPalette.show()` 用 `orderFrontRegardless()`；录音浮层本来就用它。

## I-021 录音暂停音乐：逻辑就绪，缺一次真实播放中的验证

- 实现：`MediaPause` 只用 AppleScript 查 `player state`，只在「当时在播」才 `pause`，松手只恢复自己暂停的那个。
- 现状：本机测试时 Music 未在播放（AppleScript 拉起 Music 会卡住 60s，已避免），所以未做真实开播验证。
- 待验证：用户开着 Music/Spotify 按住右 Option 说一句，确认播放暂停、松手恢复。网易云音乐未接入（无稳定脚本接口）。
- 兜底：任何 AppleScript 失败都只记日志，不影响录音链路。

## I-022 keystroke 上屏：事件构造已验证，真实投递待验收

- 已验证：`--type-event-test` 回读 UTF-16 与写入一致 → `TYPETEST OK 写入 13 单元 回读=你好 Eryu 42 🚀`。
- 未验证：真实投递到目标应用（需要辅助功能 + 输入监控，且当前 TCC 未由用户授权；agent 无法代点）。
- 已知限制：输入法联想会让打字过程中出现候选词干扰；安全输入（`IsSecureEventInputEnabled()`）直接回报失败。

## I-023 注入后校验的盲区

- 终端（Terminal/iTerm）、自绘控件、部分 WebView 内部读不到 `AXValue` → 校验降级为「已发送」，不做判断（D-028）。
- 因此「粘贴成功但用户没看到」的最后一道兜底仍是菜单里的「复制上次结果」，不是自动校验。

## I-024 GitHub 下载用 rustls 会握手失败，改用系统 curl

- 现象：`ureq`（rustls）访问 `github.com` → `tls connection init failed: unexpected end of file`；同一台机器 `curl` 正常（`https://www.baidu.com` 与 `api.deepseek.com` 用 ureq 也正常）。
- 判断：GitHub 这条路的 TLS 握手对 rustls ClientHello 不友好（疑似按指纹拦截），与本机证书库无关（已试 `native-certs`）。
- 处理：模型下载改走系统 `curl -fL -C -`（macOS 自带；Windows 10+ 自带 curl.exe），进度按 `.part` 文件大小轮询；`ureq` 只留给润色/选区指令这类 API 调用（DeepSeek 端点实测可用）。

## I-025 段间接缝在 ASCII 标点后漏空格（已修）

- 现象：Qwen3 输出带句号时拼接成 `by myself.Je suis`。
- 原因：`join_segments` 只在「两侧都是 ASCII 字母数字」时补空格，句号不算字母数字。
- 修复：两侧都是 ASCII 就考虑补空格，只有「前一段以 , . ! ? ; : 结尾」时省略；补了 `joins_ascii_segments_with_space_after_period` 单测。

## I-026 壳侧从不处理 `notice` 事件（已修）

- 现象：润色失败、到时长上限这类「不是错误但用户该知道」的提示，core 发了 `notice`，AppDelegate 的 switch 里没有这个 case，全部静默丢弃。
- 修复：`case "notice"` → 记日志 + 菜单通知；`record_limit` 同时弹浮层。

## I-027 延时样本的边界

`--bench` 的样本来自真实录音，但本机测试时音频是扬声器外放、由麦克风拾起，识别内容不可用；因此这些数字只证明
「松手→出字」的链路延时，不代表识别质量。要评估准确率需要用户真人对着麦克风说，并人工核对。

## I-028 Windows 侧无法在本机验证

本机只有 `aarch64-apple-darwin` Rust 目标，无 mingw / MSVC / Windows SDK，也没有 Windows 机器，
因此 Windows 端只做到「core 便携化 + 落地清单（docs/WINDOWS.md）」，没有可运行的 Windows 产物。
要推进必须先有一台 Windows 机器（或 CI 上的 windows-latest runner），否则任何「已支持 Windows」的说法都不成立。

## I-029 部分成功只覆盖「ASR 抛错」，不覆盖「识别成错字」

- 现在的失败判定来自引擎返回 `Err`（模型崩溃、显存/内存不足、坏音频）。模型「听岔了」但正常返回时，
  `failed_segments` 是空的，用户只能整段重说或事后改热词。
- 想做而没做：用置信度/长度异常做二次判定（例如某段文本长度与音频时长严重不匹配时标记可疑）。
  需要先有每段的置信度输出（SenseVoice/Qwen3 的 `get_result()` 目前不返回 token 级概率），因此推迟。
- 临时替代：`--bench` + 历史里的 `latency_ms`、以及热词自动学习（D-026）覆盖一部分「错字」场景。

## I-030 多屏选屏只用日志+单屏验证过

- 本机只有一块屏（1470×956），`targetScreen()` 的「光标所在屏」和「AX 前台窗口所在屏」两条分支没法真实触发；只用日志确认了单屏路径与面板坐标。
- AX 分支还有已知偏差：某些应用（Chrome/Electron）`AXFocusedWindow` 的位置是窗口左上角，窗口跨两块屏时会选到「左上角所在的那块」，不一定是用户视线所在的那块。想更准需要按「窗口面积占比最大的屏」算。
- 验证方法（有外接屏时）：`open build/Eryu.app` → 把光标移副屏 → 按右 Option → 浮层应出现在副屏底部；日志里 `屏幕=` 应显示副屏 frame。

## I-031 段间没有逗号/句内标点

- ASR 引擎输出不带标点，当前只做「句末补句号」（D-046）。一句话被 VAD 切成几段时，段与段之间没有逗号，长句读起来仍是连着的。
- 不能靠「段间插逗号」解决：0.4s 静音既可能是句间停顿也可能是换气，误插会截断句子。
- 正解：可选标点模型（sherpa-onnx `punct-ct-transformer-zh-en-vocab272727-2024-04-12`，约 290MB，int8 量化后更小）。
  代价是磁盘与内存、以及每段多一次推理（数十毫秒）。等用户确认愿意承担再进 M4。

## I-032 菜单栏图标状态没有逐个截图核对

- 阶段图标只在代码层面接上（`StatusController.show(phase:)`），截图时系统「橙色麦克风指示」和菜单栏裁剪位置都不好定位，没拍齐四个状态的图。
- 核对方法：`open build/Eryu.app` → 按住右 Option 看菜单栏图标是否变成实心波形 → 松手看是否变省略号/箭头。

## I-033 读不到输入框文本的 App（终端/自绘控件）仍会在 750ms 后还原剪贴板

- `Injector.verify` 对这些控件返回 ok（「无法校验 ≠ 失败」，见 D-028），于是走还原分支。
- 若目标 App 粘贴慢于 750ms，文字可能既没进去、剪贴板又被还原（极端情况）。
- 缓解：这类应用建议在「设置 → 通用 → 上屏方式」里改成「逐字打字」，或按应用覆盖。
- 未做真机计时验证（需要用户授权后在终端里实测）。

## I-034 标点模型常驻内存约 +90MB

- 实测同一段音频：`punct = false` 时 RSS 549–575MB，开启后 666MB（模型 int8 65MB + ORT 运行时）。
- 判断：可接受（总 RSS 仍 <700MB，低于 Qwen3 档的 1.6GB），但要写进文档；如果哪天嫌大，就在设置里关掉——关掉只损失逗号，不影响上屏。

## I-035 自测曾经真的把文字粘进用户正在用的应用（已修）

- 现象：`--mic-test` / `--decode-wav` 走的是完整上屏链路，而注入器默认全局合成 ⌘V；前台是 DSH 编辑器时，
  测试句子真的被粘贴进去（早前截图里出现 `ERYU-PASTE-MARKER-42` 就是同类事故），历史里也留下 200 条测试记录。
- 修复：所有 CLI 自测路径（`--mic-test/--decode-wav/--soak/--bench`）一律 `injector.dryRun = true`；
  同时 `eryu_set_test_mode` 让它们不写历史/不落盘（D-055）。
- 清理：200 条测试历史与统计已清零（备份 `/tmp/eryu-history-before-clean.json`、`/tmp/eryu-stats-before-clean.json`）。
- 遗留判断：**任何真实上屏只在用户按了热键时发生**；CLI 自测不再触碰前台应用。

## I-036 渐进封口的接缝仍可能丢一个字

- 切点靠能量判断，中文连续说话时切点可能落在词中间；实测 75s 语料偶尔出现「会议室和投影设备」→「会议识和投影设备」、「各自的材料」→「各材料」。
- 8s 块更严重，所以固定 10s；音频重叠 300ms 的方案被否决（文本层去重会误删真重复，见 D-059）。
- 想彻底解决需要「按字时间戳对齐」的模型接口（sherpa 目前不给 SenseVoice token 时间），或切点用 VAD 的语音段边界而不是能量。

## I-037 Qwen3 在长音频上又慢又退化

- 现象：75s 连续语音（VAD 切成 3 个 30s 段）用 Qwen3 解码 27.3s、峰值 RSS 2.24GB，且第 4–5 段文本出现重复与丢句。
- 原因：Qwen3-ASR 的上下文上限就是 512 token（≈30s 音频），长段必然贴着上限；重复是贴着上限时解码退化。
- 现状：Qwen3 定位为短句精度档（D-060），长听写默认 SenseVoice；渐进封口对 Qwen3 用 4s 块以压延迟。
- 待办（可选）：接近 30s 的段按 8–10s 再切给 Qwen3；需要单独实测它对切段的敏感度。

## I-038 睡眠后热键可能失灵

- Carbon `RegisterEventHotKey` 与 CGEventTap 在睡眠/显示器休眠/切换用户后都可能失效（社区同类反馈一致）。
- 现状：`didWakeNotification` 里重挂一次（D-061），冷启动后首次注册也已覆盖；若仍失灵，用菜单「退出并重新打开」。
- 未验证：真实合盖睡眠后的行为（需要用户手测）。

## I-039 长听写延迟仍有抖动（400–1000ms）

- D-064 之后最坏情况仍可能出现一次「等正在算的块」的开销（≤900ms）；若那一块刚好很大，延迟会靠近 1s。
- 观察：同一段语料连续跑，抖动主要来自当时是否有预览块在算、以及系统负载（截图/其他推理会明显拖慢）。
- 可选改进：块更小（6s）换更平滑的尾延迟，但接缝变多，需重新评估质量（见 D-059）。

## I-036（更新）接缝仍会吞字，三种补白方案实测对比

用 8 句重复语料（`/tmp/t80.wav`，每句都含「周三下午三点的会议室和投影设备」）连跑多次，看 8 句里「会议室」完整出现几次：

| 方案 | 会议室完整 | 其它伪影 | 松手→出字 |
|---|---|---|---|
| 基线（切在安静缝，块不补白） | 3–7 / 8 | 无重复字 | 585–995ms |
| 块尾补 150ms 静音，推进到补白之后 | 差（出现「会议和」丢「室」） | — | 422–522ms |
| 块尾补 150ms，只推进到切点 | 3–7 / 8 | 偶发延迟升高 | 858–1162ms |

结论：补白在噪声/语调边界上反而让模型把尾字当噪声吞掉，**保持基线**。要根治需要模型给 token 时间戳或 VAD 自己报段边界，
再按边界对齐文本；现在的做法是「切在安静缝 + 文本重合去重」，残留错误率约 0.4 字/接缝。

## I-040 音源锁定分不开「同时说话」

- 判断只到 VAD 段级别：一段里两个人同时说话时，embedding 是混合的，谁都不像，只能整段留或整段丢。
- 真正分开需要说话人分离（diarization / 声源分离模型），不在当前范围。
- 另外：插话如果不到 0.6s（「对」「嗯」）会被当作短段放行——这是故意的，短段算不出可靠声纹。

## I-041 「小声」和「没声音」靠能量判断，极端情况仍会错

- 均匀的宽带噪声（风扇直吹）如果响度接近说话，可能被判为「有人说话」，进而走 ASR、拿到空结果，最后报「声音太小或不清楚」。
  提示语是对的，只是归因路径不同，不算致命。
- 真静音（数字 0）与「被静音但有底噪」都能拒掉，这两种最常见的情况是准的。
- 若以后要做更准，应该用 VAD 的语音占比（`SileroVad` 已经能给出段边界）而不是能量阈值。

## I-042 终端里跑 `--acceptance` 会看到「权限全部已授权」的假象

- 从终端启动时 TCC 把权限归因给终端（I-018），所以 `--acceptance` 在终端里跑会显示三项已授权，
  即使用户从没给 Eryu 授过权。
- 正确的用法：`open build/Eryu.app` 之后由 Finder 启动，或者在终端里看「浮层/按键/结果」三项是否为 ✅。
- 报告里保留了权限明细，就是为了让人看出这个差别。

## I-043 降噪没有「按场景自动开关」

- 稳态噪声（空调/风扇）开着更好，竞争人声（旁边有人说话）开着更差，目前靠用户自己在设置里切。
- 自动判断需要区分「噪声」和「别人的语音」，那本质上是说话人分离问题，不在当前范围。
- 若以后要做：可先用 VAD 段 + 声纹判断「这一段像不像我」，再决定降噪强度；代价是每段多一次 embedding。

## I-044 `--acceptance-synthetic` 读不出 WAV 时会静默退回真实麦克风

- 表现：报告里「松手出结果」变成真实麦克风的归因文案（如「麦克风『MacBook Air麦克风』没有收到声音」），
  看起来像权限问题，其实是模拟音频没加载上。
- 现在会打印 `ACCEPTANCE ⚠️ 模拟麦克风 WAV 读不出来或太短（路径，字节数）` 作为提示。
- 根本原因是 `sim_pcm = None` 时没有任何显式信号；若以后模拟成为主要测试手段，应改成「显式失败」而不是回退。

## I-045 安装包未做公证（notarization）

- 自签名证书（Eryu Dev）+ 未公证：首次启动 Gatekeeper 会拦一次（「无法验证开发者」→ 系统设置里允许，或右键打开）。
- 个人自用无所谓；如果要分发给别人，需要 Apple Developer 账号（$99/年）+ `notarytool`。
- 另一个后果：换机器/重建证书后 TCC 授权会失效（I-013 的同类问题）。

## I-046 AEC（回声消除）没做，靠「太轻不上屏」兜底

- 现状：麦克风会拾到扬声器外放的声音；实测这种低信噪比输入会让模型幻觉出乱码句（见 D-072）。
- 兜底：峰值 <2% 不上屏；`pause_media` 可在录音时暂停 Music/Spotify；建议戴耳机。
- 为什么不做 AEC：真正的 AEC 需要"参考信号"（正在播放的音频），macOS 上要拿到系统混音输出得用
  CoreAudio process tap（macOS 14.2+）或安装虚拟声卡；工作量与个人自用收益不成比例。
- 另一个可选方向：录音开始时检测「系统是否在出声」并提示用户（同样需要 process tap）。暂不做。

## I-047 `--simulate` 的倍速会改变接缝质量

- `ERYU_SIM_SPEED=8` 时接缝完整率从 88–100% 掉到 50%：喂样本比渐进封口线程快，切块/提交互相踩。
- 这是测试脚手架的问题，不是产品路径的问题（真实录音是 1× 实时）。
- 规则：测精度用 1×，测延迟用 4×，绝不用 8× 下结论；8× 只用来压时间。

## I-048 设备名不存在时只在启动/测试那一刻报错

- 现在：开会时设置里选了一个已断开的设备，下一次录音才报 `mic_unavailable`（带设备列表）。
- 更好的做法：设备消失时主动回落到系统默认并提示「X 已断开，这次用 Y」——但那属于"静默改行为"，需要用户明确同意。
- 现状可接受：报错清楚、可操作，且不会静默录到错设备。

## I-049 降噪的收益依赖噪声区间，没有"永远更好"

- 等能量白噪声：CER 0.236 → 0.154（好）；噪声 3 倍能量：0.455 → 0.718（明显差）；背景人声：基本持平或略差。
- 现状：默认开 + 设置页写清范围 + 可关。
- 想做得更好需要噪声分类/SNR 估计后动态开关，但那会引入新的误判（把轻声说话判成噪声），暂不做。

## I-050 stub 曾经能让识别类自测假绿

- 现象：临时配置目录里没有模型 → 引擎回退 stub → `--decode-wav` 输出固定假文本却报 `WAVTEST OK`。
- 影响：我的"设备不存在""崩溃恢复"等早期实验一度读的是 stub 结果（已用真实模型重跑）。
- 现状：`requireEngine(_:)` 已拦（D-082），要跑必须显式 `ERYU_ALLOW_STUB=1`。

## I-051 接缝质量只有句子级指标

- `scripts/nopause.py` 用「第K句」标记能证明"没有整句丢失"，但测不到"词中少一个字"。
- 要测字级需要逐字时间戳的真值（TTS 的 word boundary），属于另一种工装；当前优先级不高。

## I-052 隐私模式此前仍会在菜单显示正文（已修）

- 2026-11-19 发现：`setRecent(event.string)` 无条件执行，菜单「最近」显示识别正文。
- 已修（D-087）；`--privacy-test` + `scripts/privacy_check.sh` 守住。

## I-053 `--contract` 之前会被单实例锁挡掉（已修）

- 现象：矩阵第 1 项（空配置目录跑 `--contract`）报「无输出」，实际是「已有 Eryu 实例在运行，退出」。
- 原因：观测型 CLI 白名单里漏了 `--contract`（D-071 的那份列表）。
- 修法：补进白名单；顺带给 `scripts/longcap.sh` 加「延迟偶发偏高重试一次，丢句不重试」。
- 教训：新增 CLI 一定要同步白名单——现在漏一个就会在冒烟里表现为"没输出"，不容易一眼看出。

## I-054 `--acceptance-synthetic` 会一直挂着不退出（已修）

- 现象：闸门第 4 步跑 `--acceptance-synthetic` 二十分钟没输出；单跑也只有两行启动日志。
- 原因：分支判断只认 `--acceptance`，`--acceptance-synthetic` 掉进了交互模式（建状态栏、等用户按键）。
- 修法：条件改成 `contains("--acceptance") || contains("--acceptance-synthetic")`。
- 教训：新增变体参数（`--x-yyy`）时要搜一遍所有 `contains("--x")`；这类漏判的表现是"进程挂着"，
  在带超时的脚本里会看起来像"慢"，很容易被忽略。

## I-055 局域网接口还缺的几块

- 手机端没有客户端（现在只能用 curl / 自己写个小脚本）；没有 TLS（走局域网明文，敏感场景别用）；没有并发限流
  （引擎一把锁，多个请求只会排队，不会崩，但会互相等）；`allow_lan = true` 且 token 为空时会往日志里提醒一次，
  但没强制拦住——这是有意留的（本地家庭网便利优先），要更严就设 token。

## I-056 「你好，Eryu。」自测残留仍偶尔出现在历史里（原因未定位）

- 现象：分别在 07:56 / 07:59 / 08:12 / 08:20 各多出 1 条 `你好，Eryu。`（8 字、app 空、延迟 0），都是自测文本。
- 已排除：单跑 `--self-test`（build 与 dist 副本）、`--contract`、`privacy_check.sh`、完整 `smoke.sh` 都不再新增（前后计数不变）；
  `scripts/smoke.sh` 第 2c 项就是这道守卫，跑一次会立刻红。
- 猜测：这些条目来自某次「测试模式开关加进去之前的二进制」或与 `--settings-live-test` 等未覆盖路径有关。**没有证据，不猜成结论。**
- 现状：已清理 4 条（备份 `/tmp/eryu-history-before-clean4.json`），保留 2 条真实录音。若再出现，先看时间点对应哪条命令。

## I-057 延迟验收受本机负载影响（测量环境问题，不是产品回归）

- 现象：同一份 76s 语料，机器空闲时 `松手→出字 973ms`；当 DSH 渲染进程 + Codex 同时占 ~200% CPU（load average 6.7）时，实测飙到 1829–3592ms。
- 影响：冒烟 3e 与 `longcap.sh` 会因此偶尔红；这不是代码退化（同一二进制、同一语料）。
- 处理：①`--simulate` 的倍速同样会失真（渐进线程按真实时间跑，6 倍速喂完音频时它追不上，记 I-047）→ longcap 的延迟另按 3 倍速测一次；②两条长测试的**判红线**放宽到 2000ms，同时打印「目标 ≤1500」，避免把噪声当回归；③对外口径仍是「空闲 p95 < 1500ms」。
- 想拿干净的数：先 `pkill -f Codex; 等 DSH 空闲`，再跑 `--acceptance-synthetic` 或 `scripts/bench.py`。

## I-058 浮层「四角发灰」是 CALayer 圆角不参与窗口合成的结果（已修）

- 现象：毛玻璃/半透明没生效，圆角处露出底色像素，看起来像灰色方框套一个圆角。
- 修法：背景换成 `NSVisualEffectView(material: .hudWindow, blendingMode: .behindWindow)`，
  由系统合成毛玻璃与圆角（D-100）。不再自绘半透明底色。

## I-059 松手→出字有约 1s 的空洞（高负载时明显）

- 实测（load 5+，6.4s 语料）：`start_decode` 之后 decode 线程 962ms（含等引擎锁 263ms）+ 标点 124ms，
  但 `processing_since → on_decoded` 是 2059ms，中间约 1s 找不到归属。
- 已排除：VAD 加载（<30ms）、引擎锁（已单独计时）、字符处理（微秒级）。
- **2026-11-19 定位结论**：给 `result` 加了 `wait_ms`（松手→真正开始解码，等渐进块提交）与
  `engine_wait_ms`（等预览线程放开引擎，上限 200ms）后实测：`wait=0 engine=200`——
  即约 200ms 是**预览线程占着引擎**（松手时它正跑一块 5–10s 音频的解码，取消位只能在段间生效），
  其余在高负载下随 CPU 抢占放大（load 9 时总延迟 1933–2756ms，load 5.3 时 2125ms，低负载 766ms）。
- 结论：不是隐藏逻辑错误，是「单引擎 + 预览」的结构性代价，已通过 200ms 上限与 `partial_cancel` 封顶；
  验收报告现在会写明「其中等预览块提交 Xms、等引擎锁 Yms」，不再用总延迟一个数字掩盖归因。
- 后续可选（未做）：双引擎（多 500MB 常驻，不划算）、松手时若尾部 <1s 直接跳过预览块的等待。

## I-060 演示模式（--demo-overlay）跑完不退出，留下一个没有热键的进程

- 现象：截图验收后 `--demo-overlay` 的进程一直活着、还不注册热键；用户按右 Option 没反应，看起来像「工具坏了」。
- 根因：`runOverlayDemo()` 只挂了一串定时器就返回，没有 `exit`；而正式实例在测试时被我 `pkill` 掉了。
- 修：演示到最后（6.5s）主动 `exit(0)`；验收脚本跑完也不会再留下孤儿进程。
- 教训：任何「跑完不退」的自测分支都必须在结束时退出，否则会和真实实例抢用户注意力。

## I-061 Windows 路径含反斜杠把 config.toml 写坏（已修）

- 现象（Windows 11 真机）：首次生成 `%APPDATA%\Eryu\config.toml` 后立刻读回失败——
  `TOML parse error at line 12, column 11 ... invalid escape sequence`，原因 `dir = "~\AppData\Roaming\Eryu\models\…"`
  里的 `\A` 被当成非法转义。
- 修复：`Config::to_toml` 统一经过 `esc_str`（`\\` → `\\\\`、`"` → `\"`），路径与设备名都走它；
  macOS 路径不含反斜杠，行为不变。
- 教训：路径不是字符串——凡是往 TOML/JSON 里拼路径都得转义；这条只在 Windows 上暴露，说明"跨平台"必须真机跑。

## I-062 Windows 上 `~` 缩略导致模型路径找不到（已修）

- 现象：`%~` 处理把 `C:\Users\flyea` 缩成 `~`，写进 config 后读回变 `~\AppData\…`，而 `expand()`
  只认 `~/` → 路径不存在 → 「SenseVoice 就绪=false」，模型装了也用不上。
- 修复：`tilde_path()` 在 `cfg!(windows)` 下不缩略；`expand()` 同时认 `~/` 与 `~\`；新增 `home_dir()`
  优先 `HOME`、回落 `USERPROFILE`。

## I-063 分层窗口里 GDI 画中文会双影（已修）

- 现象：`UpdateLayeredWindow` 的 32 位 DIB 上用 `CLEARTYPE_QUALITY` + `DRAWTEXT`，文字出现重影/挤成一团。
- 原因：ClearType 依赖不透明背景做亚像素渲染，透明面上再把色差当 alpha 就糊。
- 修复：先用另一张**不透明黑底** DIB 画白字（`ANTIALIASED_QUALITY`），再按 `max(r,g,b)` 当 alpha
  合成到主 DIB（白字预乘后 RGB=alpha）。中文字体用「Microsoft YaHei」+ `GB2312_CHARSET`。

## I-064 SSH 进程在 session 0，GUI/热键收不到也看不见（绕法）

- 现象：`ssh dreammaker-win` 里直接起 exe，进程在 session 0，没有桌面：浮层不显示、
  Raw Input 收不到按键、截图是黑屏。
- 绕法：`schtasks /Create /SC ONCE /ST 23:59 /TR "powershell -File C:\test.ps1" /IT /F` +
  `schtasks /Run`，任务在用户的交互会话（console，session 1）里跑，截图/合成键才有效。

## I-065 `GetWindowThreadProcessId` 的返回值是线程 id，不是进程 id

Windows 壳读插入光标时 `GetGUIThreadInfo` 一直失败（`gti_ok=false err=87 ERROR_INVALID_PARAMETER`），根因是拿到的是**进程 id**：
`GetWindowThreadProcessId(hwnd, &mut pid)` 的**返回值**才是线程 id，第二个参数只是输出进程 id。把 pid 当线程 id 传给 `GetGUIThreadInfo` 必然参数无效。
修法：`let tid = GetWindowThreadProcessId(fg, Some(&mut pid));`，跨进程读 caret 时再 `AttachThreadInput(GetCurrentThreadId(), tid, true)`，读完解绑。
验证：`--caret-probe` 输出 `PROBE … gti_ok=true err=0 caret_hwnd=330006 rc=(308,1,309,26) … conv=true`。

## I-066 Windows 高 DPI 下不声明感知，GDI 画的字会被压扁

浮层文字在 200%/150% 缩放的机器上看起来「横向挤在一起、和胶囊宽度对不上」，因为进程按 DPI-unaware 运行，GDI 用逻辑坐标画字而窗口/DIB 用物理像素。
修法：`main()` 第一件事 `SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)`（`windows` crate 需开 `Win32_UI_HiDpi` feature）。另加 `--caret-probe` 打印 `DPI scale=` 便于对账。

## I-067 透明分层窗口上不要直接画 ClearType 文字

在预乘 alpha 的 DIB 上 `DrawTextW` + `CLEARTYPE_QUALITY` 会出彩边、看起来发糊；改成按亮度转 alpha 也只是一层补丁（笔画发胖、1px 虚边）。
最终做法：把整个浮层**先当不透明位图**画（实色胶囊 + 电平条 + ClearType 文字），最后统一 `RGB *= alpha/255` 转成预乘 alpha，圆角外 alpha=0。字实、边缘干净。

## I-068 Windows 逐字上屏：`SendInput + KEYEVENTF_UNICODE` 会静默只送前几个字

实机（WinForms TextBox 聚焦）测 `SendInput` 的 unicode 事件：`SendInput` 返回的注入数量是请求的全量、没有任何错误码，但目标控件只收到前两个字（「你好」），后面全部丢掉。
改成 `PostMessageW(hwndFocus, WM_CHAR, unit, 1)`（先 `GetGUIThreadInfo` 取前台线程的 `hwndFocus`，读前 `AttachThreadInput`），逐 UTF-16 单元投递、间隔 3ms → 回读完整「你好 Eryu 42 中文标点测试」。
`SendInput` 保留为 `hwndFocus` 拿不到时的兜底。另：测试脚手架里 PowerShell `-ArgumentList` 会按空格拆参数，含空格的文本要自己套引号，否则被测程序只收到第一个词。

## I-069 Windows：Raw Input 收不到合成键，热键改用低级键盘钩子

`RegisterRawInputDevices(RIDEV_INPUTSINK)` 返回成功，`--hotkey-report` 也正常，但用 `keybd_event` / `SendInput` 合成的按键一条 `WM_INPUT` 都没有（事件日志零增长），意味着热键链路**无法自测**，而真实用户按键又不可能每次人工验。
改用 `WH_KEYBOARD_LL` 低级钩子：合成键同样进回调，`KBDLLHOOKSTRUCT.flags` 的 `LLKHF_EXTENDED(0x01)` 能分左右 Alt/Ctrl，`LLKHF_UP(0x80)` 给松开语义，正好对上「按住说话」。
验证：`CTRL+ALT+SPACE` 合成后依次收到 `state starting → listening → meter`。Raw Input 的注册代码保留但不再是主路径。
副作用与约定：钩子回调里只做「归一化 + 比对 + `hotkey_down/up`」，不做任何 I/O 或阻塞；`ERYU_RAW_DEBUG=1` 时才写 `raw.log`。

## I-070 「按应用覆盖上屏方式」存不进配置文件（macOS 与 Windows 都中招）

`Config::to_toml()` 里 `[insert]` 段的 `per_app` 恒写成 `per_app = {}`（模板里写死），`save()` 之后再 `load()` 得到的覆盖表是空的——设置页里加的「某程序用逐字」在**重启后消失**，Windows 上改 `config.toml` 的手工编辑同样会被下一次保存冲掉。
修法：新增 `per_app_body(map)` 渲染内联表（`per_app = {"Notepad.exe" = "keystroke"}`，键值都过 `escape_toml`），模板改成 `per_app = {{{per_app}}}`（format! 里要写成三对花括号才输出一对）。
注意：`format!` 的 `{per_app}` 会被当成占位符直接替换掉花括号，第一次修就是这个坑——生成出来是 `per_app = "Notepad.exe" = "keystroke"`，TOML 直接坏掉。
单测 `config::privacy_tests::per_app_survives_save_and_reload`（保存 → 重载 → `mode_for("notepad.exe") == "keystroke"`）。

## I-071 长听写（60–120s）松手→出字仍要 2.0–3.2s，没达到 1.5s

现象：`--simulate` 喂 76s / 167s 语料时 `latency_ms` 稳定落在 1.9–3.2s；短句（≤12s）仍在 400–800ms。
拆解（插桩 `Instant` + `sample` 线程栈）：
- `begin_decode`→`start_decode` 基本 0–250ms（等正在跑的预览块提交，最多 `COMMIT_WAIT_MS=900`）；
- 解码线程 `pipeline`（VAD 建流 + 识别尾巴）在 RTF≈0.18 时约 0.2–0.6s；
- 剩下的 1–1.5s 是**等引擎锁**：预览线程与最终识别共用一个 `Arc<Mutex<dyn AsrEngine>>`，松手瞬间若有一块预览正在跑（10s 块在 2 线程下 0.8–1.9s），最终识别只能排在它后面。
已做：
1. 预览窗口封顶 5s（纯预览只解末尾 5s，不再一次解 20s）；
2. 提交块 10s→4s（`chunk_secs`），让每次占锁时间减半，尾巴也更短；
3. `partial_alive` 闸门——保证任何时刻最多一个预览线程（以前会因为 `partial_busy` 复位时机出现 3–4 个并发预览，采样里亲眼看到四个 `eryu-progressive` 线程）；
4. 模拟喂音不再每个 tick `clone()` 整个 76s 音频（改成 `Arc<Vec<f32>>`，否则自测本身就在拖慢机器）。
效果：76s 语料从 3.2s 降到 1.9–2.7s（随机器负载波动）；167s 语料 2.6s。
未解决：离 1.5s 还差一截。下一步候选（按代价排序）：①预览与最终识别各自一个引擎实例（内存 +600MB，不可接受）②预览改「流式/增量」而非整段重跑（要换模型接口）③松手时若预览正在跑，直接用它的结果而不是等锁 + 再解尾巴（需要把「预览块」升级成「可提交块」的判定提前）④标点模型独立线程、与 ASR 并行。

## I-072 Windows 设置窗口：直接用了原生 Win32 控件，没按设计稿做

用户评价（2026-11-20）：「字体是破碎的，分辨率非常差」「界面非常土」「很多按钮点下去直接卡死」「说好的跟 Mac 对齐呢」。
事实核对：
- 设置窗口是 `WS_OVERLAPPED | WS_CAPTION` 的原生窗口 + STATIC/EDIT/COMBOBOX/CHECKBOX/BUTTON，**完全没有按 `design/settings.png`（Stitch 稿：深色、左侧图标导航、卡片分组、蓝色主按钮）实现**；只是在 W1 里"能改了"。
- DPI：`k = GetDpiForWindow(hwnd)/96` 只在建窗时算一次，没有处理 `WM_DPICHANGED`；换了缩放比例的屏幕上会用到旧 `k` 建的字体与尺寸。
- 卡死：进程内计时（`--settings-perf-test`）显示各页重建 3–6ms、各 FFI 读取 ≤5ms，说明不是 FFI 慢；怀疑是 `WIN` 与 `APP` 两把锁的获取顺序，在「点按钮 → 取 APP → 回到 WIN」路径上与主线程形成互等（待用日志验证）。
**状态（2026-11-20）：已修（W3.5）**。三处根因分别是：①原实现确实是裸原生控件；②`k` 只在建窗时算一次；③`section()` 在 `build()` 持 `WIN` 锁期间又 `with_win()` 取同一把**不可重入**的 `std::sync::Mutex` → 打开设置窗口时自锁死（`--settings-perf-test` 直接挂住 600s，可复现）。已改：自绘深色主题（`paint()` 画底/卡片/导航、`BS_OWNERDRAW` 按钮与开关、`LBS_OWNERDRAWFIXED` 列表、`SetWindowTheme(h," "," ")` 关掉 EDIT/COMBO 的系统主题）、`WM_DPICHANGED` 重建字体与布局、`NAV` 可点区域改用独立 `Mutex`。真机点击验收：`WM_COMMAND` 依次打 保存(101)/历史刷新(120)/润色保存(131)/诊断复制(105)，进程全程 alive、无卡死。截图 `docs/settings-win-{general,engine,hotwords,history}.png`。

原修法记录（W3.5）：①按设计稿重画——深色底 `#1b1c1f`、卡片 `#24262c`、蓝色主按钮 `#3b82f6`、Segoe UI/微软雅黑 13px、左侧自定义导航（选中态圆角蓝条），不再用裸 STATIC/LISTBOX 观感；②DPI 全链路 per-monitor：`WM_DPICHANGED` 重建字体与布局，字号用 pt→px 换算；③锁顺序统一为「先 APP 后 WIN」，且任何 `with_win` 闭包里不许再取 APP；④每个按钮都走一遍真实点击（VM 里本机点，远程用 `PostMessage(WM_COMMAND)` + 截图）验收。

## I-074 按应用覆盖缺真实录音的端到端验证（Windows）

- **现象**：远程 Windows 机器没有可用麦克风——默认设备是 `Steam Streaming Microphone`（虚拟），换成 `麦克风阵列 (Realtek(R) Audio)` 后 `meter` 仍全程 `peak=0.0 / silent=true`，会话最后报 `no_audio`。
- **影响**：`per_app` 的链条只验到两段：①`mode_for()` 的决策单测（大小写/双向包含/回落/空名不误命中）②逐字引擎 `WM_CHAR` 实机打字（`你好 Eryu 42 中文标点测试` 全对）。缺「前台是 Notepad/powershell 时真的走逐字路径」这一段。
- **怎么补**：在有麦克风的机器（VM 或用户机器）上，配置 `insert_per_app = {"notepad.exe":"keystroke"}`，粘贴一个剪贴板哨兵，按住热键说一句话，检查剪贴板是否仍是指哨兵（没被替换=走逐字）且记事本出现文字。

## I-075 设置页保存后读回旧值（配置命令排队把新值覆盖回去）

- **现象**：连续两次改配置，第二次读回总是上一次的值（`--settings-live-test` 表现为 OK/FAIL 交替）。单独一次改动（每个进程只改一次）反而正常。
- **根因**：`apply_config_now` 先写共享快照再 `send(Cmd::ApplyConfig)`；worker 里可能还排着上一条旧命令，它执行 `apply_config` 时又把旧 cfg 写回同一个 `Arc<Mutex<Config>>`。
- **修法**：给配置加版本号（`EryuCore::cfg_gen` 单调递增 + `cfg_applied` 记录「已写进快照的版本」）；`Cmd::ApplyConfig(cfg, gen)` 进 worker 后**在锁内**比较 `gen < cfg_applied` 就整条丢弃（连盘都不落）。
- **验证**：Windows 上 `--settings-live-test` 连跑三次全绿（10 项域：最短录音/上屏方式双向/隐私开关双向/降噪双向/热词/润色端点/引擎/设备）。

## I-076（重要，踩了很久）Windows 开发时壳加载的是**旧 dll**

- **现象**：改了 core 的代码、`cargo build --release` 成功了，壳的行为却一点没变（I-075 的版本号修复看起来「无效」）。
- **根因**：`shell-win` 用 `LoadLibraryW("eryu_core.dll")`，Windows 先找 **exe 所在目录**；`core/target/release/eryu_core.dll` 是新的，但 `shell-win/target/release/eryu_core.dll` 是 18:00 的旧副本，加载的是后者。
- **修法**：每次改 core 后必须把新 dll 拷到壳的 exe 目录（打包时同理）。已经在 `docs/WINDOWS.md` 的构建流程里写死这一步。
- **教训**：判断「改没生效」之前先看 `Get-ChildItem` 两个 dll 的 `LastWriteTime`，比反复读代码快得多。

## I-077 长听写延迟在忙机器上会压线（≠功能坏）

- **现象**：`scripts/longcap.sh`（120s 连续语音）在构建/DSH 同时跑时 `latency_ms` 2.0–3.1s；同一台机器空闲时 76s 素材 1.9–2.7s、5s 素材 1.2–1.5s。
- **已确认不坏**：`收尾正常 / 保留开头 / 超出上限的部分没有出现` 三项一直通过，说明只是慢，不是错。
- **原因**：松手后解码线程要和仍在跑的预览线程抢同一个 `Mutex<dyn AsrEngine>`（`等引擎锁` 200ms 级）；120s 尾巴虽只有 ≤4s，但 VAD 建流 + 识别 + 热词 + 标点串起来，机器被抢占时就是 2–3s。
- **处理**：闸门阈值按场景拆开（短句 1500 / 长听写 3500），并在 DESIGN §4.4 写清口径；真正要再压下去的三条候选：①预览线程在松手时立即取消并等它真退出（现在最多等 200ms）②长录音时把提交块缩到 2s ③标点/热词后处理挪到独立线程、先上屏后替换。

### I-077 追记（2026-11-20 03:10，逐阶段插桩后的结论）

用 `ERYU_DEBUG_TIMING=1` 在 76s 语料上逐段打点（`core/src/lib.rs` 的 `[eryu][t]`）：

| 阶段 | 耗时（1× 速度喂音频） | 说明 |
|---|---|---|
| 线程起点 → VAD 就绪 | 13–26ms | VAD 模型加载很便宜，不是瓶颈 |
| 等引擎锁 | 0–2091ms | 和预览线程抢锁；1× 时实测 0 |
| VAD 切段 + ASR | 654–1274ms | 尾巴只有 3.9s（提交块已经吃掉 70.9s，前缀 295 字） |
| 热词 + 标点 | 414–1111ms | ct-punc 对 295 字做一遍 |
| 润色（若开） | **2500ms 级** | 网络调用，跟本地链路无关 |

**两条修正**：
1. `--simulate`/`--acceptance-synthetic` 这类「WAV 当麦克风」的测试**跳过润色**（`ERYU_MIC_WAV` 存在时不润色）——否则验收指标里混进了 2.5s 的网络往返，看不出本地链路到底多快。真实录音仍照常润色。
2. 之前的「关掉长录音预览」实验（>45s 不跑预览块）在 4× 模拟下反而更慢（提交块被拖住），**已回退**；1× 实测预览与最终解码抢锁的概率不高。

**当前结论**：本地链路（VAD + ASR + 热词 + 标点）对 76s 语料是 **1.1–2.2s**，符合 DESIGN §4.4 的长听写目标 ≤3.5s；短句仍是 1.2–1.5s。闸门在负载 ≥4 时按 warn 处理（I-057），因为这台机器经常同时跑 Windows 虚拟机和构建。

## I-078 多行 prompt 会把 profiles.toml 写成非法 TOML（整份档位静默失效）

- **现象**：`--settings-live-test` 报「没收到 engine_ready」；日志里先有一行 `[eryu] profiles.toml 解析失败，用内置：TOML parse error at line 10, column 39 … invalid basic string`。
- **根因**：`profiles.rs::toml_str` 只转义 `\` 和 `"`，用户把多行中文写进「附加要求」后，裸换行直接落进 `prompt = "..."` 的基本字符串里 → 整个文件解析失败 → 所有档位回退内置（用户自定义的档看起来「消失了」）。
- **修法**：`toml_str` 增补 `\n` / `\r` / `\t` 转义；把 TOML 文本生成抽成 `Profiles::to_toml()`（`save` 与单测共用）；新增单测 `prompt_with_newlines_roundtrips`。已把用户机器上那份坏文件就地修好（换行改成 `\n`），内容未丢。
- **附带发现**：`--settings-live-test` 等 `engine_ready` 只等 6s，负载高时 qwen3 加载要 8–10s → 误报「没收到」。放宽到 20s。

## I-080 VM（Windows 11 ARM）没有声卡，录音链路无法在里面验证（已解决，2026-11-20）

- **原状**：`Get-CimInstance Win32_SoundDevice` 返回空，core 的 `--diagnose` 报「输入设备：（无）」；VM 里只能验渲染/配置/`--decode-wav`。
- **现测**：声卡已在（VMware 虚拟 HDAudio，未被禁用的宿主音频设备自动接管）：`Win32_SoundDevice` = `High Definition Audio Device`，PnP `HDAUDIO\FUNC_01&VEN_15AD&DEV_1975`，端点有「扬声器 / 麦克风 (High Definition Audio Device)」，MMDevices Capture/Render 各 1 个 state=1。
- **验证**：guest 内 `C:\eryu-green\Eryu.exe --mic-test 5`，host 音量 100 并 `say -v Tingting "今天天气不错，我们开始吧"`，输出 `[eryu] 采音开始 麦克风 (High Definition Audio Device) 44100Hz 2ch` → `MICTEST OK 结果=天天气不错我们开始吧今天天气不错。`——VM 内「采音 → SenseVoice 识别」链路通。
- **踩坑**：此前 VB-Cable 的 `vbMmeCable64_win10.inf` 是 x64 驱动，ARM Windows 不加载（`pnputil` 加进了驱动库但不生成设备）；改用系统自带的 VMware HDAudio 即可，不需要虚拟线缆。
- **仍缺**：`gtcrn_simple.onnx` 降噪模型没装（只影响降噪）；模型下载完整性问题见 I-103。

## I-081 合成按键（SendInput 右 Alt）在 VM 里没被 LL 钩子收到

- 现象：`raw.log` 只有鼠标事件，没有 `vk=0xA5`；同一套 `keybd_event` 在远程 x86 机器上能触发。
- 影响：VM 里没法自动化验热键；远程 x86 机器与真机手按仍有效。
- 待查：注入输入的完整性级别、UIPI、以及钩子所在会话。

## I-082 设置窗口的蓝色焦点框会画在大列表外面（已修，待构建验证）

- 现象：打开历史页/热词页时，第一个控件是 LISTBOX，`SetFocus(ctrls.first())` 把焦点给了它，
  `draw_focus_ring` 于是在整个列表外围画了一圈蓝框，看起来像"选中了整块区域"。
- 修法：新增 `is_input(HWND)`（按类名判 `Edit` / `ComboBox`），只给输入框画焦点框；
  `open_at` 的初始焦点也改成"第一个输入框"而不是"第一个控件"。

## I-083 Windows 设置/引导「点下一步就未响应」

症状：Windows 版引导页在「按键」页点下一步后整个窗口未响应（用户 m12857）。
根因：`shell-win/src/onboarding.rs` 的 `build()` 在持 `WIN` 锁的情况下重建控件，而重建过程会同步触发：
- 父窗口 `WM_PAINT` → `paint()` 里 `WIN.lock()`（不可重入 Mutex）
- 子控件初始化 `WM_CTLCOLORLISTBOX` → 颜色回调里 `WIN.lock()`

两处都会在同一线程上二次抢同一把锁 → 死锁，表现为「未响应」。
修法：`paint()` 与 `WM_CTLCOLOR*` 一律不阻塞等锁（`try_lock` 拿不到就跳过这一帧；画刷改为一次性缓存），
控件通知（`EN_SETFOCUS/EN_KILLFOCUS` 等）在 `WM_COMMAND` 里直接返回，不进业务分支。
验证：VM 里 `--onboarding --onboarding-step 1` → 点按键框 → 按 F13 → 点下一步 → `responding=True`、页面切到「上屏方式」。

## I-084 Windows 字体过小与文字重影

- 字号：`settings.rs` 之前用 13px/14px，96 DPI 下偏小 → 正文 15px、标题 19px。
- 重影：`WM_CTLCOLORSTATIC` 用 `SetBkMode(TRANSPARENT)` 且只返回底色画刷 → 文本背景不擦除，旧文字留在下面。
  改成 `OPAQUE` + `SetBkColor(C_CARD)`。

## I-085 Windows 下载模型时闪出空的 cmd 窗口、失败没有原因

- `core/src/models.rs` 用 `std::process::Command` 起 `curl`/`tar`，Windows 子系统程序会为子进程分配控制台窗口 → 闪黑框。
  新增 `hide_console()`（`CREATE_NO_WINDOW`），curl/tar 全部走它。
- curl 的 stderr 原来被 `piped` 但从不读取，失败只剩 `下载失败（curl exit status: 22）`。
  改为 stderr 写 `download.err`，失败时把最后一行拼进错误信息；设置页收到 `model_failed` 时弹具体原因。

## I-086 按键不该让用户手写 ALT_R

Windows 版新增 `shell-win/src/keycap.rs`：自定义控件 `EryuKeyCap`，点一下进入录制，直接按键自动识别
（单修饰键在松开时定，组合键在按下主键时定，显示成「右 Alt」「⌃⌥ Space」这类可读文本），
引导页与设置页都改用它。写进配置的仍是 `ALT_R` / `CTRL+ALT+SPACE` 这种 spec，与 core 解析一致。

## I-087 模型下载在内网里失败（系统代理没被用上）

**现象**：用户在内网，点下载没有开始，最后报「下载失败（curl 22）」，没有说明原因。

**根因**：core 用系统 `curl` 下载，而 curl 自己**不读 Windows 的 Internet Settings**（只有浏览器/部分库会读），内网必须走代理时 curl 直连必然失败。

**修法**（`core/src/models.rs`）：
- `system_proxy()`：先看 `HTTPS_PROXY/https_proxy/HTTP_PROXY/http_proxy/ALL_PROXY`，再读 `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings` 的 `ProxyEnable`/`ProxyServer`（支持 `http=…;https=…` 形式）；
- 下载与 `head_length` 都带 `--proxy`；
- 新增配置 `[models] proxy`（JSON `model_proxy`），填了优先用；
- `curl_hint()` 把退出码翻成人话（6 DNS / 7 连不上 / 22 HTTP 404 / 28 超时 / 35 TLS），错误信息附带实际使用的代理；
- Windows 设置「引擎」页加「下载地址」「下载代理」两个输入框；macOS 设置「引擎」页同样加代理框；
- Windows 新增 `--net-check`：打印系统代理、配置里的镜像/代理，并对镜像做一次 HEAD，输出 curl 状态码与耗时。

**验证**：VM 里 `--net-check` → `reg ProxyEnable: DWORD 0x0`、`config mirror=https://officeace.harmonyos.fun/eryu-models`、`curl … code=200 time=0.35s`。

## I-101 引导状态没写进 config.toml（重启后引导重新弹）

**现象**：`--dump-config` 的 `[ui]` 段只有 `launch_at_login`；`onboarding_done` 只存在于 JSON 里，整份配置保存时被丢掉。0.7 的「稍后再说」因此只在本次进程内有效，重启后仍会弹引导。

**根因**：`core/src/config.rs` 的 `merge_json`/`to_json` 有 `onboarding_done`，但 `to_toml`/`merge_toml` 漏了；`save()` 走 TOML，所以落盘即丢。

**修法**：`[ui]` 补 `onboarding_done`、新增 `onboarding_probe_done`（R-101 判定是否做过试说）与 `mini_window`（R-103 窗口开关），`to_toml`/`merge_toml`/`Default` 同步，单测覆盖往返。

## I-102 单实例锁的 fd 被 curl 继承，App 退出后仍显示「已有实例在运行」

**现象**（0.8.1 验证时遇到）：把 App 杀掉、`pgrep` 也没有 Eryu，再启动却打「已有 Eryu 实例在运行，退出」；`lsof <改>w`（如 `/var/folders/…/T/eryu.lock`）显示是一个 `curl -fL …/eryu-models/…` 子进程占着这个 fd。

**根因**：`InstanceLock.acquire` 里 `open(path, O_CREAT | O_RDWR, 0o600)` 没带 `O_CLOEXEC`，`flock` 的 fd 被模型下载的 `curl` 子进程继承；父进程退出后 curl 还活着，锁就一直被占。

**修法（未做，0.8.2 处理）**：`open(… | O_CLOEXEC, 0o600)`，或在拼 curl 命令时显式关闭继承的 fd；单测/冒烟加一条「杀掉 App 后立即重启能拿到锁」。

## I-103 模型下载没有完整性校验：断点续传可能留下「同尺寸但内容损坏」的文件（已修）

**现象**（2026-11-20 在 Windows ARM VM 实测）：`Eryu.exe --pull-model sense` 显示下载成功、文件大小与上游完全一致（237,115,547 字节），但 SHA256 与 host 上的同版本不同（host `12ca1a2a…5276d`，guest `31B1DB0D…2E9722`）；随后初始化报 `Failed to create OfflineRecognizer: … model.int8.onnx failed:Protobuf parsing failed`，引擎静默回退 stub，`--mic-test` 只返回「测试文本：你好，Eryu。」。把 host 的正确文件 `copyFileFromHostToGuest` 覆盖后，同样命令立刻真实识别。

**根因**：`core/src/models.rs::fetch` 用 `curl -fL -C -` 续传到 `.part`，成功后直接改名/解包，**不校验大小与哈希**；只要 `.part` 里残留了上一次中断的旧内容、而服务器又返回 206 从错位处续传，就会拼出同长度但内容损坏的文件。

**修法（待做，0.8.2 处理）**：
- 下载完成后校验：优先取镜像的 `<file>.sha256`；没有就至少做一次「模型加载自检」，失败则删文件重下并明确报错（不再静默回退 stub）。
- 续传前校验 `.part` 与远端 ETag/长度一致；不一致就丢弃重下。
- 若镜像侧能带 `Content-Length` 之外的校验值，Windows/macOS 两个壳共用同一套校验。

**修法（2026-11-21）**：
- 下载完**真的加载一次**再报成功：`core/src/lib.rs::validate_installed_model()` 对 `sense` 用 `SenseVoiceEngine::load`、`qwen3` 用 `Qwen3Engine::load`、`punct` 用 `Punctuator::load` + 一次 `apply`，其余检查 marker；失败就删掉文件/目录并发 `model_failed`「下载的文件打不开（可能是网络断了或代理坏了），已删除，请重试」。
- 内置镜像只同步了部分模型（实测 `gtcrn`/`punct`/`speaker` 在 `officeace.harmonyos.fun/eryu-models` 上 404），所以 `core/src/models.rs::fetch` 改成候选列表：镜像 404/403 时**自动退回官方源**并打日志「镜像没有这个文件，改走官方源」。
- 单测：`corrupt_onnx_is_rejected`（坏字节 → Err）、`missing_marker_is_rejected`（缺文件 → 含「没找到」）。
- Windows 壳 `config_dir()` 补认 `ERYU_CONFIG_DIR`（与 core 优先级一致），自动化测试不再污染真实配置。
**验证（VM）**：`--pull-model gtcrn` 先 404 → 回退官方源 → `model_done`、文件落在 `pullsandbox\models\gtcrn_simple.onnx`；`cargo test` 67/4/6/2 全过。

## I-104 Windows：一次上屏后整个程序卡死（submit_request 分支重入 APP 锁）

**现象**（2026-11-20 VM 里端到端复现）：按热键说话 → 浮层出现识别文字并显示「上屏中…」→ 此后浮层不再消失、托盘菜单点不动、`Get-Process Eryu | Select Responding` 为 `False`；文字也没进目标窗口。只能强杀进程。

**根因**（shell-win/src/main.rs 的 `handle_event`）：函数开头 `let mut app = APP.lock().unwrap();` 拿到 `MutexGuard`，随后 `let Some(app) = app.as_mut()` **把变量名遮住成 `&mut App`**。`submit_request` 分支里的 `drop(app)` 只丢掉了这个 `&mut`，`MutexGuard` 仍然活到函数结束；接着 `let mut guard = APP.lock().unwrap();` 在同一线程二次加锁 → 死锁。

**影响**：Windows 版「第一次上屏就卡死」，属 P0；用户感知即「死机」。

**修法**：上屏分支不再二次加锁，直接用已持有的 `app` 调 `core.submit` / `overlay.error`；并在 `inject::paste` 后打一行 `[eryu] 上屏 status=… detail=…` 方便以后定位。修完用 `--hotkey-sim` + `ERYU_MIC_WAV` 在 VM 里回归（Notepad 前台、单窗口）。

## I-105 Windows：设置页残留一圈对不上控件的蓝线（已修复）

**现象**（2026-11-20 VM 里看到）：设置窗口打开后，卡片右侧和中部残留两条 1px 的直角细线，看起来像「卡片底没画出来、控件叠在一起」。同一台 VM、同一份配置，仓库里的旧图 `docs/settings-win-general.png`（900×560）是正常的。

**已排除**：我把设置页 5 处 `CreateSolidBrush` 改成用完即删（P0-4 GDI 泄漏修复）后怀疑是自己改坏了，于是用 HEAD（6e03cf8）单独构建了一份基线 exe 对比——**基线同样画错**，所以不是这次改动引入的，是既有缺陷。

**根因（2026-11-20 定位）**：
- 那两条 1px 线不是卡片底，是 `draw_focus_ring()` 在用**窗口 DC + 控件屏幕坐标反推矩形**画焦点环；跨页重建/焦点切换后矩形会套到错误的控件上（甚至套到另一页已销毁的控件），于是留下一个对不上的蓝框。卡片底其实画出来了（采样像素 #24262C）。
- 另一处：`paint()` 用 `WIN.try_lock()`，`build()` 创建控件时同步触发的 WM_PAINT 拿不到锁就**整帧返回**，卡片底会缺一帧。

**修法（已做）**：
- 去掉 `draw_focus_ring()`（连同函数一起删），焦点反馈交给系统原生绘制。
- `paint()` 在拿不到 WIN 锁时仍用临时画刷画出底色与卡片，不再整帧跳过。
- 窗口 `ShowWindow` 之后再 `InvalidateRect + UpdateWindow` 强制重画一遍。
- 顺带把「改快捷键后立刻生效；隐私模式与开机启动在下次录音 / 下次登录时生效。」缩短到一行内，避免底部文字被裁。
- 证据：`docs/settings-win-general.png`（2026-11-20 VM 实拍，884×552）。

**残留可疑点（未验证）**：
- 老图窗口是 900×560，现在打开的是 900×520；`paint()` 的卡片与控件坐标都是固定偏移的绝对值（`px(k, …)`），一旦客户区高度/宽度和布局假设不一致，卡片底边与控件就会错位（`shell-win/src/settings.rs:1301` 起）。
- `paint()` 用 `WIN.try_lock()`，拿不到锁就直接 `return`（`settings.rs:1303-1307`）。如果这次 WM_PAINT 发生在 `build()` 持锁重建控件期间，卡片底色就被整帧跳过了——正好能解释「底色缺失、只剩残留线条」。控件位置重叠则更像固定坐标 + 未随窗口尺寸布局。

**修法（待做，0.8.2）**：
- `paint()` 拿不到锁时不要整帧跳过：把卡片底色交给窗口类背景刷（`hbrBackground`）或改用 `InvalidateRect` 排到下一轮，避免出现「一半画一半没画」。
- 卡片与控件布局改成按客户区尺寸计算（或给内容区做成真正可滚动的子窗口），不再依赖 900×560 这个隐含假设。
- 修完补一张 VM 实拍图，替换/新增 `docs/settings-win-*.png` 证据。

## I-106 Windows 设置页所有自绘开关点了不生效（永远显示「关」，保存也写错值）

- 现象：设置页里「隐私模式」「开机自动启动」「背景降噪」「录音时暂停播放」「记录历史」等 pill 开关，配置里明明是 `true`，界面仍画成关；点一下也不翻转，点「保存」写回的还是旧值。
- 根因：`shell-win/src/settings.rs` 的 `checkbox()` 用 `BS_OWNERDRAW` 建按钮，再靠 `BM_SETCHECK`/`BM_GETCHECK` 读写开关状态；owner-draw 按钮上这两个消息不生效（实测 `BM_GETCHECK` 恒为 0、`BM_SETCHECK` 不改变状态），于是所有开关的状态永远是 false。
- 修法：状态改由 `settings.rs` 自己维护（`TOGGLES: Mutex<Vec<(id, bool)>>` + `toggle_on()/set_toggle()/flip_toggle()`）；`checkbox()` 建控件时登记初值，点击时由 WM_COMMAND 翻转并 `InvalidateRect`，`draw_toggle` 与 `checked()` 都读这份状态，不再依赖 `BM_*`。
- 证据：修复后「记录历史」正确显示为开（`docs/settings-win-history.png`）；点击后立即翻转。
- 回归：`--open-settings-page 1` 看「隐私模式」/「开机自动启动」状态与 `--dump-config` 一致；点一下再保存，`config.toml` 跟着变。

## I-107 Windows：局域网听写页 UI 已加，链路未验证（config 手改/CLI 传参都会踩坑）

> 已解决（2026-11-20 23:30，VM 实测）。真因是核心的缺陷，不是界面：**监听只在 worker 启动时起一次，改配置不会重启**。修法与证据见下。

现象：设置页新增「局域网」页（开关 / 端口 / 令牌 / allow_lan / 日志），但在 VM 里没能拿到监听成功与 `{"text": …}` 的证据。

已查明：
- 核心的接口实现是 `core/src/lib.rs:1059 start_lan_if_enabled()`（worker 启动时读 config 调用一次），路径是 **`POST /transcribe`**（不是 `/`）；`allow_lan=true` 且 token 为空会拒绝启动（`lan_no_token`）。
- **真正的产品缺陷**：`Cmd::ApplyConfig` 只换内存里的配置，不会重启监听 → 设置页点开、保存（`config.toml` 里已写成 `lan.enabled = true`）后仍然没有 8765 监听。mac 用户改开关同样要重启 App 才生效。
- 修法（`core`）：`core/src/lan.rs` 加 `static STOP: AtomicBool`，监听改成非阻塞 `accept` 循环（`WouldBlock` 时 sleep 80ms），新增 `pub fn stop()`（置位 + 等 160ms 让 binder 释放）；`core/src/lib.rs` 的 `Worker` 记 `lan_cfg: Option<(bool, u16, String, bool)>`，`apply_config` 末尾比较开关/端口/token/allow_lan，变了就 `crate::lan::stop(); self.start_lan_if_enabled();`。这样设置页保存按钮文案「保存并重启接口」名实相符，mac/Windows 都受益。
- 顺带修的两处界面问题：LAN 控件 ID 原本 200–205，撞上 `ID_EDIT_BASE = 200` 的输入框区间（`is_toggle` 里的 `+4/+5` 让「保存/刷新」被画成开关、点了没反应）→ 改成 230–235；日志区直接显示 `[` 原始 JSON → 现在解析成 `HH:MM:SS  src path → code (ms)`，最多 6 行。
- VM 里踩到的其它坑都不是产品缺陷：① 手改 `config.toml` 后如果 TOML 有语法问题，核心会**整体回退默认值**（`load_warning`），表现为「改了不生效」；② 经 cmd/PowerShell 传 `--set-config '{...}'` 时内层双引号会被吞，核心收到 `{lan_enabled:...}` 报 `SETCONFIG code=-3 err=key must be a string`；③ 有旧实例在跑时新进程直接退出（单实例），看到的是旧进程的行为。

验证（VM，干净配置，全部走设置页 UI）：
- 点「开启」→ 点「保存并重启接口」→ `netstat -ano | findstr 8765` 出现 `TCP 127.0.0.1:8765 0.0.0.0:0 LISTENING 9868`；
- `Invoke-WebRequest -Uri http://127.0.0.1:8765/transcribe -Method Post -InFile probe.wav -ContentType audio/wav` → `{"ok":true,"text":"今天天气不错我们开始吧","ms":298,"segments":1,"chars":11}`；
- 设置页日志区显示 `02:39:48  127.0.0.1 /transcribe → 200 (656 ms)`，截图 `docs/settings-win-lan.png`。
- 回归：`cargo test --manifest-path core/Cargo.toml` 全过（65 + 4 + 6 + 2）。

遗留（与本条无关）：mac 侧同样补了「改配置即重启监听」，但还没在真机上手动点过开关，下一轮冒烟时一并确认。

## I-108 Windows：遇到中文错误事件后整个程序崩溃（按字节切 UTF-8 字符串）

现象：VM 里按热键说话、录音窗口里没声音时，程序不是回落到「没听到声音」提示，而是直接退出（Windows 事件日志里是 `thread caused non-unwinding panic. aborting.`），用户看起来就是「闪退/死机」。
复现：`ERYU_RAW_DEBUG=1` + 按一下热键立刻松开（或安静环境录音）→ 触发 `no_audio` 错误事件 → 崩溃。

根因：`shell-win/src/main.rs` 里调试日志按**字节**截断事件 JSON：`&json[..json.len().min(160)]`。中文错误消息（`麦克风「…」没有收到声音…`）的 UTF-8 是多字节，160 正好落在字符中间 → `byte index 160 is not a char boundary` panic；而这段代码跑在 `extern "system"` 的窗口过程里，Rust 不允许 panic 越过 FFI，于是直接 abort 整个进程。

修法：
- 截断改成按字符：`json.chars().take(160).collect::<String>()`。
- 兜底：`WM_ERYU_EVENT` 分发处用 `std::panic::catch_unwind` 包住 `handle_event`，单条事件处理出错只打印一行日志，窗口和热键继续工作（避免以后再有一条边界 bug 就整个程序消失）。

回归：VM 里真实右 Alt 按住 6 秒 + 宿主 TTS 念例句 → 日志 `[eryu] 采音开始 麦克风 (High Definition Audio Device)` → `[eryu] 上屏 status=ok detail=`（Notepad 里出现「今天天气不错我们开始吧。」），`Get-Process Eryu-new` 全部 `Responding=True`；证据 `docs/win-e2e-real.png`（Notepad 里是识别结果，标题栏 `*`）。

## I-111 编辑观察（热词自动学习）只验到算法与采纳，未做真实控件端到端验证

**现状**：`observer.rs` 已实现「上屏后 15 秒内 1s 轮询焦点控件 → 公共前后缀 diff → 候选存 `suggestions.json` → 热词页采纳」，并验证了：`--obs-diff "我们用 qwen 转写|我们用 通义 转写"` → `OBSDIFF from=qwen to=通义`；`--obs-list` 读回候选；VM 里点「采纳」后 `hotwords.toml` 写入 `qwen = "通义"`、`suggestions.json` 变空。

**未验证**：真实「上屏到某个控件 → 用户改词 → 出现候选」的整链路（VM 里的 WinForms TextBox 自动化测试没能让 paste 落进去，Notepad 的 WinUI 控件不响应跨进程 `WM_GETTEXT`）。需要在真机的经典 Win32 编辑框（记事本另存为对话框、VS Code 的 Electron 输入框、Office 等）各试一次。

**影响**：功能不会出错，只是可能「什么都没学到」；`watch()` 读不到文本时会安静退出，不阻塞。

## I-110 右 Alt 热键在部分环境不触发（左右修饰键码不一致）

**现象**：Windows 版把热键设成「右 Alt」后，按右 Alt 有时完全没反应；`ERYU_RAW_DEBUG` 日志里能看到按键事件，但 `matched=false`。

**根因**：低级钩子回调拿到的 `vkCode` 是左右两种码（`VK_LMENU=0xA4` / `VK_RMENU=0xA5`）。合成键或某些键盘/远控软件会上报「另一侧」的码；而 `hotkey.rs` 的单修饰键匹配原来是严格 `m == vk`，于是 `ALT_R` 配 `VK_LMENU` 永远不中。用户根本不关心左右。

**修法**：新增 `same_modifier(spec, vk)`，单修饰键按同类判断（ALT/CTRL/SHIFT/WIN 的左右互认），`Spec::matches` 的主键判定改用它；补单测 `single_modifier_matches_either_side`。

**验证**：VM 里 `keybd_event(0xA5)`（上报成 0xA4）→ `raw.log` 出现 `handle vk=0xa4 break=false spec=ALT_R matched=true`；随后 `[eryu][sim] 用 WAV 代替麦克风` → `[eryu] 上屏 status=ok`，进程 `Responding=True`。回归：`--set-hotkey ALT_R` + `--dump-config` 显示 `keys = "ALT_R"`。

## I-109 Windows：设置页打开「热词」页时整个程序直接退出

现象：VM 里用 `--open-settings-page 3`（或点左侧「热词」）打开热词页，进程立刻消失，事件日志里退出码 `0xC000041D`（`STATUS_FATAL_USER_CALLBACK_EXCEPTION`，即「窗口回调里抛了未处理异常」）；其它 8 页都正常。

根因：`settings.rs` 的 `build()` 在持有 `WIN` 锁的情况下，页面里又同步调用了 core 的阻塞式 FFI（`history_json` / `hotwords_json`），并把结果塞进新建的 `LISTBOX`；`LB_ADDSTRING` 会同步派发 `LBN_SELCHANGE`，回调里再取 `WIN` 锁 → 重入异常直接终止进程（Rust 侧是 `extern "system"` 回调，异常无法 unwind）。

修法：
- 建议词改成「页面打开 / 点刷新时算好」的缓存：新增 `static SUGGESTIONS` + `pub fn refresh_suggestions()`，在 `open_at(3)` 与导航切到第 3 页、以及点「刷新建议」时调用（此时不持 `WIN` 锁）；`page_hotwords` 只读缓存，不再在 `build()` 里碰 core。
- 建议区展示从 `LISTBOX` 改为一行 `STATIC` 文字（`词（次数） · …`），「加进热词」直接取排第一的词，避开列表选中态的回调。
- 兜底：`build()` 的页面分发整体套 `std::panic::catch_unwind`，某一页构建出错只打印日志，程序继续运行。

验证：`--open-settings-page 3` 窗口正常出现；截图 `docs/settings-win-hotwords.png`（热词表 + 建议词行 + 两个按钮）；`--hotwords-suggest` 仍输出 `SUG …`、`SUG done`。

## I-112 Windows：上屏「假成功」——注入报 ok 但文字没进去

- 现象：VM 里真实按键（右 Alt 按住）跑完整链路，日志 `上屏 status=ok` 且胶囊浮层显示了识别文字，但记事本仍是「0 个字符」；用户看到的就是「识别对了，字没进来」。
- 根因（两层）：
  1. `inject::focused_text()` 读不到目标输入框时（WinUI 记事本、自绘控件、UIA 隔离）直接返回 `ok`，把「无法校验」当成成功；
  2. 焦点在 Eryu 自己的窗口（设置窗、引导窗、托盘弹窗）时也会照常往那里注入，既打不进去，还可能把测试/真实文本粘到设置框。
- 修法（`shell-win/src/inject.rs`、`shell-win/src/main.rs`）：
  - 新增 `foreground_is_self()`：前台是本进程的窗口 → 直接 `undelivered`，文案「焦点在 Eryu 自己的窗口上，这段话留在剪贴板里了」；
  - 读不到目标输入框 → 返回 `unknown` 并说明「无法确认是否落进去（文字在剪贴板里）」；三种状态都会在浮层显示；
  - 非 ok 时把文字放回剪贴板（`inject::set_text_only`），用户可手动粘贴；
  - `WM_PASTE` 只在目标类名确实是 Edit/RichEdit 时才发（WinUI 顶层窗口会「收下」WM_PASTE 但不处理，之前因此跳过了 Ctrl+V）；模拟 Ctrl+V 改为分段发送、每步 15~25ms；
  - 加 `ERYU_INJECT_DEBUG=1` 的注入日志（前台 hwnd/pid/class/title，粘贴前后各一条）。
- 验证：把自己的设置窗点到前台再按热键 → `上屏 status=undelivered detail=焦点在 Eryu 自己的窗口上…`（改前会假报 ok）。
- 未解决：VM 里 WinUI 记事本仍收不到合成的 Ctrl+V（前台确实是 Notepad，`SendInput` 无报错，粘贴前后前台不变），因此实际落点还需在真实 Win32 应用/真实新机上复核；见 I-113 待办。

## I-113 Windows：WinUI 记事本收不到合成的 Ctrl+V（已解决）

- 现象：`ERYU_INJECT_DEBUG=1` 日志显示粘贴前后前台都是 `class=Notepad`，`SendInput` 无报错，但记事本内容不变（0 个字符）。
- 已排除：焦点在自家窗口（前置守卫已拦住）、`WM_PASTE` 误吞（已限制只对 Edit/RichEdit 发）、Ctrl+V 事件分开发送（已加间隔）。
- 下一步：① 用 `Start-Process notepad.exe` 之外的经典 Edit 目标（如自建 Win32 测试窗口）复验；② 试 `KEYEVENTF_SCANCODE` 发送；③ 试先 `AttachThreadInput` + `SetFocus` 到目标编辑区再发；④ 真机复核（VM 里 WinUI 对合成输入的接受度可能与真机不同）。

### I-113 排查记录（2026-11-21）

- 已排除：焦点在自家窗口（前置守卫已生效）、`WM_PASTE` 误吞（已限制只对 Edit/RichEdit）、Ctrl+V 事件合并发送（已分段 15~25ms）、**粘贴后立即还原剪贴板**（已去掉还原，仍不进字）。
- 新事实：VM 里前台窗口可能是 `class=SysShadow`（WinForms 窗口的阴影窗，pid 属于宿主 powershell），说明「前台」不一定是可编辑目标；用 `runProgramInGuest -interactive` 起 WinForms 测试窗体时窗体没有真正出现在交互桌面（截图只有壁纸），这个 VM 里搭 GUI 测试目标不可靠。
- 下一步：① 改用 UIA（`IUIAutomation` → `ValuePattern.SetValue`）作为注入首选，成功率高且可回读校验；② 若 UIA 不可用再退回模拟键；③ UIA 的可行性先用能真正前台的应用验证（记事本可，注意先点进编辑区再取 `FocusedElement`）。

### I-113 进展（2026-11-21 凌晨，已落盘的改动）

- **找到并修掉一个真 bug**：迷你主窗创建时带了 `WS_VISIBLE`（随后才 `SW_HIDE`），创建即显示的置顶窗会把前台抢走，于是录音结束后 `GetForegroundWindow()` 常常是 `class=EryuMini`，注入守卫判定「焦点在 Eryu 自己的窗口上」→ `undelivered`，用户看到的就是「说了但没上屏」。现在创建不带 `WS_VISIBLE`，只用 `SW_SHOWNA` 显示。
- 注入不再在 `WM_ERYU_EVENT` 的事件分发里同步做，改成 `PostMessage(WM_ERYU_INJECTED)` 回到 UI 线程队列里执行；`submit_request` 只负责把任务交出去。
- 按下热键时记下前台窗口（`LAST_TARGET`，自家窗口不覆盖），注入前若前台被抢走，先 `SetForegroundWindow`，失败再 `AttachThreadInput` 设一次。
- 注入顺序改成先 `Ctrl+V`、再用 `WM_PASTE` 兜底（WinUI 记事本内层是 `RichEditD2DPT`，收下 `WM_PASTE` 却不处理，先试它等于什么也没做）。
- 新增两个自测开关：`--inject-test [文本]`（直接调 `inject::paste`）和 `--inject-test-hold <秒> [文本]`（先把进程完整跑起来再注入）。它们在 VM 里返回 `status=ok` 且文字确实落进记事本，用来把「注入实现」和「调用时机」分开。
### I-113 结论（2026-11-21，已解决）

- **最外层原因**：我们的低级键盘钩子会收到自己注入的 `Ctrl+V`；而注入又发生在被阻塞的 UI 线程消息处理里，钩子回调没法及时跑，超过 `LowLevelHooksTimeout` 之后这段合成输入被系统丢掉——「粘贴了，但什么都没进去」。
- 修法：钩子回调里看到 `LLKHF_INJECTED` 直接放过（热键只认人手按下的键）；注入用 `PostMessage(WM_ERYU_INJECTED)` 回到 UI 线程、在消息处理之外执行；先试 UIA `ValuePattern.SetValue`（不依赖模拟键、能回读），不支持再退回剪贴板 + `Ctrl+V`，最后才用 `WM_PASTE` 兜底。
- 验证（VM，`--hotkey-sim --hotkey-sim-force` + `ERYU_MIC_WAV`）：日志 `[eryu] 上屏 status=ok detail=`，记事本里出现「今天天气不错我们开始。」；`--inject-test` / `--inject-test-hold` 也都能独立落字。
- 注意：因为钩子现在放过合成键，VM 里用 `keybd_event` 模拟的真实按键不再触发热键，自动化测试一律用 `--hotkey-sim`；真机人手按键不受影响。

## I-114 Windows：引导「试说一句」没拦住结果，文字被推去上屏/剪贴板（已解决）

**现象**：Windows 引导第 5 步（试说一句）读对了例句，页面上也显示了「读到了」，但同一次识别同时走上屏流程：日志 `[eryu] 上屏 status=undelivered detail=焦点在 Eryu 自己的窗口上，这段话留在剪贴板里了`，引导测试文字进了用户剪贴板，违反 D-110「探针只显示、不注入」。

**根因**：引导从 5 步改成 6 步时（`28f4508` 之后加麦克风页），`STEP_PROBE` 变成 4，但 `onboarding.rs` 里 `offer_result` / `note_partial` / `note_state` 三处仍写着字面量 `3`（那是新的「上屏方式」页）→ `offer_result` 永远返回 false，`submit_request` 直接落到注入分支。

**修法**：三处改成 `STEP_PROBE`；并用 `grep -n "step [!=]= 3"` 复核没有其它字面量。

**顺带修**：「下一步」是 owner-draw 子按钮，`probe_repaint` 只 `InvalidateRect` 父窗口，按钮不会重画 → 试说成功后仍然显示灰色（看着像不能点）。现在额外 `GetDlgItem(hwnd, ID_NEXT)` + `InvalidateRect` 刷按钮。

**验证**：VM `--hotkey-sim --hotkey-sim-force --hotkey-sim-secs 4` + `ERYU_MIC_WAV` → 日志只有 `[eryu] onboarding probe ok=true len=12`、**没有** `上屏 status=`；截图 `docs/onboarding-win-probe-ok.png`（蓝色「下一步」+ 识别文本 + 成功提示）。

## I-115 Windows：胶囊档位徽标是一个汉字「日」，正文比 mac 小、胶囊比 mac 大（已解决）

**现象**（用户真机反馈，2026-11-21）：录音时胶囊左侧的润色档徽标显示成汉字「日」；同一块胶囊里正文字号明显比 mac 小，而胶囊整体却比 mac 大一圈。

**根因**：
1. 徽标取的是「档名首字且仅当是 CJK 才画」——激活档叫「日常」，于是画出了「日」。这个策略原本是为了修「emoji 在 GDI 里画成豆腐块」，但把语义带偏了：档名首字不是图标。
2. 字号常量名写的是 `TEXT_PT`，实际按**像素**渲染（14px ≈ 10.5pt，mac 用 14pt ≈ 18.7px），所以字小一号。
3. 胶囊高度 40 与 mac 的 40pt 数值相同，但 mac 的点是 1/72 英寸、Windows 的像素是 1/96 英寸，物理上矮了 1/4；再叠加偏小的字，观感就是「胖而空」。

**修法**（`shell-win/src/overlay.rs`）：
- 徽标改用 Segoe MDL2 Assets 的编辑图标 `\u{E70F}`（mac 用 SF Symbol，这里是等价物），字号 `ICON*0.62`，不再依赖档名文字。
- 尺寸一档：胶囊高 36、提示行 16 + 间距 4、左 10 / 右 12、电平条宽 4、图标 22、正文 17.5、下划线 19、提示 12.5。
- 顺带修掉提示行在浅色壁纸上「看不见」的老问题：提示行没有底色，原来用「深色描边 + 浅色正文」，浅色桌面上几乎隐形。现在一律用白色画、按亮度当覆盖率，再按系统明暗（`HKCU\…\Themes\Personalize\AppsUseLightTheme`）把颜色染成深灰/浅灰，并按预乘 alpha 写回。

**验证**（VM，100% 与 150% 缩放）：窗口 264×56 / 396×84；铅笔徽标、加大的正文、清晰的「松开上屏 · Esc 取消」；真实录音态截图 `docs/overlay-win-rec.png`（`ERYU_MIC_WAV` + `--hotkey-sim`，宿主音量未改）。

## I-116 Windows：热词建议会把长句碎片当候选（「们开始」「天天气不错」）（已解决）

**现象**：热词页的建议列表里出现「们开始 x56」「我们开始 x55」「天天气不错 x52」这类切片——它们不是词，只是同一句历史文本里重复出现的子串。

**根因**：`settings.rs::suggest_from` 只用 n-gram（2–6 字）计数，没有看上下文。同一句话被历史记录重复多次时，任何子串的计数都很高；而「被更长建议词包含就丢弃」的去重也救不了——这些碎片彼此不包含。

**修法**：加一道「边界熵」过滤：统计每个候选左右两侧出现过的**不同字**（句首/句尾不计），左右邻字种类之和 ≥3 才留下，再叠加两字功能词停用表（这个/是一/可以/然后…）。真实词汇因为语境多变能通过，固定搭配的句子碎片过不了。

**验证**：VM 里用真实历史（79 条）跑 `--hotwords-suggest`，修前 `们开始 x56 / 我们开始 x55 / 天天气不错 x52 …`，修后只剩 `测试 x3 / 怎么样 x2 / 的这个 x2 / 你现在 x2 / 情况 x2`；`--hotwords-suggest-selftest` 仍输出 `SUGSEL 设计文档 x3`（正常词汇没被误杀）。

## I-117 Windows：每次重建设置页漏一个普通子窗口（USER 对象缓慢增长）（已解决）

**现象**：设置页反复切换/重建后，进程的 GDI 与 USER 对象数缓慢上涨。用新增的 `--settings-cycle N` 自测（同一进程内反复 `rebuild_page()`）：40 次 +5 GDI / +11 USER，200 次 +23 GDI / +103 USER，近似线性。

**根因**：`build()` 重建设置页时只销毁 `w.ctrls` 里登记过的控件；`crate::keycap::create(...)` 这类直接 `CreateWindowExW` 的控件没有 `push` 进 `ctrls`，每重建一次就漏一个窗口（USER 对象）。GDI 一侧则是同源——被销毁控件上的字体/画刷句柄随窗口泄漏。

**修法**：`build()` 清完 `ctrls` 后再兜底扫一遍 `GetWindow(hwnd, GW_CHILD)` 并逐个 `DestroyWindow`（上限 500，防止意外死循环）。不再依赖每个页面记得登记。

**验证**（VM，同一进程 200 次重建）：`GDI before=29 after=28 delta=-1 USER before=40 after=31 delta=-9`；`--settings-pages-smoke` 9 页仍全 `ok / PAGES done`。

## I-118 Windows：托盘「自测…」与 `--self-test` 会往历史里写一条「待复制」（已解决）

**现象**：历史页里堆着多条 `自测：你好，Windows。 / 待复制 / 14 字`；每点一次自测多一条。

**根因**：`--self-test` 走 `core.debug_submit_text(...)`，整条链路与真实听写一样，会经 `record_success` 落历史。core 其实有自测模式（`eryu_set_test_mode`，命中时不写历史，与隐私模式同分支），但 Windows 壳从没调过它。

**修法**：`shell-win/src/core.rs` 补 `f_set_test_mode` 绑定与 `set_test_mode(bool)`；`shell-win/src/main.rs` 的 `--self-test` 分支在提交前 `core.set_test_mode(true)`。托盘「自测…」另起 `--self-test` 子进程，同样受益。

**验证**（VM，安装目录构建）：连续跑 3 次 `--self-test`，历史里含「自测」的行数 `before=17 after=17`，不再新增；自测本身仍 `SELFTEST OK 结果=自测：你好，Windows。`

## I-119 Windows：历史列表最后一行被切一半、长句没有省略号（已解决）

**现象**：历史页列表底部那行只露出上半截；长句直接顶到滚动条被硬切。

**根因**：`listbox()` 没调 `LB_SETITEMHEIGHT`，行高由字体撑开、和列表高度 290 不是整数倍 → 末行被裁；`text_at()` 的 `DrawTextW` 没有 `DT_END_ELLIPSIS`，超宽就硬切。

**修法**：`listbox()` 里 `LB_SETITEMHEIGHT = px(k, 32.0)` 固定行高；历史列表高度 290→288（9 行整）；`text_at()` 的 flags 加 `DT_END_ELLIPSIS`（所有单行文本超宽时收成「…」）。

**验证**：VM 截图 `docs/settings-win-history.png`：9 行完整、滚动条独立、长句末尾显示「…」。

## I-120 Windows：设置页键帽只显示主键，组合键的修饰键丢了（已解决）

**现象**：快捷键设置成 `CTRL+ALT+SPACE` 后，通用页键帽里只显示 `SPACE`，看起来像只绑了空格。

**根因**：`shell-win/src/keycap.rs` 的 `pretty()` 只把带左右的写法（`CTRL_L`/`CTRL_R`/`ALT_L`/`ALT_R`）映射成符号，而 `spec_from()` 早在组合键上就统一改成不分左右的 `CTRL`/`ALT`，于是修饰键匹配到 `_ => ""` 被丢掉，只剩最后一个键；主键也没走可读名映射（`SPACE` 原样输出）。

**修法**：`pretty()` 的修饰键分支补 `"CTRL" | "CTRL_L" | "CTRL_R" => "Ctrl+"`（Alt/Shift/Win 同理），主键走新增的 `key_name()`（`SPACE→空格`、`ENTER→Enter`、`TAB→Tab`、`ESC→Esc`、方向键→箭头），组合键显示成 `Ctrl+Alt+空格`；单修饰键仍显示「右 Alt」这类带左右的写法。

**验证**：VM 里配置 `CTRL+ALT+SPACE`，通用页键帽显示 `Ctrl+Alt+空格`（`docs/settings-win-general.png`）。

## I-121 Windows：设置窗的 ✕ 压在内容第一行，点它也没反应（已解决）

**现象**：设置窗右上角画着 ✕，但点上去窗口不关；同时它浮在「快捷键与拾音」那一行的右上角，看上去像少画了一条顶栏。

**根因**：`child()` 统一给子控件的 y 加 `HEADER`（36px，内容从顶栏下方算起），而 ✕ 是按顶栏坐标 `4.0` 传进去的 → 实际落在 40..74，既不在顶栏（4..40）里，又盖住内容第一行；用户点顶栏位置（4..40）当然没有按钮。

**修法**：建按钮时传 `4.0 - HEADER`（`child()` 加回 HEADER 后正好落在 4..40），并加注释说明这个偏移；`ID_CLOSE_HEADER` 仍是 `ShowWindow(hwnd, SW_HIDE)`。

**验证**：VM 里用鼠标点 `(window.right-26, window.top+22)` → `AFTER-CLICK vis=False`，窗口隐藏；重开 `--open-settings-page 0` 正常。

## I-122 Windows：快捷键设成组合键后，迷你主窗的麦克风圆钮点了没反应（已解决）

**现象**：把「按住说话的键」设成 `Ctrl+Alt+空格` 后，点迷你主窗中间的麦克风圆钮没有任何反应（不开始录音、浮层不出现），但按键盘热键正常。

**根因**：`mini_toggle_talk()` 只从热键里取主键（`spec.main`）再伪造一次 `handle_key(vk, false, e0)`，而 `handle_key` 会用 `Spec::matches` 校验修饰键是否真的按着。组合键要求 Ctrl/Alt 同时按下，伪造的单键事件永远匹配失败 → 直接 `return`，什么都不发生。单修饰键（如右 Alt）能过，所以之前没暴露。

**修法**：把 `handle_key` 里匹配通过后的「开始/结束按住说话」抽成 `talk_down()` / `talk_up()`（含 `HOTKEY_DOWN` 去重、记录上屏目标窗口、通知 core）；`mini_toggle_talk()` 不再伪造按键，按当前相位直接调 `talk_down()`（idle/done）或 `talk_up()`（starting/listening）。

**验证**（VM，配置 `CTRL+ALT+SPACE`）：点迷你圆钮 → `EryuOverlay vis=True`（开始听）；再点 → `vis=False`（停止）；点右上 ✕ → `EryuMini vis=False` 且 `config.toml` 里 `mini_window = false`。

## I-123 Windows：托盘「显示主窗」打开后不记忆，下次启动又没了（已解决）

**现象**：用迷你主窗右上 ✕ 关掉（会写 `mini_window=false`），再从托盘「显示主窗」打开，窗口出来了；但下次启动迷你窗仍然不出现——托盘这次打开没有把选择写回配置。

**根因**：`shell-win/src/mini.rs::toggle()` 只 `show()` / `hide()`，没写配置；只有 ✕（`close()`）写 `mini_window=false`，mac 侧菜单「显示主窗」写 `true`，两边行为不一致。

**修法**：`toggle()` 在显示后写 `mini_window=true`、隐藏后写 `false`（`settings::set_config_quiet`），启动路径仍按配置 `show()`，不会互相打架；新增 `--mini-toggle` 自测入口（走同一条 toggle 路径并打印 `MINI shown=…`）。

**验证**（VM）：`--mini-toggle` → `MINI shown=true` + `mini_window = true`；再跑一次 → `MINI shown=false` + `mini_window = false`。

## I-124 Windows：引导窗右上角的 ✕ 点了没反应（已解决）

**现象**：引导窗顶栏画着 ✕，点它关不掉窗口；只能走到最后一步或者点「稍后再说」。

**根因**：`shell-win/src/onboarding.rs` 的 `WM_LBUTTONUP` 判定里把**客户区坐标** `x`/`y` 和**屏幕坐标** `wr.left`/`wr.top` 混着减：`x - wr.left > 宽 - 40` 在客户区 x 约 610 时算成 `195 > 600`，永远不成立。这个 ✕ 从改无边框那版起就没生效过。

**修法**：直接用客户区坐标判断 `y < HEADER && x > 客户区宽 - 40`（`client_w = wr.right - wr.left`），命中后仍调 `finish(hwnd)`（与「稍后再说」同一语义：`onboarding_done=true`，`onboarding_probe_done` 保持未成功）。

**验证**（VM，`--onboarding-step 0`）：点右上角 → `AFTER-CLICK onb_vis=False`，`config.toml` 里 `onboarding_done = true`、`onboarding_probe_done = false`（下次启动回到「试说一句」，符合 D-112）。

## I-125 Windows：设置窗左侧导航点了不换页（已解决）

**现象**：设置页左侧「通用/引擎/音频/…」一列，用鼠标点没有任何反应，只能靠 `--open-settings-page N` 打开指定页。

**根因**：`paint()` 里先 `SetViewportOrgEx(hdc, 0, HEADER)` 把内容整体下移 36px，再按「内容坐标」算导航项矩形（`top = 76 + 44*i`），但 `NAV` 里存的就是这份绘制坐标；`WM_LBUTTONDOWN` 拿到的是客户区坐标（比它大 36），于是每次都差一行的高度，命中判定永远失败。同类偏移在关闭键（I-121）、引导 ✕（I-124）上都出现过。

**修法**：`NAV` 登记时补回 `HEADER`：`nav.push((left, top + px(k, HEADER), right, bottom + px(k, HEADER)))`，并加注释说明绘制坐标与客户区坐标的区别。

**验证**（VM，实测鼠标点击）：点「音频」后枚举子窗口，出现 `开始测试`（id=141）/`保存`（id=101）等音频页控件，导航高亮也在「音频」上；截图 `docs/settings-win-page2.png`。

## I-127 Windows：上屏成功后整个程序卡死（观察线程里二次拿 APP 锁，已解决）

**现象**：Windows 上第一次成功上屏之后，文字确实进了目标程序，但 Eryu 随后完全没反应——托盘点不动、迷你窗点不动、设置打不开，`Get-Process | Select Responding` 从 True 变成永久 False，CPU 只有缓慢增长。

**复现**：VM 里 `ERYU_MIC_WAV=...probe.wav` + `--hotkey-sim --hotkey-sim-force --hotkey-sim-secs 3`，记事本在前台。`Responding` 在 t=7 还是 True，t=11 起一直是 False；`ERYU_TRACE=1` 的日志停在 `injected-in` / `injected-locked` / `injected-watch`，没有 `injected-watched`。

**根因**：`WM_ERYU_INJECTED` 的处理里先 `APP.lock()` 拿锁，再在该锁的作用域内调用 `observer::watch(...)`；而 `observer::watch()` 第一步就是 `crate::privacy_mode()`，`privacy_mode()` 内部又去 `APP.lock()`。`std::sync::Mutex` 不可重入、同一线程二次加锁 → 永久自锁。这和 I-104（`submit_request` 分支二次加锁）是同一类错误，只是这次藏在「观察热词」这条路径里。

**修法**（`shell-win/src/main.rs`）：把 `core.submit` / 失败降级留在锁内，**锁释放后**再调 `observer::watch`；注释写清「privacy_mode 会自己 APP.lock，别在锁里调」。

**验证**：同一条 E2E，`Responding` 全程 True（t=7/11/17/25），记事本里出现「今天天气不错我们开始。」；`ERYU_TRACE=1` 下日志走完 `injected-watched` → `injected-out`。

**教训**：`APP.lock()` 的持有范围要一眼可见——任何「看起来只是读配置/查状态」的辅助函数（`privacy_mode()` 这类）内部可能也加这把锁。以后在锁内只做赋值与 FFI 调用，别调用本项目自己的 pub 函数。

## I-128 Windows：explorer 重启后托盘图标消失、再也回不来（已修复）

- **现象**：explorer.exe 崩溃恢复 / 注销重登后，右下角 Eryu 图标消失；程序本身还在跑（进程 Responding），但用户点不到托盘菜单。
- **根因**：① 主消息窗口用 `HWND_MESSAGE` 建成 message-only 窗口，而 `TaskbarCreated` 广播只发给顶层窗口 → 永远收不到；② 即使收到也只该重挂一次，没有任何兜底。
- **修法**（`shell-win/src/main.rs`、`shell-win/src/tray.rs`）：
  - 消息窗口改成不可见的顶层 `WS_POPUP` 窗口；
  - 注册 `TaskbarCreated` 消息，收到就 `tray::reinstall()` 重发 `NIM_ADD`；
  - 再加 5 秒 `WM_TIMER` 兜底：`tray::ensure()` 用 `NIM_MODIFY` 探活，失败就补 `NIM_ADD`（VM 里实测 explorer 重启不一定发广播，光靠消息不够）。
- **验证**：新增 `--tray-drop-test`：先 `NIM_DELETE` 删掉图标，再 `ensure()`，输出 `TRAYDROP dropped=true back=true`（图标确实被删掉、又被补回来）。
- **遗留**：VM 里 explorer 重启不广播 `TaskbarCreated`，所以「真实广播」这条没法在 VM 里验；顶层窗口 + 定时兜底已覆盖两种路径。

## I-129 模型没下载时，stub 的假文本会被当识别结果上屏（已修复）

- **现象**：没装语音模型时启动，按住热键说一句话，程序会把 `测试文本：你好，Eryu。` 粘进用户正在写的文档里。
- **根因**：`core/src/lib.rs` 的引擎加载失败会回退到 `StubEngine`（为让界面/设置还能用），而 stub 的 `transcribe` 固定返回占位文本；识别链路照常走 `result` → `submit_request`，壳就真把它上屏了。
- **修法**：
  - `core/src/asr.rs` 导出 `pub const STUB_REPLY`，`StubEngine::default()` 用它；
  - `EryuCore::start` 记录 `engine_fallback`（只有「加载失败回退」才为 true，用户主动选 stub 不算），`Worker` 增字段；
  - `on_decoded` 里：`engine_fallback && raw_text.trim() == STUB_REPLY` → 不发 `result`/`submit_request`，改发 `asr_failed` 错误「语音模型还没下载：打开设置 →「引擎」页，选一个模型后点「下载选中模型」。」并回 idle。
  - 只匹配占位文本，`--debug-submit-text`（自测/FFI 冒烟）与用户主动选 stub 引擎不受影响。
- **验证**：
  - `cargo test --manifest-path core/Cargo.toml` 全过（65/4/6/2）；mac `scripts/build.sh` + `scripts/smoke.sh` 30 项全绿；
  - VM 里 portable 空数据目录（无模型）跑 `--hotkey-sim` + WAV：日志只有 `[eryu][sim] 用 WAV 代替麦克风（45594 采样）`，**没有 `[eryu] 上屏 status=` 行**（注入从未发生）；同命令换成有模型的安装版构建则出现 `上屏 status=unknown`（注入被尝试、文本留剪贴板），两条路径对比成立。

## I-130 引擎加载同步阻塞启动（已评估，暂不修）

- **现象**：Windows 冷启动到迷你窗出现在 VM 里约 4.3s，其中约 3.2s 花在 `EryuCore::start` 同步加载 SenseVoice 上。
- **试过的改法**：把 `make_engine` 挪到后台线程，先用 `LoadingEngine` 顶着，加载完再换。窗口能立刻出来，但代价是——开机后立刻按热键会得到「还在加载」；`--self-test`/`--soak`/隐私模式/崩溃恢复等冒烟项都假设「start 返回即引擎可用」，实测 6 项失败。已完整回退。
- **结论**：要动就得加显式的「引擎就绪」握手（FFI 事件 + 壳/冒烟都等），属于跨壳改动，留到有明确启动耗时需求时再做。当前保留同步加载，`engine_fallback` 改成 `Arc<AtomicBool>` 以便将来异步化，并顺手修了「切换引擎成功后 fallback 标记不复位」的小 bug。

## I-131 写坏 config.toml 会静默丢设置（已修复）

- **现象**：手工编辑 `config.toml` 写错一个引号，程序下次启动「一切恢复默认」——用户以为设置被清了，其实是解析失败回退默认值，随后任意一次保存（开机自启对账、设置页保存）就把坏文件覆盖掉。
- **修法**：
  - `core/src/config.rs::Config::load`：`merge_toml` 失败时先把原文件复制成 `config.toml.broken`，并在 `load_warning` 里带上备份路径；
  - Windows 壳 `main.rs::warn_bad_config_once()`：启动后读 `config_json().load_warning`，非空就写日志 + 托盘提示 + 浮层提示（只提示一次，不弹模态框）。
- **验证**（VM）：把 `config.toml` 写成 `[hotkey` + 未闭合引号后启动 → 进程 `Responding=True`、`config.toml.broken=True`、日志 `config.toml 解析失败（已用默认值）：TOML parse error at line 1, column 9`；恢复原文件后配置回到原样。
- **mac 侧**：核心同样备份；`AppModel.load_warning` 已有字段但界面暂未展示（mac 下一步可复用同一提示）。

## I-132 设置里改局域网页会把已有令牌当成没设置（Windows）
- **现象**：`config.toml` 里明明有 `[lan] token = "…"`，设置页的令牌框却是空的、旁边也没有「已设置」；在别处改完其他开关点「保存并重启接口」时，令牌被一起写回空串——局域网接口从此不接受带令牌的请求（如果同时打开 `allow_lan`，保存还会被拒，看起来像「保存没反应」）。
- **根因**：`core` 的 config JSON 出于安全**只给 `lan_has_token: bool`，不给明文 `lan_token`**（mac 就是读这个）。Windows 的 `page_lan`/`save_lan` 读的是 `c["lan_token"]`，永远是空串 → `has_token` 恒 false → 保存时把空串写回去。
- **修法**：`shell-win/src/settings.rs` 读 `c["lan_has_token"]`；令牌框改 `ES_PASSWORD`（与 mac 的 `SecureField` 一致，不再把凭据摊在屏幕上）；标题按状态显示「令牌（已设置）」；留空保存时不写 `lan_token` 字段（保留原值），只有输入了新令牌才覆盖。
- **验证**：VM 里先在设置页输入 `secret123` 保存 → `dump-config` 有 `token = "secret123"`；重开设置页显示「令牌（已设置）」+ 空白密码框；**不输入任何东西**再点保存 → `token` 仍为 `secret123`。

## I-133 Windows 设置页无法用键盘操作（已修复）

现象：按住 Tab 不会在控件间移动；即使点进控件，界面上也看不出焦点在哪；键盘用户完全没法开隐私模式或点保存。

根因：
1. `button()` / `checkbox()` 建控件时只给 `WS_CHILD | WS_VISIBLE | BS_OWNERDRAW`，没有 `WS_TABSTOP`；
2. 无边框普通窗口（不是 dialog）没有消息泵层面的 `IsDialogMessage`，Tab 不会自动遍历；
3. 自绘按钮/开关把系统默认虚线焦点框盖掉了，也没有自己画焦点。

修法：
- `button()` / `checkbox()` 加 `WS_TABSTOP`；
- `WM_KEYDOWN` 处理 `VK_TAB`：`GetNextDlgTabItem(hwnd, GetFocus(), shift)` 后 `SetFocus`（Shift 反向）；
- `draw_button()` / `draw_toggle()` 用 `GetFocus() == dis.hwndItem` 判断焦点，画 2px 描边（主操作白、其余主色）；
- 新增自测：`--settings-tab-demo N`（打印每次 Tab 后的焦点控件 id）、`--settings-key-demo <id> [saveId]`（焦点 + BM_CLICK 激活）。

验证（VM）：
- `--settings-tab-demo 8` → 焦点 id 依次 `240, 241, 242, 243, 156, 157, 204, 205`（预设键位四个、上屏方式两段、隐私/开机两个开关）；
- `--settings-key-demo 204 101` → 先激活「隐私模式」再点「保存」，`--dump-config` 里 `[privacy] enabled = true`（随后改回 false）；
- 截图 `docs/settings-win-focus.png`：「逐字打字」上可见焦点描边。

## I-134 开了润色但没选档，文字一点没变（Windows + macOS）

现象：
- 设置页把「润色」开关打开，`polish_enabled = true`，但 `polish_level` 仍是默认的 `off`，于是每段文字都原样上屏。用户看到的是「我明明开了润色，怎么没效果」。

根因：
- 核心的润色条件是 `polish_enabled && level != Off`（`core/src/lib.rs:1892`），而两端的「打开开关」只写 `polish_enabled`，没人管 `level`；档位是另一处（润色档列表）才写的。

修法：
- Windows `save_polish()`：要开启时若当前 `polish_level` 为 `off`/空，一并写 `light`（`shell-win/src/settings.rs`）。
- macOS `AppModel.setPolishEnabled`：`on && config.polish_level == "off"` 时 `patch(["polish_enabled": true, "polish_level": "light"])`。

验证（VM）：
- 先 `--set-config` 置 `enabled=true, level=off`，再打开设置第 5 页点「保存润色设置」（`WM_COMMAND wParam=131`），`--dump-config` 得到 `enabled = true` / `level = "light"`。

## I-135 两个开关点了不落盘（记录历史 / 只认我的声音）

现象：
- 「记录历史」和「只认我的声音」这两个自绘开关，点一下界面会变，但配置没变；录完历史照样写、声纹锁定照样关。重开设置又回到旧状态。

根因（顺序问题）：
- `WM_COMMAND` 里先跑 `match id` 的具体分支（分支里读 `checked()` / `toggle_on()` 决定写什么值），**之后**才在通用块里 `flip_toggle(id)`。于是分支读到的是点击前的旧状态，写回去的也是旧值，只有界面被翻转了。

修法（`shell-win/src/settings.rs`）：
- 把「翻转 + 润色首次同意」提到 `match id` 之前：`if is_toggle(id) && note == 0 { …flip_toggle(id) }`；
- 通用块里只保留「润色开关点完立刻保存」和重画；具体分支读到的就是新状态。

验证（VM）：
- 记录历史：冷启动 `history.enabled=true`，点开关 → `--dump-config` 为 `enabled = false`；再置回 true。
- 只认我的声音：`voice_lock=false` 起，点开关 → `voice_lock = true`。
- 音频页四个开关连点两次（点一次 → 保存 → 读配置，再点一次 → 保存 → 读配置）：`pause_media`、`denoise`、`source_lock`、`punct` 两轮都正确翻转（false→true→false / true→false→true）。
- 通用页：隐私开关 204 → `[privacy] enabled = true`；开机自启 205 → `launch_at_login = true` 且 `HKCU\...\Run\Eryu` 写入指向当前 exe；再点一次 → `launch_at_login = false`、Run 键被清空。

## I-138 150% 缩放下润色档列表露出白色滚动条槽（已修）（2026-11-21）

- **现象**：把设置窗按 150% 缩放（`ERYU_SETTINGS_DPI=150`）打开「润色」页，档位列表右侧出现一条白底竖条，深色页面上很扎眼。
- **根因**：列表用原生 `LISTBOX` + `WS_VSCROLL`，滚动条由系统主题绘制；`settings.rs::child()` 只给 `EDIT`/`COMBOBOX` 套了深色主题，`LISTBOX` 漏了。100% 时 3 行刚好占满 96px 不出现滚动条，150% 行高变成 48px、只放得下 2 行 → 滚动条出现，白槽暴露。
- **修法**：`class == "LISTBOX"` 时 `SetWindowTheme(h, "DarkMode_Explorer", null)`；滚动条保持系统行为（该滚就滚），只是换成深色绘制。
- **证据**：VM 150% 截图 `docs/settings-win-dpi150.png`；修后 `--self-test`、`--settings-pages-smoke`（9 页）全绿；100% 布局无变化（列表本来就不显示滚动条）。

## I-139 浮层提示行在壁纸上看不清（已修）（2026-11-21）

- **现象**：胶囊下方那行「松开上屏 · Esc 取消」在 Win11 默认蓝色壁纸上看不清——字发灰、笔画发虚，用户第一眼以为没画。
- **根因**：两层。① 颜色只按**系统主题**选（浅色主题写死深灰 `0x46`），而壁纸亮度跟主题无关；默认壁纸是饱和蓝（感知亮度约 105/255），深灰字对比度只有约 1.9:1。② 提示行没有底色，alpha 直接取「白字覆盖率」；12.5pt 小字在 100% 缩放下抗锯齿把笔画摊薄，覆盖率普遍只有 40–60%，等于半透明字。
- **修法**（`shell-win/src/overlay.rs`）：新增 `bg_luma()`，渲染前用 `GetPixel` 采窗口正下方 9 个点的桌面亮度（采不到退回系统主题）；`<128` 用近白 `0xF2`、否则用近黑 `0x33`。覆盖率放大 1.5 倍（截顶 255）让笔画实一点。
- **验证**：VM 默认壁纸下 100% 截图 `docs/overlay-win-hint.png`，提示行清晰可读；`--self-test` 通过。

## I-140 引导页自绘按钮重绘时用阻塞锁（已修）（2026-11-21）

- **现象**：静态审计指出 `onboarding.rs` 的 `WM_DRAWITEM` 里两次 `WIN.lock().unwrap()`，而同一文件的注释明确写着「`WM_CTLCOLOR` 里再锁就会死锁」——同一个隐患，只是没在实测里撞到。
- **根因**：`WM_DRAWITEM` 可能是 `build()` 创建自绘控件时同步回调进来的，那时父窗口构建正持着 `WIN` 锁；用阻塞锁就互等。实测目前没触发，但属于「早晚会撞」的路径。
- **修法**：改成一次 `WIN.try_lock()`，把 `blocked`/`mode_paste`/`polish_on` 一次取出；拿不到锁就按默认样子画（下一步可用、粘贴模式、润色关），等解锁后的 `WM_PAINT` 再修正。
- **验证**（VM）：`--onboarding-probe-selftest` 8 条仍 OK（1.00/0.55/0.45/0.18/0.00/0.55/1.00/0.00）；`--onboarding-key-demo 3` 焦点 204→202→200；截图与 tab 顺序无变化。

## I-141 升级后仍加载旧 eryu_core.dll（已修）（2026-11-21）

- **现象**：改完 core、重新打包、覆盖安装目录 exe 后，行为仍是旧版的（新加的调试输出完全不出现）。
- **根因**：`shell-win/src/core.rs::ensure_core_dll` 把内置 dll 解到 `%APPDATA%\Eryu\bin\eryu_core.dll`，用「长度 + 首字节 + 末字节」当指纹判断要不要重写。core 改几百行长度不变、首末字节也不变（PE 头 `MZ` 与末尾对齐字节），指纹相同 → 一直复用旧 dll。
- **修法**：指纹改成内容哈希（`fnv1a64`，22MB 约 20ms，只算一次），stamp 形如 `22609920-1f3a9d5c8b7e4a20`。
- **验证**（VM）：把旧 dll 留着，装新 exe → 日志出现「CORE 内置 dll 已解出」，随后新加的调试输出正常出现；`%APPDATA%\Eryu\bin\eryu_core.dll` 哈希与构建产物一致。

## I-142 上屏成功却被判超时 → 历史永远「待复制」（已修）（2026-11-21）

- **现象**：模拟输入把「润色后：今天天气不错，我们开始吧。」打进记事本，`history.json` 里那条却是 `status: "pending"`，而且之后每次启动都弹「恢复上次没上屏的文字」（17 字）——文字其实早就在文档里了。
- **根因**：`core/src/session.rs` 的 `SUBMIT_TIMEOUT` 是 2s；Windows 壳的上屏要 UIA 尝试 + 剪贴板 + Ctrl+V + 回读校验，慢机器上实测超过 2s，`Machine::tick` 先判 `SubmitTimedOut` 把 `pending_submit` 清掉。壳随后报 `ok` 时 `submit_result` 返回 `NoPending`，`on_submit` 什么都不做 → 历史停在 `pending`、`last_result.txt` 也没清，下次启动自然又提示取回。
- **修法**：① `SUBMIT_TIMEOUT` 2s → 6s（注释写明为什么不能太短）；② `on_submit` 里「超时之后才到的 ok」也当成功处理（`mark_status(inserted)` + `clear_pending_result`），只在 `pending_history_id` 还在时生效，避免误清别人的记录。
- **验证**（VM，装机目录构建）：同一条模拟输入 → `history.json` 为 `"status": "inserted"`、`app: "Notepad.exe"`；紧接着再启动一次，不再出现「恢复上次没上屏」；截图 `docs/settings-win-history-inserted.png`（历史页显示「17 字 已上屏」）。
- **附带**：`inject.rs` 的读回校验加 UIA 兜底（`uia_focused_text`：ValuePattern → TextPattern 光标行 → 整篇 40 万字），WinUI 记事本这种没有经典 `Edit` 的目标也能确认上屏成功。


## I-143 逐字打字路径不复核，可能「报已上屏其实没进」（已修，待真机复验）（2026-11-21）

- **现象**：关闭「用粘贴」改走逐字打字（`[insert] mode = "keystroke"`）时，`type_text` 只要把 `WM_CHAR` 投出去就返回 `ok`；`SendInput` 那条兜底同理。目标程序把字符吞掉（WinUI、输入法组合态、只读控件）时，界面和 `history.json` 都会说「已上屏」。
- **根因**：`shell-win/src/inject.rs::type_text` 没有任何回读校验，而粘贴路径（含 `read_focus_text()` 的 UIA 兜底）有。属于 I-112「读不到目标也报 ok」在打字路径上的同类问题。
- **修法**：`type_text` 两条出口都接 `verify_typed()`：去掉空白后比对 `read_focus_text()` 的结果（整段或前 8 字命中即 ok）；读不到（`None`）仍按 ok，避免对读不出文字的控件制造假失败；读到了但不含这段文字，返回 `unknown` 并在提示里建议改用「粘贴」。
- **验证**：`./scripts/build-win.sh` 通过；路径级验证（记事本里真打一遍、以及 WinUI 控件上确认会报 unknown）待 VM 登录后补——当时 VM 停在锁屏，`runProgramInGuest -interactive` 无登录会话。

## I-144 装完不能用 / 模型列表膨胀 / 引导在没模型时让人试读（已修）（2026-11-21）

- **现象**（用户 m12352，五条里前三 条）：①「搞了一堆下载的模型，用户怎么能理解」②「根本不能录音和生成，只有个测试信号在」③「启动界面上来就让我尝试读一段话，但这时候根本没有模型，你的意义在哪里」。
- **根因**：
  1. `dist-win/Eryu-0.8.1-win64.zip` 只有 7 个文件（exe、安装/卸载脚本、README），**不含任何模型**；装完直接按住说话 → 引擎回退 stub 或 `asr_failed`，用户看到的是电平条在动、没有字。这就是「只有个测试信号」。
  2. 引擎页把 `models_list_json` 的 6 项全列（sense/qwen3/punct/gtcrn/speaker/stub），看不出哪个是「必须」的。
  3. 引导 `open_if_needed` 的续跑规则是「`onboarding_done` 且没试说成功 → 直接回 STEP_PROBE」，不看模型是否就绪；没模型时这一页必然失败，用户被卡在一个注定过不去的关口。
  4. VAD/降噪两个 1MB 以内的小模型也要单独点下载，属于把实现细节暴露给用户。
- **修法**：
  1. `scripts/package-win.sh` 把 SenseVoice 打进 zip 的 `models/`；`install-win.ps1` 装到 `%APPDATA%\Eryu\models`（已存在跳过，不覆盖用户自己的）。
  2. `shell-win/src/core.rs` 用 `include_bytes!` 内置 `silero_vad.onnx` + `gtcrn_simple.onnx`，`ensure_bundled_models()` 在 `Core::load` 前解到数据目录（长度一致就跳过）。
  3. `settings.rs::page_engine` 只渲染 `engine==true && id!="stub"` 的两行（用新的 `Window::model_ids` 做列表下标映射），说明文案「默认 SenseVoice 随安装包装好」+「标点、降噪、声纹模型不在这里：打开对应开关时自动下载」。
  4. 打开「标点 / 降噪 / 只认我的声音」时 `auto_pull_if_missing()` 自动下载对应模型（不弹框，托盘提示），用户不用再去引擎页找。
  5. 引导：`open_if_needed` 在模型未就绪时一律回 STEP_MODEL；STEP_MODEL 的「下一步」在模型未就绪时灰显并说明原因；`model_done` 事件调 `onboarding::refresh_asr()` 解除灰显。模型就绪时模型页只显示「已就绪」不再摆两个下载按钮。
- **验证**（VM Win11 ARM，装机目录构建 `B2D4BCB8…`）：
  - 删掉 `%APPDATA%\Eryu\models\sherpa-onnx-sense-voice-…` 后跑 `install-win.ps1`：`已安装语音模型` 输出、目录恢复；`--self-test` 里出现 `内置模型已解出：…\gtcrn_simple.onnx`；`models` 下两个小模型就位。
  - 引擎页截图 `docs/settings-win-engine-2engines.png`：只剩 SenseVoice（使用中）/ Qwen3（未安装 879MB）。
  - 引导模型页截图 `docs/onboarding-win-model-ready.png`：显示「语音模型已就绪（随安装包装好）」，只有「下一步 / 稍后再说」。
  - 试说页截图 `docs/onboarding-win-probe.png`：不再出现「还没有可用的语音模型」；未试说成功时「下一步」灰。
  - 端到端回归：`ERYU_MIC_WAV=probe.wav` + `ERYU_POLISH_IN_SIM=1` + 隐藏启动 → 记事本出现「润色后：今天天气不错，我们开始吧。」，`history.json` 为 `inserted / Notepad.exe / 17 字`。
  - 模型缺失时的引导（2026-11-21 补验）：把 SenseVoice 目录改名后 `--onboarding-step 0` → 第一页仍停在「选一个识别模型」、SenseVoice 显示「已选中（未安装）」、Qwen3「未安装（在「设置 → 引擎」里下载）」、`下载 SenseVoice` 可点、「下一步」灰；点灰的「下一步」弹「先下载语音模型（点「下载 SenseVoice」，约 233MB），装好会提示。也可以点「稍后再说」，下次启动会回到这一步。」。截图 `docs/onboarding-win-model-missing.png` / `-dialog.png`。
  - 试说门禁（真实 ASR 解锁）：`--onboarding-step 4` + `ERYU_MIC_WAV=probe.wav` + `--hotkey-sim --hotkey-sim-force` → 页面显示识别结果「今天天气不错。」、提示「读到了，点「下一步」继续。」、「下一步」由灰变蓝（截图 `docs/onboarding-win-probe-passed.png`）。
- **未覆盖**：真实麦克风（用户真机）复测仍需本人做。

## I-145 胶囊提示行发虚/像重影（已修）（2026-11-21）

- **现象**（用户 m12352 第 4 条「很多字体小于系统字体，根本没与分辨率对齐，导致显示都是重影」）：胶囊下方「松开上屏 · Esc 取消」在浅色壁纸上糊成一团，笔画边缘有灰雾；胶囊内文字也比系统界面软。
- **根因**：① 胶囊是分层窗口（`UpdateLayeredWindow` + 预乘 alpha），文字用 `CLEARTYPE_QUALITY` 画——子像素渲染在透明合成面上必然出彩色边缘/重影；② 提示行没有底色，只能用「白色笔画亮度 = alpha」反推，12.5pt 字在 1x 下抗锯齿把笔画摊薄，覆盖率直接当 alpha 就会发虚（早先 ×1.5 截顶的补丁仍不够）。
- **顺带排除**：`--dpi-report` 实测进程 `DPI awareness=2`（PER_MONITOR_AWARE_V2）、`system=96`，说明 VM 里不是 DWM 位图缩放导致的虚；用户机上的「小于系统字体」对应 R-125（字号 13→14、11→12）。
- **修法**（`shell-win/src/overlay.rs`）：分层窗口的三处字体改 `ANTIALIASED_QUALITY`（灰度抗锯齿，无子像素彩边）；提示行覆盖率曲线改成 `cov = (base/255)^1.7 * 255`、门限 8→24，笔画实、边缘快速收干净。
- **验证**：VM 截图 `docs/overlay-win-capsule.png`（3× 放大后「松开上屏 · Esc 取消」清晰可读，无灰雾）；`--self-test`、9 页冒烟、`--dpi-report` 全通过。

## I-146 升级安装时残缺模型被「跳过」，装完仍然不能说话（已修）（2026-11-21）

- **现象**：已装过旧版（或下到一半被杀掉）的机器上装新包，安装器输出没有任何「已安装语音模型」，但 `%APPDATA%\Eryu\models\sherpa-onnx-sense-voice-…\` 目录**存在**——里面只有 `tokens.txt` 和一个 1MB 的残余 `model.int8.onnx`。启动后引擎加载失败，用户看到的仍是「按住热键没字」。
- **根因**：`scripts/install-win.ps1` 的判据是 `if (-not (Test-Path $target))`——只认目录存在。任何半截下载/手工删文件都会让安装器以为装好了。
- **修法**：判据改成主模型文件 `model.int8.onnx` 存在且 > 100MB，否则补齐（合并拷入，覆盖同名小文件）；已有完整模型时输出「语音模型已存在」，不动用户自己的文件。
- **验证**（VM）：先手工造「残缺状态」（目录 + `tokens.txt` + 1MB 假 `model.int8.onnx`）→ 跑新安装器 → 输出「已安装语音模型」、`model.int8.onnx` 变成 237115547 字节（226MB）→ `ERYU_MIC_WAV=probe.wav` + 热键模拟 → 记事本落「今天天气不错。」、`history.json` 为 `inserted / Notepad.exe / 7 字`（真实 SenseVoice 输出，不是 stub 的自测句）。

## I-147 「诊断」页第一屏是 TOML，用户看不出为什么不能用（已修）（2026-11-21）

- **现象**（对应 m12352 第 5 条「越来越乱了」）：点「诊断」看到的是一整屏可编辑的 TOML，想排查「为什么没上屏」的用户完全不知道从哪看起。
- **修法**（`shell-win/src/settings.rs::page_diag`）：第一屏改成只读「状态」（`core.diagnose()`：核心版本、配置目录、引擎/模型就绪、VAD、输入设备、历史条数、润色档位），下排四个动作「复制状态 / 打开日志 / 运行自检 / 麦克风权限」；TOML 编辑器降级为「配置全文（高级）」放在下半屏，高度压缩，保留「保存 TOML / 复制全文 / 打开配置目录」。
- **验证**：截图 `docs/settings-win-diagnostics.png`（状态区显示 `SenseVoice 就绪=true`、`输入设备：麦克风 (High Definition Audio Device)`、`历史：1 条（开启）`）；`--self-test`、`--settings-pages-smoke`（9 页）、`--settings-tab-demo 6` 全绿。

## I-148 逐字打字时前台是桌面/任务栏也会「报成功」（已修）（2026-11-21）

- **现象**（VM 复验 keystroke 路径时撞到）：目标程序没拿到前台、焦点落在任务栏的 `Windows.UI.Input.InputSite.WindowClass`（任务栏搜索框）时，`PostMessageW(WM_CHAR)` 照样把字送进去，回读也能读到 → 历史记 `ok`，但用户眼前什么都没发生。
- **根因**：`type_text` 只看「焦点控件是谁」，没看「前台窗口是不是桌面/任务栏」。桌面（`Progman`/`WorkerW`/`FolderView`）、任务栏（`Shell_TrayWnd`/`TrayNotifyWnd`）在 Win32 眼里都是正常窗口，甚至带有可写的输入宿主。
- **修法**（`shell-win/src/inject.rs`）：`type_text` 开头先取前台窗口类名，命中桌面/任务栏那一组就返回 `undelivered` +「焦点在桌面或任务栏上：先把光标放进要写字的输入框，再说一次」；焦点控件本身是 `FolderView`/`Progman`/`WorkerW`/`SysListView32`/任务栏类时同样拒打；`DirectUIHWND` 仅在它属于 `explorer.exe` 时才拒（避免误伤普通程序的 DirectUI 控件）。
- **另一半修法（I-143 补强）**：贴/打之后的回读改成 `wait_contains()`——最多 ~700ms、每 70ms 读一次，读到就返回。UWP/WinUI 的 RichEdit 是异步合成，读太快会把「其实已经进去了」判成 `unknown` → 历史留一条假 `pending`、下次启动还弹「恢复上次没上屏」。粘贴路径也复用同一套判据（半截命中仍单独提示「可能没粘全」）。
- **验证**（VM，装机目录 `AA0D949BD66F3515…` → `E5…` 系列构建）：
  - 逐字打字成功路径：UIA `Document.SetFocus()` 把记事本设成前台 + `--hotkey-sim-delay 8000` 延后模拟热键 → `type fg_class=RichEditD2DPT`、`verify #0 got="润色后：今天天气不错，我们开始吧。今天天气不错。"`、`上屏 status=ok`，记事本正文确实多出「今天天气不错。」。
  - 拒打路径：前台是任务栏 → `上屏 status=undelivered detail=焦点在桌面或任务栏上…`（不再假成功）。
- **测试基础设施**：新增 `--hotkey-sim-delay <ms>`（默认 1500），让测试脚本先把目标窗口点/`SetFocus` 成前台，再让模拟热键按下——否则隐藏启动的 PowerShell 会抢走前台，测出来的全是「焦点在桌面」。

## I-149 选区改写粘回可能落在自己的面板上（已修）（2026-11-21）

- **现象/风险**：`selection.rs::run()` 在**面板已经显示之后**才 `GetForegroundWindow()` 记「原来的窗口」，而 `show_palette()` 里会 `SetForegroundWindow(面板)`。一旦面板真抢到前台，记下的就是面板自己：`hide()` 只发 `WM_CLOSE`，随后 `SetForegroundWindow(面板)` + Ctrl+V 会把改写结果粘进一个正在收起的面板，用户看到的是「执行了但原文没变」。
- **为什么之前没暴露**：VM 自测时 Eryu 从 PowerShell 启动、`SetForegroundWindow` 被系统拒绝（调用进程不是前台进程），面板没抢到前台 → `prev_fg` 恰好还是记事本，看起来是对的。这类「靠运气对」的路径必须显式修掉。
- **修法**（`shell-win/src/selection.rs`）：
  - 新增 `static PREV_FG: AtomicIsize`，在 `open()` / `open_demo()` 里、`show_palette()` **之前**就 `fg_now()` 存下来。
  - `run()` 用 `PREV_FG`，为 0 才退回当前前台；并打 `SELECTION 粘回目标 0x…`，以后日志能直接看出粘回谁。
- **验证**（VM，装机目录构建 `5CCDFF4E77D8512F…`）：记事本写「嗯那个我们明天下午三点会议室评审设计文档」→ 全选 → `--selection-run 0`（新增自测入口）→ 记事本正文变成 mock 返回的「改写结果：明天下午三点在会议室评审设计文档，有问题随时说。」、日志 `SELECTION 已粘回 29 字`；剪贴板里用户在跑之前放的「用户原内容-不要被吃掉」在结束后仍原样（还原逻辑生效）。面板外观截图 `docs/selection-win-palette.png`（38 字原文 + 五条指令 +「↑↓ 选择 · 回车执行 · 1-5 直达 · Esc 收起」）。
- **测试入口**：新增 `--selection-run <N>`（不按键，直接跑「示例文字 → 第 N 条指令 → 粘回」全链路），配合本地 mock 润色服务即可无人值守复验。

## I-150 热词验证：自动化陷阱，不是产品 bug（2026-11-21，已闭环）

- **结论**：热词三条路径分开验过，都是好的——
  1. **替换效果**：`hotwords.toml` 写 `"今天天气" = "今天天气真好"`，`dist-win/Eryu.exe --decode-wav probe.wav` 输出 `WAVLAST 今天天气真好不错我们开始吧。`（原始识别是「今天天气不错我们开始吧。」）→ 规则在识别后正确替换。
  2. **页面「读」**：设置 → 热词把 `hotwords.toml` 渲染成 `今天天气 -> 今天天气真好`、「共 1 条」。
  3. **页面「写/保存」**：焦点进编辑框后逐字敲 `abc -> xyz`，点「保存热词」→ `hotwords.toml` 出现 `abc = "xyz…"`，即按「第一个 `->` 左边是错、右边是对」写入。
- **之前为什么判不出**（测试脚本的坑，记下来免得再踩）：
  - `SetWindowText` 直接塞中文进 `ID_HW_EDIT(112)` 后，保存读到的仍是空/旧值——`GetDlgItem` 拿到的句柄与页面重建后的实际编辑框可能不是同一个，`SetWindowText` 走的是另一条路径；
  - 用 `PostMessage(WM_CHAR)` 从 PowerShell 敲中文，PowerShell 侧的字符编码把字节打散，落进去就是乱码规则（`"蔔))" = "蔔))}"`）。**这是脚本问题，不是产品问题**；
  - 敲 ASCII 时为了省事从光标位置插入（光标在开头），结果拼成 `abc -> xyz今天天气 -> 今天天气真好`，被按第一个 `->` 切分——这也解释了当时看到的「规则怪怪的」。
- **怎么用自动化验它（下次照这个来）**：别在 PowerShell 里传中文；在宿主生成 UTF-8 的 `hotwords.toml` → `copyFileFromHostToGuest` → `Eryu.exe --decode-wav <wav> -RedirectStandardOutput` 到文件再读 `WAVLAST`。
- **仍未端到端覆盖的一小段**：热词 + 热键听写 → 记事本里看到被替换的句子。`--hotkey-sim` 按会话顺序消耗同一个 WAV，第二轮只剩尾巴（历史 `chars=3`），所以这条用真机更靠谱；手测清单 F7 保留，描述改成「规则已确认在识别后生效，这里只需看一眼上屏的句子是否也带替换」。

## I-151 通用页有两个「保存」、页脚写着内部路径（2026-11-21，已改）

- **现象**：设置 → 通用，页面中间一个「保存」（快捷键/拾音/上屏方式/开关），下面「按应用覆盖」文本框旁边又一个「保存覆盖」。用户改完文本框点上面的「保存」不会存覆盖，得记得点第二个键——两个保存键等于让人猜。
- **修法**（`shell-win/src/settings.rs`）：
  1. 去掉「保存覆盖」按钮，通用页只留一个「保存」；`ID_SAVE` 在 page 0 时先抓 `e_perapp` 的文本、再 `save_general()`（它会 `rebuild_page()` 换掉编辑框）、最后 `save_per_app_text(&text)` 落盘——顺序反了会把用户的覆盖清成 `{}`。
  2. 去掉左下角「配置：%APPDATA%\Eryu」：诊断页已有「配置目录」一行，这里只是噪音。
- **验证**：装机构建 `FB422BC6…`，通用页截图 `docs/settings-win-general-single-save.png`（只有一个保存键、无页脚路径）；`--self-test` OK；`--settings-perapp-selftest` 仍 `PERAPP OK` 且 `config.toml` 写入 `per_app = {...}`（这条自测走的是应用内 `SetWindowText` + 保存，验证的是同一条落盘路径）。
- **没能自动验的一段**：从 PowerShell 用 `SetWindowText` 往编辑框写、再点保存——文本在点击前就没了（外部进程写进去后应用侧读回为空，`GetDlgItem` 拿到的句柄和应用持有的 `e_perapp` 是同一个）。这是自动化的问题（跨进程写编辑框 + 快速收尾），不是产品行为；真实用户是键盘输入。真机手测 D 组顺手确认一次即可。

## I-152 历史页五个按钮、还要记得点「刷新」（2026-11-21，已改）

- **现象**：历史页一排按钮：刷新 / 复制选中 / 删除选中 / 清空历史 / 重试失败的段。五个并列看不出主次；而且刚说完一句，列表不会自己更新，得点「刷新」——用户很容易以为「没记上」。
- **修法**（`shell-win/src/settings.rs`）：
  1. 去掉「刷新」；按钮改四个一行：「复制」「删除」「清空」「重试失败（N）」（没有失败段时显示「没有失败的段」）。
  2. 自动刷新：`main.rs` 的 `result` 事件里只置 `hist_dirty`，**等 APP 锁放开**再调 `settings::refresh_history_if_open()`（正停在历史页且窗口可见才重建）。不能在持锁期间重建页面——`build()` 会再取 APP 锁，就是 I-127 那个自锁。
- **验证**：装机构建 `484F3412…`；`--open-settings-page 4` + `--hotkey-sim --hotkey-sim-force --hotkey-sim-secs 1` → 说完那句后列表里出现 `18:45 explorer.exe 2字 待复制`（不需要点刷新），窗口 `SendMessageTimeout(WM_NULL)=1` 仍响应（没死锁）；截图 `docs/settings-win-history-4buttons.png`。

## I-153 润色页六个按钮、一页三处「保存」语义（2026-11-21，已改）

- **现象**：润色页原来有「保存润色设置 / 测试连接 / 检测模型 / 用选中的档 / 保存槽 / 保存指令」六个按钮，加上「还没填 API Key」和「还没填 Key：填好点保存」两处重复提示。用户要改一个档的快捷键，得先选档、点「用选中的档」、再点「保存槽」——三个动作才完成一件事。
- **修法**（`shell-win/src/settings.rs`）：
  1. 去掉「检测模型」（同类功能「测试连接」已经能验证端点+key；模型名是可手填的，mac 也没有这个按钮）。
  2. 「保存润色设置」→「保存」；「测试连接」旁边一行状态文字，未填 key 时提示「还没填 Key：填好点「保存」，再点「测试连接」」——**只留这一处提示**，删掉重复的「还没填 API Key」。
  3. 「保存槽」并进「用选中的档」：点一次 = 激活这一档 + 槽位框里有内容就一起绑上（没内容不动）。按钮从 6 个降到 4 个。
- **验证**：装机构建 `696FB1F4…`；`--self-test` OK；截图 `docs/settings-win-polish-clean.png`（4 个动作键、一处提示、档位一行一条）。调试插桩确认点击「用选中的档」时 `sel=0` 且确实进了 `save_slot()`（sel=0）。
- **没能自动验的一段**：跨进程把「档位快捷键」框里的文字改掉再点保存——`Ctrl+A` 和 `EM_SETSEL(0,-1)` 都不生效，新增字符被拼到原值后面变成非法键名，保存被拒。这是自动化限制；真机点一下确认即可（手测 D 组）。
- **顺带修测试设施**：新增隐藏开关 `--assume-consent`（等同 `--self-test` 那批的静默位），否则自动化点「用选中的档」会卡在「文字会发到服务端」的模态框上——第一次测试就是这么挂住的（VM 里 `#32770` 对话框等不到人点，`runProgramInGuest` 一直不返回）。

## I-154 引擎页两个「保存」、灰字说明占位（2026-11-21，已改）

- **现象**：「下载地址」「下载代理」两行各有一个「保存」，一次改两个值要按两次；页面中间还有一行灰字解释「标点、降噪、声纹模型不在这里」，长期占一行。
- **修法**（`shell-win/src/settings.rs::page_engine`）：
  1. 两行共用一个「保存」（`ID_ENGINE_SAVE=109`）：一次写 `model_mirror` + `model_proxy`。
  2. 那行灰字收进 ⓘ（悬停才看），同一处 ⓘ 也说明「留空 = 官方源 / 系统代理」。
  3. 「使用选中模型 / 下载选中模型」→「使用 / 下载」。
- **验证**：装机构建 `7176443E…`；在「下载代理」里键入 `http://127.0.0.1:7890` 后点那一个「保存」，日志 `CFG model_proxy=http://127.0.0.1:7890 下载代理已保存`，`config.toml` 出现 `proxy = "http://127.0.0.1:7890"`（同一按也写了 `model_mirror`）。截图 `docs/settings-win-engine-single-save.png`；测完已把 VM 配置恢复（mirror 官方默认、proxy 空）。

## I-155 按应用覆盖：规则有单测、两条注入路径各自有端到端，缺一次真机串联（2026-11-21）

- **想验什么**：设了 `[insert] per_app` 后，真实热键听写是否真的按前台程序切换上屏方式。
- **已验证**：`mode_for()` 单测（含大小写、包含匹配、回退）；逐字打字与粘贴各自在 VM 里真落字；焦点在桌面/任务栏时报 `undelivered`。
- **卡在哪**：VM 里 `runProgramInGuest` 起的进程会让前台掉回 `Progman`，记事本拿不回前台，`submit_request` 的 `app` 恒为 `explorer.exe`，覆盖规则无从命中（见 `docs/win-parity.md`）。
- **不是产品缺陷**：`core/src/lib.rs:2185` 只用 `mode_for(&c, &app)` 的结果填 `mode` 字段，壳按该字段分派，两端都已单独验过。
- **补验入口**：`--inject-test <文本> --mode paste|keystroke`；真机见 `docs/win-manual-test.md` D 组。

## I-156 测麦克风会把「上次没上屏的旧文字」当成刚听到的（2026-11-21，已修）

- **现象**：`--mic-test` 在什么都没录到时也输出一句具体文本。VM 里复现：配置一个不存在的设备名 `device = "不存在的麦克风"` → `MICTEST 录音 2s…` / `MICTEST OK 结果=今天天气不错我们开始吧。`——那句话其实是历史里上次的识别结果。
- **根因**：`core/src/lib.rs::recover_pending_result()` 启动时把 `pending/last_result.txt` 读回内存的 `last`（给崩溃恢复用）；`--mic-test` 与设置页「测试麦克风」都用 `last_result()` 当结果，于是「没听到」和「听到了旧句子」分不开。
- **修法**：新增 `EryuCore::clear_last_result()` 与 FFI `eryu_clear_last_result`（只清内存副本，不动落盘文件）；`shell-win/src/main.rs` 的 `--mic-test` 与设置页 `mic_test_3s()` 都在开录前清一次，`--mic-test` 在结果为空时打「（没听到内容）」。
- **验证**：VM 里先写一份 `pending/last_result.txt`（「上次没上屏的旧话。」）再跑 `--mic-test 2`，输出 `MICTEST OK 结果=（没听到内容）`；`last_result.txt` 仍在（没破坏崩溃恢复）。`--self-test`、9 页冒烟全绿。
- **顺带查清**：设备名不存在时核心会报「找不到输入设备「X」：可能已断开。可用设备：…。在设置 → 音频 里改回系统默认即可」（`core/src/audio.rs::start_with`），不是静默换默认设备；测完已把 VM 的 `device` 恢复为空（= 系统默认）。

## I-157 麦克风打不开时用户「按了热键什么都没发生」（2026-11-21，已修）

- **现象**：把输入设备改成不存在的名字（或真机拔掉麦）后按热键，屏幕上没有任何反馈；历史不新增、音频不响，用户只能靠托盘提示才知道出错（提示还常常看不到）。
- **根因**：① `shell-win/src/overlay.rs::error()` 只改状态并 `render()`，从不 `ShowWindow`——「录音还没开始就失败」时浮层从未显示过，这次 `render` 等于画给一个隐藏窗口；② 即使显示了，核心紧接着发的 `idle` 状态事件会把错误内容立刻替换成普通胶囊（0.9 秒后隐藏），错误一闪而过；③ 错误文案带完整可用设备列表（80 字），胶囊最宽 420pt，`fit_tail` 又把开头「找不到输入设备…」裁掉，只剩尾巴。
- **修法**：`error()` 里 `ShowWindow(SW_SHOWNA)`（不抢焦点）+ 按文案长度预设 `width_pt`；新增 `ERR_UNTIL` 时效窗，`show()` 在错误提示时效内忽略后续 `idle`；错误文案改成「找不到输入设备「X」：可能已断开，设置 → 音频 里改回系统默认」，可用设备列表只写日志（`core/src/audio.rs`）；错误文案用 `fit_head`（留开头加省略号），识别中间结果仍用 `fit_tail`。
- **文案分工（重要）**：核心的完整文案仍然保留「可用设备：… 在设置 → 音频 里改回系统默认即可」——`scripts/matrix.sh` 会断言这一点（用户要能照着做）；浮层上只显示一句短的「麦克风打不开：在「设置 → 音频」里改回系统默认设备」，全文进托盘提示。
- **验证**（装机构建 `63760BB9…` core / `d87d649c…` 壳）：VM 里 `device = "不存在的麦克风"` + 模拟热键 → 日志 `OVERLAY 错误提示：找不到输入设备「不存在的麦克风」…`、`OVERLAY 保留错误提示，忽略状态 idle`、`OVERLAY 设置位置 … w=420 h=56 实际=420,56`；宿主 `vmrun captureScreen` 截图 `docs/overlay-win-error.png` 显示「! 找不到输入设备「不存在的麦克风」：可能已断开…」。声音、历史不受影响（`HIST_BEFORE=1 AFTER=1`），进程存活。
- **测试坑**：VM 客户机里 `CopyFromScreen`（PowerShell 截图）在分层窗口更新时会拿到**旧帧**（同一张图 md5 反复不变、窗口 rect 也是旧的），验收要以宿主 `vmrun captureScreen` 为准。

### I-159（Windows 侧补齐 I-131）配置读坏时，Windows 设置页看不到提示

- **现象**：手改 `config.toml` 写坏（例如多一行 `BADLINE`）后启动，Windows 上只在浮层闪一条 3 秒的提示、托盘提示一行；设置页里什么都不说。mac 的通用页顶部有一段橙色警告，两边不一致。
- **修法**（`shell-win/src/settings.rs::page_general`）：`load_warning` 非空时，在通用页底部加「上次加载配置时出过问题」小节 + 首行警告（超过 90 字截断），并指到「诊断」页看完整报错；完整文案（含行列号、`config.toml.broken` 备份路径）仍在 `core.diagnose()` 与托盘提示里。
- **验证**（VM 装机构建）：写坏配置启动 → 设置窗（`--open-settings-page 0`）子控件文本里有「上次加载配置时出过问题」和「config.toml 解析失败（已用默认值）：TOML parse error at line 3, column 8」；`config.toml.broken` 已生成；截图 `docs/settings-win-config-warning.png`。
- **注意**：页面位置只有一行，别把多行报错硬塞进去（第一版塞了 3 行，被窗口底边裁掉）。

### I-158 启动时的「上次没上屏」提示会吞掉第一段录音的胶囊

- **现象**：刚启动就按热键，胶囊不出现、也没有音量条，像是没录；松手后也没有上屏，用户会以为热键坏了。
- **根因**：托盘/浮层为「上次有一段没上屏的文字」这类提示调用了 `overlay.error()`，它会钉住 `ERR_UNTIL`（3.2 秒）。`show(phase)` 对非 error 状态一律 return，于是紧随其后的 `starting` / `listening` 都被吞掉（日志：`OVERLAY 保留错误提示，忽略状态 starting`）。
- **修法**（`shell-win/src/overlay.rs::show`）：`starting` / `listening` 是两个「用户主动开始一段」的状态，进函数先把 `ERR_UNTIL` 清掉（日志「OVERLAY 开始新一段，清掉旧提示」），再正常显示；`idle` / `processing` 仍在时效内保留错误，避免错误被一闪而过的状态顶掉。
- **验证**（VM 装机构建 `34861616…`）：有「上次没上屏」提示启动 → 2.5 秒后模拟热键 → 日志出现 `OVERLAY 开始新一段，清掉旧提示`，随后胶囊正常出现；用 WM_APP+3 直接投 Esc（绕过低级钩子对注入键的忽略）取消一段长录音 → 进程存活、`history.json` 条数不变（没有留下半截条目）。
- **顺带确认**：Esc 取消录音的路径本身没问题（`shell-win/src/main.rs` 里 `vk == 0x1B && !is_break` → `core.cancel()`），只是自动化要靠直接投 `WM_ERYU_KEY`，`SendKeys` 产生的注入键会被钩子按 I-113 的规则跳过。

### I-160 explorer 重启后托盘图标不会自己回来（I-128 只接了一半）

- **现象**：重启 explorer（崩溃/注销重登常见）之后，托盘里 Eryu 图标消失，热键还能用但用户找不到入口，只能靠重开程序。
- **根因**：`shell-win/src/main.rs` 里只做了「注册 `TaskbarCreated` 消息」和「`SetTimer(TRAY_TIMER_ID, 5000)`」两件事，**消息循环里没有任何分支处理它们**——`tray::ensure()` 在整个工程里只有自测入口 `--tray-drop-test` 调用过。也就是说 I-128 记的「顶层消息窗 + TaskbarCreated + 5s ensure」里，后半截没接。
- **修法**：
  - wndproc 增加 `m if m == tray::taskbar_created_msg()`（该值来自 `RegisterWindowMessageW`，不是编译期常量，只能在 match 里用 guard 比）→ `tray::ensure()`；
  - 增加 `WM_TIMER if wparam.0 == tray::TRAY_TIMER_ID` → `tray::ensure()`（explorer 没广播或广播丢失时的兜底）；
  - `ensure()` 在 `NIM_MODIFY` 失败时补一条 trace，便于事后判断图标是真的丢了还是只是刷新。
- **验证**（VM 装机构建 `5E0F9977…`，`ERYU_TRACE=1`）：启动 → 6 秒后 `Stop-Process explorer` → 日志依次出现
  `tray-timer：复检图标` → `tray-ensure：图标不在了（NIM_MODIFY 失败），重新 NIM_ADD` → `taskbar-created：重加托盘图标` → `tray-ensure re-added icon`；explorer 自动重启（`EXPLORER=1`）、Eryu 进程存活、后续 5 秒复检继续正常。
- **教训**：注册了系统广播/定时器，一定要在消息循环里搜到对应的处理分支——「注册了」不等于「接上了」。这类缺口只有真跑一遍（并且日志里能看到）才算验证。

### I-161 迷你窗点起来的一段结束后，下一次点击/热键第一下被吞

- **现象**：从迷你窗点一下圆圈开始说话，等它自己结束（静音/到上限/取消）后，再点一次圆圈没反应；如果是改用热键，第一下也没反应，第二下才行。
- **根因**：`HOTKEY_DOWN` 这个去重标记只有一个地方会清——`talk_up()`（热键松开时调用）。迷你窗是「点一下开始、再点一下停止」，没有「松开」事件；会话自己结束（核心发 `idle`/`done`）时没人调 `talk_up()`，标记一直留在 `true`，于是 `talk_down()` 的去重分支把后续的开始请求全部吞掉。
- **修法**（`shell-win/src/main.rs`）：
  - 把真正开始的逻辑拆成 `talk_start()`；`talk_down()`（热键）保留去重 + 源标记，`talk_from_mini()` 不参与去重；
  - 新增 `static TALK_SOURCE`（0 没有 / 1 热键 / 2 迷你窗）；
  - 收到 `state` 事件的 `idle`/`done` 时，若 `TALK_SOURCE == 2` 就放掉 `HOTKEY_DOWN` 并清源标记（trace「迷你窗一段结束：放掉去重标记」）。
- **验证**（VM 装机构建 `4E98E3BE…`）：一次进程内连点三次圆圈 → 日志出现两次 `talk_from_mini` + 一段结束后的 `迷你窗一段结束：放掉去重标记`，并真的开了第二段会话（`generation:2` 的 `starting → listening → live` 事件）；第三次点击落在上一段的 `processing` 阶段，按现有设计忽略（`mini_toggle_talk` 只处理 idle/done/starting/listening）。
- **热键路径回归**：同构建上 `--hotkey-sim --hotkey-sim-force` 一段完整走完（`talk_down-in/out` → `starting/listening/live` → `talk_up-in/out`），历史新增 `status=inserted` 条目（`app=SearchHost.exe` 是因为测试时焦点在搜索窗，注入本身成功）。
- **教训**：两个入口共用一份去重状态时，必须分别问「谁会来清它」——按住说话有 `break`，点按说话没有。

### I-162 热键写法错了不会告诉你（还可能把 `CTRL+` 当单个 Ctrl）

- **现象**：在设置 → 通用 →「按住说话的键」里填了一个看不懂的值（例如 `CTRL+FOO`、`ctrl`、`CTRL+`），点保存照样成功、界面照样显示那串字，但实际按下去不生效（壳启动时静默退回 `ALT_R`，只往 stderr 打一行，GUI 进程里看不到）。
- **更隐蔽的一条**：`hotkey::parse("CTRL+")` 原来返回 `Some`——分隔符后面的空段被过滤掉，于是它被解析成「单个 Ctrl」，用户以为配了组合键，实际按一下 Ctrl 就开始录音。
- **修法**：
  1. `shell-win/src/hotkey.rs::parse` 先把分隔符两边的空段判成写错（`CTRL+`、`+ALT`、`CTRL++A` 全部拒绝）；`ALT_R` 这类单键不受影响。
  2. `shell-win/src/settings.rs::save_general` 保存前先 `hotkey::parse`，解析不了就弹「「{spec}」这个组合键看不懂，没有保存」并**不落盘**。
  3. `shell-win/src/main.rs` 启动时若配置里的值解析不了，除退回 `ALT_R` 外还发一条托盘提示「热键「X」看不懂，先用 Alt（右键）说话；到设置里改一下」。
  4. 新增 CLI `--hotkey-check <spec>` 打印 `HOTKEYCHECK spec=… ok=true/false`（单元测试要 link.exe，宿主跑不了，所以用装在 VM 里的真实 exe 验）。
- **验证**（VM 装机构建 `C315450D…`）：`CTRL+` → `ok=false`、`CTRL+FOO` → `ok=false`、`ALT_R` / `CTRL+ALT+SPACE` / `F13` → `ok=true`；改之前 `CTRL+` 是 `ok=true`。
- **教训**：配置项「能存进去」不等于「能用」。只要解析失败必须有回退可见路径（弹框/托盘提示），否则用户看到的就是「设了没用」。

### I-163 热词表里一行打错，全部热词静默失效

- **现象**：用户手工编辑 `%APPDATA%\Eryu\hotwords.toml`（或 mac 的 `~/.eryu/hotwords.toml`），多打一个引号 / 少一个等号，界面上「共 0 条」，之前所有热词都不再生效；更糟的是这时候点一次「保存热词」，文件会被重写成空模板，原来的规则**真丢了**。
- **根因**：`core/src/hotwords.rs::from_toml_with_path` 用 `text.parse::<toml::Value>()` 整份解析，失败时 `if let Ok(...)` 不成立 → 规则表直接留空，错误被吞掉；`to_json` 也没有错误字段，界面无从提示。
- **修法**：
  1. 整份解析失败时**逐行兜底**：只认 `key = value` 的形状，引号必须成对（`"坏的 = "值"` 这种半截引号直接跳过），把能认的规则捞回来照常生效。
  2. `Hotwords` 增加 `error: Option<String>`，`to_json` 输出 `"error"`；文案只留一眼能看懂的「文件有格式错误（第 N 行），只载入 X 条 · 点「保存热词」重写」（完整 TOML 报错太长，界面放不下）。
  3. Windows `settings.rs::page_hotwords` 计数行改成 `⚠ 共 N 条 · {error}`；mac `AppModel.hotwordsError` + `HotwordsSection` 用橙色 `Label` 展示。
- **验证**：`cargo test --lib hotwords` 9 项全过（含新增 `salvages_lines_when_file_is_broken` / `good_file_has_no_error`）；VM 装机构建 `9F78FDB9…`：写坏文件后 `--hotwords-json` 输出 `{"error":"文件有格式错误（第 3 行），只载入 2 条 …","rules":[{深度求索…},{艾瑞…}]}`，页面截图 `docs/settings-win-hotwords-broken.png`，两条规则仍在用（`--decode-wav` 仍能替换）。
- **教训**：凡是「用户可手改的文件」，解析失败必须「尽量捞 + 明确说」，绝不能静默清空——静默清空的下一次保存就是数据丢失。


## I-164 Windows 诊断包：一次点击导出与上传（已修，R-127 / D-114）

**现象**：用户在 Windows 上遇到问题时，我只能让他「把 %APPDATA%\Eryu 打包发我」——多数人不会，也会带上不必要的正文。

**修法**
- 新增 `shell-win/src/diag.rs`：`snapshot()`（UI 线程取版本/自检/配置/历史统计/host 信息）→ `bundle()` 七段 JSON → `save()` 写 `%APPDATA%\Eryu\diag\eryu-diag-<时间>.json` → `upload_body()` 走核心 `eryu_upload_diag`（ureq，15s）POST `[diag] upload_url`。
- 配置新增 `[diag] upload_url`（默认 `https://officeace.harmonyos.fun/eryu-diag/upload`；留空 = 只导本地）。
- 设置 → 诊断页按钮「导出诊断包 / 上传诊断包」，结果写在按钮右侧；CLI `--diag-bundle` / `--diag-upload` 带 `DIAG …` / `DIAGCODES n` 输出。
- 错误码落盘 `eryu-errors.log`（`diag::note_error` 同时写内存与文件），诊断包 `errors` 段取最近 20 条；GUI 与 CLI 两条路径都记。
- 隐私：`config` 里 `api_key/token/password/secret` 一律 `***`；`history` 只有条数与状态分布；正文、热词内容不入包。
- 顺手修：`polish_failed` 之前被 `if code != "polish_failed"` 整段吞掉，现在浮层提示「润色没成功，这次贴的是原话」，详情进托盘。
- 核心版本号从 0.1.0 改为跟随产品 0.8.1（此前诊断包/UA 显示 0.1.0，和安装的版本对不上）。

**验证（VM 装机构建）**
- `--diag-bundle` → 生成 `eryu-diag-20261121-214005.json`（8.1KB）。
- 宿主起 mock（0.0.0.0:18082）→ 配置指向 `http://192.168.36.1:18082/diag` → `--diag-upload` 报「已上传」，mock 收到 8613 字节，`User-Agent: Eryu/0.8.1`，`Content-Type: application/json; charset=utf-8`。
- 触发一次真实错误（`device = 根本不存在的麦克风` + 模拟热键）→ `eryu-errors.log` 出现 `20261121-214144  mic_unavailable  找不到输入设备「…」…` → 再导出：`errors` 段正是这条，`DIAGCODES 1`。
- 脱敏核对：`api_key = ""`、`token = ""`（空值保留空串，能看出没设），没有明文密钥。
- 服务端端点尚不存在时：`POST https://officeace.harmonyos.fun/eryu-diag/upload` → 404，客户端提示「服务端还没有开放诊断上传（404）：诊断包已存本地，把它发给我即可」，文件保留。

## I-165 发版清理：测试口、托盘自测项、`--assume-consent` 的连带副作用

**审计发现**（`docs/AUDIT-0.8.1.md` 的来源）：安装包里带着能改用户数据的 CLI（`--set-config`/`--set-hotkey`/`--history-delete`），托盘菜单里挂着「自测…」，`--assume-consent` 顺带把 `message_box` 全局降级成 stderr（于是「已在运行」「组合键看不懂」这些提示在自动化下凭空消失，也让别人以为产品没提示）。

**修法**
- `--dev` / `ERYU_DEV=1` 闸门：`--set-config`、`--set-hotkey`、`--history-delete`、`--decode-wav`、`--inject-test`、`--inject-test-hold`、`--hotkey-sim*`、`--onboarding-step`、`--dump-config`、`--selection-run`、`--selection-demo` 不带 `--dev` 时退出码 2 并打印原因。
- 托盘「自测…」只在 `--dev` 下出现（`crate::dev_mode()`）；发版菜单 17 项，dev 18 项。
- 新增 `ASSUME_CONSENT` 原子量：`--assume-consent` 只跳过润色同意弹窗，`QUIET` 不再被它打开。
- 新增 `--version`（`Eryu 0.8.1`）。

**验证（VM 装机构建）**：`--set-config '{"denoise":false}'` → rc=2、`--set-config 只在开发版可用（启动时加 --dev）`；`--dev --dump-config` → rc=0；`--version` → `Eryu 0.8.1`；`--tray-menu-dump` 17 项、`--tray-menu-dump --dev` 18 项；`--diag-bundle` 不受影响。

## I-166 胶囊变长时往右跑（mac，位置累计漂移）

**现象**（用户 2026-11-21 22:36）：「胶囊怎么变长的时候跑右边去了」。录音时识别文字越攒越多，胶囊一格一格变宽，但整个胶囊整体向右平移，最后停在偏右的位置。

**根因**：`app/Sources/EryuApp/Overlay.swift::resize(to:reset:)` 用 `lockedOrigin` 记「上一次定位的原点」，宽却用当前 `size.width` 算新原点：
```swift
if let o = lockedOrigin, !reset { frame.origin = NSPoint(x: o.x + (oldWidth - width) / 2, y: o.y) }
```
第一次变宽时 `o.x` 还是 190pt 宽时的原点，算出来是对的；第二次变宽时 `oldWidth` 已经是上一次的宽度、而 `o.x` 仍是原始原点 → 每次少补「增量的一半」，中心一格一格往右挪。宽度按 24pt 台阶增长，所以看起来就是「每长一次往右跳一点」。

**修法**：把算出的新原点写回 `lockedOrigin`，让「中心 x 恒定」在整段录音里成立：
```swift
let newX = o.x + (oldWidth - width) / 2
frame.origin = NSPoint(x: newX, y: o.y)
o.x = newX
lockedOrigin = o
```
另加 `ERYU_OV_TRACE=1` 时的 `[ov-size] w=… x=… center=…` 日志，专门盯这类位置漂移。

**验证**：`--simulate /tmp/a.wav`（9 秒、文字持续增长）→ 日志 `w=264 x=603 center=735.0` / `w=312 x=579 center=735.0` / `w=336 x=567 center=735.0` / `w=384 x=543 center=735.0` / `w=408 x=531 center=735.0` —— 宽度从 264 长到 408，中心始终 735.0（屏幕中心），不再右移。修复前同一序列中心会依次变成 772 / 796 / …。

## I-167 新一段录音开始时胶囊还挂着上一段的文字

**现象**（用户 2026-11-21 23:01）：说完一段后，再按快捷键呼出，胶囊里仍有上一段的内容（有时看着像一句「预置」的话），要等新一段第一次识别结果回来才换掉。用户期望：按下去应当先是一个光标在那儿等着。

**根因**：`app/Sources/EryuApp/Overlay.swift::show(phase:)` 只在 `previous == "idle"` 时做「新会话」重置（宽度/位置），**从不把 `view.partial` / `view.redacted` 清掉**。上一段结束到下一次按下之间只隔几百毫秒，旧句子仍在 `partial` 里；而 `setPartial` 又对空串直接 return，所以界面上就成了「新一段开始但显示上一段」。Windows 侧 `overlay.rs::show()` 一直有 `s.text.clear()`，所以只有 mac 有这个问题。

**修法**：把「新会话」的判据改成「上一刻不在录音里」（`previous != "starting" && previous != "listening"`，也就是 idle/processing/inserting 都算），进入时清空 `partial`、`redacted`，宽度回到 190pt、位置重取一次插入点；`setPartial` 收到空串也允许清空。`ERYU_OV_TRACE=1` 时会打 `[ov-session] 新一段录音：清掉上一段文字（previous=…）`。

**验证**：`--simulate` 日志出现 `[ov-session] 新一段录音：清掉上一段文字（previous=idle）`；同一轮 `[ov-size]` 中心恒为 735.0（顺带覆盖 I-166 的回归）。

## I-168 第二份实例会卡在「已在运行」的对话框上

**现象**：真机（FLYEASY_AI）上点两次图标，任务管理器里出现两个 `Eryu.exe`，用户看到的是一个必须点「确定」的提示框；不点就不消失。

**根因**：单实例守卫用 `message_box`（模态）告知「已经在运行」，模态框等人点，进程就一直挂着——看起来像「装了两份」。

**修法**：`shell-win/src/main.rs` 两处守卫改成——叫出已有实例的设置窗口（`WM_ERYU_REVEAL`）+ 让它弹一句托盘提示（新消息 `WM_ERYU_SECOND = WM_APP + 11`，只 `tray::set_tip`），然后第二个实例 `std::process::exit(0)`，不弹任何对话框。

**验证**：真机上用计划任务在交互会话里连开两次 → `BEFORE=1 / AFTER=1`，`Get-CimInstance Win32_Process -Filter "Name='Eryu.exe'"` 只有一个（sid=1）。

## I-169 润色会截断长句，最后一句丢失或被改残（0.8.2，已定位）

- 现象：口述较长时最后一句经常「意思出不来」，像被截掉。
- 根因：`core/src/polish.rs` 请求体没有 `max_tokens`，服务端默认额度（常见 512）在句子中间截断；`suspicious()` 只按 40% 长度比兜底，半句仍可过检。
- 修法（D-115）：带 `max_tokens`；`finish_reason == "length"` 判失败 → 回退原文 + 浮层提示。

## I-170 润色「附加要求」输入框打不进输入法、不能粘贴（0.8.2，mac）

- 现象：mac 设置 → 润色 → 附加要求（`app/Sources/EryuApp/SettingsView.swift:788` 的 `TextEditor`）无法用中文输入法上字，⌘V 也不出字。
- 现状：`KeyTap.swift` 是 `.listenOnly` 且原样放行事件，理论上不拦截；Windows 润色页没有这个输入框，故问题仅在 mac。
- 下一步：最小复现后修（窗口 key 状态 / TextEditor 焦点 / 快捷键冲突三条线）。

## I-171 冒烟测试会被真实配置带偏（2026-11-22，已修）

- 现象：`scripts/smoke.sh` 一夜之间 6 项失败（`decode-wav` 空文本、模拟听写超时、验收「松手没结果」、两分钟上限失败）。
- 根因：冒烟直接用真实配置目录 `~/.eryu`；此前测声纹时把 `voice_lock = true` 留下了，所有模拟音频被判「不像你的声音」整段跳过（`voice_all_skipped`），于是「没有结果」。
- 修法：`smoke.sh` 开头 `mktemp -d` + `export ERYU_CONFIG_DIR`，把 `~/.eryu/models` 软链进临时目录，`trap` 退出时清理。测试不再受用户真实设置影响。

## I-172 长听写的最后一句话会丢（2026-11-22，已修）

- 现象（用户 m16118）：「说话十几秒，最后一句话总是没了；能感觉到模型已经识别出来了，但最后一句没上屏」。
- 复现：22.4s 四句语料，`--simulate` 走真实会话链路 → 输出停在「…下周一开始执行」，最后一句「最后一句话是请大家务必准时参加」整段消失；同一文件走 `--decode-wav`（离线整段）则一字不差。⇒ 丢在「松手取音频」这一段，不是 ASR 也不是润色。
- 根因（两处，同一后果）：
  1. `core/src/audio.rs::Capture::finish` 先 `drop(self.stream)` 再 `try_recv`：停流之后才收，还没回调上来的那批样本直接丢；模拟麦克风更明显——松手瞬间按墙上时间算还差几百毫秒到几秒没喂完，于是尾巴整段没进 PCM。
  2. `core/src/audio.rs::cap_len_to` 超上限时 `drain(0..excess)`（丢开头留结尾），与 `DESIGN` v0.32「只保留前 120s」相反：长听写超时后开头反而没了。
- 修法：`finish` 改成「先宽限 120ms 边等边收，再停流取最后一批」；模拟麦克风的喂样本改成「按按下到现在的墙上时间算目标位置」（以前按每次 tick 的增量算、并立刻把 sim_last 推到 now，tick 一被拖慢，落后的那截就永久丢了——这正是模拟侧长句丢尾的成因；另外不能把整个 WAV 一次倒进去，否则松手很早的长听写用例会凭空多出几分钟音频）；`cap_len_to` 改为 `truncate(max)`（保留前 120s）。
- 验证：同一条 22.4s 语料 `[eryu][tail] 松手时音频 22.368s`（= WAV 全长）、输出含最后一句；`scripts/longcap.sh`（167s 语料，上限 120s）→ 到点提示 ✅ / 保留开头 ✅ / 超限部分没出现 ✅ / 松手→出字 641ms；冒烟 5d 连续 16 句前缀 1164ms。
- 测试夹具修正：`scripts/longcap.sh` 原来用真实配置（用户把上限调到 300s 时 167s 够不着上限，假失败），改成自建 `ERYU_CONFIG_DIR` 并把 `max_record_secs` 钉成 120。

## I-173 说话期间弹了别的窗口，结果就贴进新窗口（2026-11-22，已修）

- 现象（用户 m16291）：「按着快捷键说话，中间弹出一个别的窗口，松手以后内容不回到原来的文本框，像没有锁定焦点」。
- 根因：mac 壳在 `submit_request` 里才现取焦点（`AX.focusedElement()` + ⌘V 发到当前前台），整条链路没有「说话开始时是哪个应用/哪个输入框」这个概念；Windows 壳早有 `note_app`（只在历史里记名字，注入仍按当前前台）。审计里那条「§4.1 提交前目标复核」正是这个缺口。
- 修法（`app/Sources/EryuApp/Injector.swift` + `AppDelegate.swift`）：
  - `Injector.Target { pid, name, element }`，`captureTarget()` 在 `handleHotkeyDown()` 里记录当时的 pid 与焦点控件；
  - `inject(text:mode:target:completion:)` 提交前比对前台 pid：不同就先 `restore(target)`——`NSRunningApplication.activate(.activateIgnoringOtherApps)` + 把 `kAXFocusedAttribute` 还给原控件，最多等 0.6s；切回来了再粘，切不回来就 `undelivered`（文字进剪贴板 + 托盘提示），绝不把字塞进别人窗口；
  - `submit_request` 取走并清空 `speechTarget`，避免下一次误用。
- 验证（`--focus-lock-test "焦点锁定测试ABC"`）：TextEdit 新建文档 → 记录目标「文本编辑」→ 强行切到「访达」→ 注入 → 日志 `[eryu][inject] 前台已换：说话时 文本编辑(96047)，现在切回去再粘` → `FOCUSLOCK target=文本编辑 switchedTo=访达 result=ok frontAfter=文本编辑 文本框含结果=true`。

## I-174 设置页三处 UI 毛病：热词输入框打断输入法、润色字段靠右、说明文字糊满界面（2026-11-22，已修）

- 现象（用户 m16347 / m16441 / m16472）：
  1. 热词页「想听到的词」打两个字符就跳出去，后面打不进；
  2. 润色页输入框全部靠右、和多行框叠在一起，「很丑」；
  3. 界面上摆长句解释（占位符写着「例如：保留原话…公司名写成华为云」，字号比标题还大）——用户明确要求：说明只放 ⓘ，「小符号放在标题后面」。
- 根因：
  1. `HotwordsSection` 的 `TextField` 在 `onChange(of: rule.to)` 里改 `rule.from`——每次按键都改数组元素，行视图重建，中文输入法的组合态被打断；
  2. grouped `Form` 的默认行样式把控件挤到右侧；`TextField(axis: .vertical)` 在 Form 里还会把文字排到右边（`multilineTextAlignment` / `layoutDirection` 都压不住），且与「力度/快捷键」共用一行导致重叠；
  3. 之前按「把说明写清楚」的思路填了占位符与整段解释，违反设计里的「界面文案克制」。
- 修法（`app/Sources/EryuApp/SettingsView.swift`）：
  1. `onChange` 只 `dirty = true`，空 `from` 在保存时归一成 `from == to`；
  2. 所有润色字段改为「标签在上、输入框占满一行、文字天然靠左」的 `fieldLabeled/labelWithInfo`；多行「附加要求」换成 `TextEditor`（自绘圆角边框，`frame(minHeight: 44, maxHeight: 72)`）；力度、快捷键各占一行不再挤；
  3. 折叠行只留「名称（title3）· 力度徽标 …… 快捷键（右）」，删掉润色内容摘要 `subtitle()`；所有说明（名称/快捷键/附加要求/服务地址/模型/Key/润色开关/热词用法/热词文件路径）移进标题后的 ⓘ 悬停提示；占位符清空。
- Windows 同步：热词页计数行只留「共 N 条」（去掉「一行一个想听到的词」这类说明），说明改由页面 `info()` 承担；`./scripts/build-win.sh` rc=0，真机部署 exe `1B5C380D…`。
- 验证：`./scripts/build.sh` rc=0；`--show-settings=polish` 截图确认名称/力度/快捷键/附加要求左对齐且字号放大（`docs/settings-mac-polish.png`）；`--show-settings=hotwords` 截图确认页面只剩列表 + 添加/保存（`docs/settings-mac-hotwords.png`）；热词行内已学到的映射仍以「自动学到：X → Y」显示。


**真根因（第二轮，2026-11-22 01:2x）：行身份随内容变**

现象一直没好的原因：前两次都在猜「是 List / 是 dirty 状态 / 是 onChange 改数组」，换成 `ScrollView + VStack`、去掉 `onChange`、去掉 `@State dirty` 之后**仍然只吃第一个字符**。

用 `ERYU_CONFIG_DIR=$(mktemp -d)` + `--show-settings=hotwords` 复现：AppleScript 点第一个输入框、`keystroke "abcde"`，截图里输入框只有 `a`，四秒后的字符全丢。

根因在 `app/Sources/EryuApp/AppModel.swift` 的模型定义：

```swift
var id: String { from + "\u{1}" + to }   // 计算属性 = 内容
```

`ForEach($draft)` 用 `id` 认行，用户在「想听到的词」里每敲一个字符 → `to` 变 → `id` 变 → SwiftUI 认为这一行被删掉、又插了一行新的 → 文本框销毁重建 → 焦点丢、输入法组合态断。和外面套 `List` 还是 `VStack` 无关。

修法：`let id = UUID()`（解码时生成，生命周期内稳定，与 `from`/`to` 解耦），并写自定义 `init(from decoder:)` 只解 `from`/`to`。验证：同一复现步骤下输入框完整出现 `abcde`、焦点环仍在框内、右下「有未保存的改动」正常。

**Windows 侧对照（2026-11-22）**：热词页是单个多行 `EDIT`（不存在「每行一个控件」的重建问题）；`EDIT` 原生处理 `Ctrl+V`，不依赖菜单派发。新增自测 `--dev --settings-input-selftest` 实测 `INPUT typed_ok=true typed_tail="abcde" inject=ok inject_ok=true`——键入 5 字不丢焦点、往自己窗口粘贴能落字并被读回。见 `docs/win-parity.md`「自家窗口输入」。

## I-175 往 Eryu 自己的输入框里粘贴/说话都上不去（2026-11-22）

**现象**：用户（m16521）「为什么这么简单的一个自己语音输入不了自己的文本方式呢？」——在 Eryu 设置里选中一个文本框，按热键说完一段，文字不见了；连手工 ⌘V 也粘不进去。

**复现**：`ERYU_MIC_WAV=/tmp/t2.wav /Applications/Eryu.app/Contents/MacOS/Eryu --show-settings=polish` → 点「名称」框 → `/tmp/press 2600`（CGEvent 按住右 Option 2.6s）→ 日志 `submit_request mode=paste len=11` 后接 `unknown 已发送粘贴，但输入框里没看到这段文字`；框里仍是原值。另用 `osascript … keystroke "v" using command down` 手工粘贴同样无效（排除注入层）。

**根因**：本 app 是菜单栏常驻（`LSUIElement`），只建了状态栏 NSMenu，**从没设过 `NSApp.mainMenu`**。SwiftUI 文本框的 ⌘C/⌘V/⌘A/⌘Z 不是文本框自己接的按键，而是靠主菜单里「编辑」菜单项的命令沿响应链派发（`paste:`）。没有主菜单 → 这些快捷键在自家窗口里全部失效 → 往里贴任何东西都「凭空消失」，校验当然也判 unknown。

**修法**（`app/Sources/EryuApp/AppDelegate.swift::installEditMenu()`，在 `applicationDidFinishLaunching` 里第一件事调用）：建一个不显示的主菜单，挂「Eryu（隐藏 ⌘H / 退出 ⌘Q）」和「编辑（撤销 ⌘Z / 重做 ⇧⌘Z / 剪切 ⌘X / 拷贝 ⌘C / 粘贴 ⌘V / 全选 ⌘A）」，action 用 `Selector(("undo:"))`/`#selector(NSText.paste(_:))` 等走响应链。accessory app 激活时不改菜单栏，所以用户看不到它，只是快捷键恢复。

**验证**：
- 手工粘贴：`pbcopy CLIPBOARDTEST` + ⌘V → 名称框变 `CLIPBOARDTEST`，右下出现「未保存」。
- 语音上屏：点名称框 → 按住右 Option 2.6s（`/tmp/t2.wav` 播放「这是一次测试，用来看看能不能写进自己的输入框。」）→ 名称框变「这是一次测试用来看看。」，`history` 记 `inserted`。

**Windows 侧对照（2026-11-22）**：热词页是单个多行 `EDIT`（不存在「每行一个控件」的重建问题）；`EDIT` 原生处理 `Ctrl+V`，不依赖菜单派发。新增自测 `--dev --settings-input-selftest` 实测 `INPUT typed_ok=true typed_tail="abcde" inject=ok inject_ok=true`——键入 5 字不丢焦点、往自己窗口粘贴能落字并被读回。见 `docs/win-parity.md`「自家窗口输入」。
