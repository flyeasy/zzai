# 决策记录（ADR）

规则：每条决策一个编号，写明**决策 / 理由 / 影响的设计章节 / 日期**。设计文档 `DESIGN.md` 与这里的结论冲突时，以 DESIGN.md 为准；要改 DESIGN.md 必须同时新增或修订这里的条目。

---

## D-001 文档分层与权威顺序（2026-11-19）

- **决策**：`DESIGN.md` 是唯一 SSOT；决策进 `docs/DECISIONS.md`，用户需求与变更进 `docs/REQUIREMENTS.md`，实现问题进 `docs/ISSUES.md`，进度进 `docs/PROGRESS.md`。代码服从文档，不允许"代码先跑偏、文档后补"。
- **理由**：用户要求保持设计文档的优先级与决定权；此前 v0.2→v0.10 出现过"同一行为四种说法"的补丁叠加问题（见 `DESIGN-audit-deepseek.md`）。
- **约束**：任何偏离初始方向的需求，先在 `docs/REQUIREMENTS.md` 登记为显式变更（`R-xxx` + 状态=已批准），再动 DESIGN.md 与代码。

## D-002 无 Xcode：SwiftPM 构建 + 脚本组装 .app（2026-11-19）

- **决策**：本机只有 Command Line Tools（`/Library/Developer/CommandLineTools`，macOS SDK 26.5），没有 Xcode.app。因此放弃 Xcode 工程，改用 `swift build` 出可执行文件，再用 `scripts/bundle.sh` 组装 `Eryu.app`（手写 Info.plist，`LSUIElement=true`）。
- **理由**：安装 Xcode 要 ~10GB，对个人工具不划算；SwiftPM 能编 SwiftUI/AppKit，只是不能出归档包。
- **对 DESIGN 的影响**：§5.2 第 2 步"Xcode clean build"改为"`cargo build --release` + `swift build --clean` + 组装 + `otool -L` 检查"。

## D-003 签名：先 ad-hoc，必要时升级为本地自签名证书（2026-11-19）

- **决策**：当前 `security find-identity -v -p codesigning` = **0 个有效身份**。M1a 起步用 ad-hoc（`codesign -s - --identifier com.lucien.eryu`）。
- **理由**：先跑通链路，不阻塞。
- **已知成本**：ad-hoc 的 designated requirement 含 cdhash，**每次重编译都可能让 TCC（麦克风/辅助功能）重新授权**。若实测确实每次都要重授，则执行 D-003b：用自签名证书（login keychain）签名，使 DR 锚定证书而非 cdhash。
- **对 DESIGN 的影响**：§6「TCC 身份先定」保留；但"用同一 Apple Development 身份即可避免重授"改为"用同一自签名身份证书"。

## D-004 壳先纯 AppKit，SwiftUI 从设置页开始（2026-11-19）

- **决策**：M1a 的壳用 `NSApplication` + `NSStatusItem`（`.accessory` 激活策略、LSUIElement），不引入 SwiftUI App 生命周期；M1b 的设置页/统计页用 SwiftUI Hosting 嵌入窗口。
- **理由**：菜单栏常驻 + 手动 .app 组装下，AppKit 生命周期最可控；SwiftUI 的 `@main App` 在无 Xcode 工程的组装包里容易踩 `NSPrincipalClass`/激活策略的坑。
- **影响章节**：§3 架构图（壳侧职责不变）。

## D-005 热键后端：`RegisterEventHotKey`（Carbon）独占（2026-11-19）

- **决策**：M1a 用 `RegisterEventHotKey` 注册「修饰键 + 普通键」，靠 `kEventHotKeyPressed/Released` 拿按下/松开；保留键（⌘Space、⌃Space 等）直接拒选。
- **理由**：`NSEvent` 全局监听收不到本 App 自身事件、不能吞键；`global-hotkey` 在 macOS 必须主线程构造且语义不全（见 §6）。
- **待验证**（I-001）：Carbon 热键在 macOS 26 上是否仍给 `Released` 事件；若不给，退化为"按一下开始、再按一下结束"（仍可用，但需改交互文案）。

## D-006 FFI 事件协议 v1（2026-11-19）

- **决策**：core → 壳的每条事件是单行 JSON：
  `{"schema":1,"type":"state|result|error|progress|stats|submit_request","seq":n,"session":n,"generation":n,"payload":{...}}`
  - `state.payload.phase` = `idle|starting|listening|processing|inserting`
  - `result.payload.{task_id,text,segments[],partial}`、`error.payload.{code,message,detail}`
  - **注入方向**：core 发 `submit_request{task_id,text,target}`；壳执行后调 `eryu_submit_result(task_id, status, detail)`，`status ∈ ok|undelivered|unknown`；同一 `task_id` 只接受一次。
- **理由**：审计 P0「回调要有 schema/session/generation/seq」；JSON 让壳与 core 的版本独立演进。
- **影响章节**：§3.1 硬规则 4 与 8。

## D-007 VAD 必须逐窗喂（512 采样），不接受整段（2026-11-19）

- **决策**：`vad::Segmenter::accept` 内部按 `WINDOW_SAMPLES = 512` 切块逐个 `accept_waveform`，不再整段一次性喂。
- **理由（实测）**：整段喂 `zh.wav`（5.59s 语音）只吐出尾部 0.31s 一段；逐窗喂得到 4.39s 完整段。`core/examples/vadprobe.rs` 里 `VAD_WHOLE=1` 可复现两者差异。
- **影响**：`core/src/vad.rs` 注释已写明；`WINDOW_SAMPLES` 与 `cfg.silero_vad.window_size` 同源。

## D-008 模型安装位置与获取方式（2026-11-19）

- **决策**：模型装 `~/.eryu/models/`（`Config` 默认值），获取走 `scripts/fetch-models.sh`（GitHub k2-fsa/sherpa-onnx `asr-models` tag）；env 可覆盖 `ERYU_MODEL_DIR` / `ERYU_VAD_MODEL` / `ERYU_ENGINE`。
- **实测**：sense-voice int8 解包 233MB，silero_vad 632KB；引擎加载 320–410ms；5.6s 音频 RTF 0.046；55.7s（9 段）总 2.36s。
- **影响**：DESIGN §4.2 引擎表与 §11 实测数据；不内置模型，首次使用需联网下载（与「听写全本机、下载需联网」口径一致）。

## D-009 静态库链入需补 libc++（2026-11-19）

- **决策**：`app/Package.swift` 的 linkerSettings 加 `.linkedLibrary("c++")`；ABI 符号强引用放 `app/Sources/EryuC/shim.c` 的 `__attribute__((used))` 指针表（Swift 侧 `-O` 会优化掉 `_ = symbol` 引用）。
- **理由**：链入 sherpa-onnx（C++）后 Swift 链接报 `std::logic_error::what() const` 等未定义符号；`cargo rustc --release -- --print native-static-libs` 给出的完整清单是 `-lc++ -framework Foundation -lc -lm -liconv -framework AudioUnit -framework CoreAudio -framework CoreFoundation -lSystem`。
- **教训**：新增 ABI 函数要同步三处（C 头、shim.c 引用表、Swift `keepABISymbolsAlive`），否则 `scripts/smoke.sh` 第 2 项会红。

## D-010 设置页走 config JSON，不直接改 TOML（2026-11-19）

- **决策**：ABI 同时提供 `eryu_get_config_toml / set_config_toml`（原文件语义，诊断页用）与 `eryu_get_config_json / set_config_json`（设置页绑定用，只认出现过的键）。
- **理由**：JSON 面能只提交用户真的改过的字段，避免设置页把用户手写在 TOML 里的注释/未知字段冲掉；TOML 面保留给「高级用户直接编辑」这一条路（DESIGN §5.1）。
- **影响**：`Config::merge_json` 做白名单 + clamp；`set_config_json` 失败发 `config_save_failed` 并保留旧配置。

## D-011 引擎热切换在后台线程重建，下次录音生效（2026-11-19）

- **决策**：`eryu_set_engine` 只校验模型存在 → 存配置 → 发 `config_changed` → 起后台线程建新引擎 → `EngineBuilt` 换入；录音中不切。
- **理由**：SenseVoice 加载 320–410ms、Qwen3 1.2–1.7s，同步切会让 UI 卡住；审计要求「音频回调不持锁、不做重活」同理。
- **影响**：`engine_ready` / `engine_switch_failed` 事件；「下次录音生效」写进设置页文案。

## D-012 首次运行写出默认 config.toml（2026-11-19）

- **决策**：`Config::load` 发现文件不存在时，加载完默认值后立刻 `save()`（带注释的 to_toml）。
- **理由**：用户要能直接编辑配置文件；设置页「诊断」里的 TOML 编辑器也需要有文件可改。写失败只记 `load_warning`，不阻断启动。
- **影响**：`~/.eryu/config.toml` 0600；`Eryu --dump-config` 可查看当前生效值。

## D-013 失败归因按「音量事实」分档（2026-11-19）

- **决策**：用 `audio::peak(pcm)` 与 `SILENCE_PEAK = 0.01` 区分三类失败：
  1. 峰值 < 0.01 且录音时长不足 → `no_audio`（指名 `last_device`，让用户去查麦克风）
  2. 峰值 < 0.01、时长够 → `empty_result`「录音几乎是静音」
  3. 峰值 ≥ 0.01、识别为空 → `empty_result`「声音太小或不清楚」
  4. 有声音但时长 < `min_record_ms` → `too_short`（报实际 ms 与阈值）
- **理由**：DESIGN §10.1 第 6 条要求静音 clip 独立归因（AutoGLM 的做法是直接让用户查麦克风）。给用户错误的原因，比给一个笼统的「识别失败」有用。
- **影响**：`core/src/audio.rs` 的 `peak`/`SILENCE_PEAK`；`core/src/lib.rs` 的 `begin_decode` 与 `on_decoded`。

## D-014 结果先落盘再上屏，取回后清暂存（2026-11-19）

- **决策**：`on_decoded` 在发 `submit_request` **之前**把文本写进 `~/.eryu/pending/last_result.txt`（0600，tmp+rename）；`submit_result == ok` 时删除；启动时若文件还在则读回内存并发 `pending_result` 事件，同时保留文件等用户取回；用户点「复制上次结果」调 `eryu_drop_last_result` 删除暂存；`pending/` 里超过 24h 的文件由 `cleanup_failed_clips` 清理。
- **理由**：审计 P1「崩溃丢结果需 pending_result 落盘」。写在上屏之前，是因为「已识别但没上屏」才是会丢的那一段。
- **影响**：ABI 加 `eryu_drop_last_result`（头/shim/Core.swift 三处同步）；`--print-last` 可用于验证恢复路径。

## D-015 权限分三项独立体检，只说缺什么和缺了会怎样（2026-11-19）

- **决策**：`app/Sources/EryuApp/Permissions.swift` 分开查麦克风（`AVCaptureDevice.authorizationStatus`）、辅助功能（`AXIsProcessTrusted`）、输入监控（`CGPreflightListenEventAccess` + `CGPreflightPostEventAccess`）；启动时只报「缺少权限：X、Y」+ 指向设置 → 诊断；诊断页有权限矩阵（状态/后果/去设置），麦克风未询问时可主动请求。
- **理由**：DESIGN §5 M1c「权限丢失检测 + 指名提示」；审计 P0「别首启动索要全部」「麦克风 entitlement + usage description」。
- **影响**：ad-hoc 重签会让 TCC 失效并重新弹窗（I-013）。

## D-016 热词替换用「单遍最长匹配扫描」，不用 Aho-Corasick（2026-11-19）

- **决策**：`core/src/hotwords.rs` 把规则按 from 字符数降序排列，逐字符位置尝试匹配，第一条命中即最长匹配；不做分词、不引入 `aho-corasick` 依赖。英文/数字边界规则：规则两端任一为 ASCII 字母数字时，两侧字符不能也是字母数字（`eryu` 不会替换 `eryuabc` 的子串）。规则文件 `~/.eryu/hotwords.toml`（0600），首次启动写带注释的空模板。ABI v1.2 加 `eryu_get_hotwords` / `eryu_set_hotwords`（JSON 面，设置页不必懂 TOML）。
- **理由**：单次转写文本最长约 300 字，规则通常 < 50 条，O(n·m) 扫描在微秒级，不需要 AC 自动机的常数开销；最长优先 + 词边界已覆盖 DESIGN §4.3 的三条约束。JSON 面沿用 D-010 的思路（UI 只碰结构化数据，TOML 由 core 负责）。
- **影响**：热词改动保存即生效（`hotwords_changed` 事件），不重建引擎；`apply_config` 换目录时重载热词表。`recognize_wav` 例程也过一遍热词表，便于离线核对。

## D-017 仓库纳入 git，二进制与逆向材料不入库（2026-11-19）

- **决策**：`git init` 后 `.gitignore` 排除 `core/target/`、`app/.build/`、`build/`、`vendor/sherpa/`（91MB 静态库）、`artifacts/`（167MB 逆向材料与 DMG）。
- **理由**：项目要长期迭代，"哪一步改了什么"必须可回溯；静态库和 DMG 可由 `scripts/fetch-models.sh` 与历史记录重建。
- **影响**：里程碑提交；`.git` 目前 1.2MB。

## D-018 用自签名证书「Eryu Dev」固定代码签名身份

- 背景：ad-hoc 签名每次重建 CDHash 变化 → TCC（麦克风/辅助功能）每次都失效，用户要反复点（I-013）。
- 做法：`openssl` 自签 10 年证书（`basicConstraints=CA:false`、`keyUsage=digitalSignature`、
  `extendedKeyUsage=codeSigning`、CN=Eryu Dev）→ `openssl pkcs12 -export -legacy`（**必须 `-legacy`**，
  否则 macOS 导入报 `MAC verification failed`）→ `security import` 到 login 钥匙串并 `add-trusted-cert -r trustRoot`。
- 效果：`codesign --sign "Eryu Dev"` 无 GUI 授权、`Authority=Eryu Dev`，TCC 跨重建保持。
- 代价：首次启动 Gatekeeper 会显示「正在验证 Eryu…」（一次性）；证书只在本机可信，不用于分发。

## D-019 浮层用 AppKit 绘制，不用 SwiftUI

- 原因：I-017（NSHostingView 在透明无边框 NSPanel 里不合成）。
- 代价：浮层要手写布局与动画（30Hz Timer 驱动电平条），但换来「一定能显示」；
  设置窗口继续用 SwiftUI（普通 NSWindow 正常）。

## D-020 准流式预览用「整段重跑」而非流式模型

- 原因：SenseVoice/Qwen3 都是段级模型；重跑整段能复用同一条 VAD+热词+润色链路，
  接缝行为与最终上屏完全一致；900ms 间隔下 RTF≈0.05 的开销可接受。
- 备选（未来）：换 streaming zipformer 做真流式；或按音频长度自适应间隔。

## D-021 单键热键统一走 CGEventTap（.listenOnly）

- 原因：Carbon 不接受纯修饰键，也不接受裸字母；用户要求「任何单键都能按住说话」。
- 取舍：`listenOnly` 不拦截按键，所以裸字符键会边录音边打字——设置页明确警告，并推荐右 Option/Fn/F13。

## D-022 默认热键改成单键「右 Option」（ALT_R）

- 依据：用户 m01947/m02028 明确不要「只会组合键」，右 Option 只是举例。
- 取舍：右 Option 不产生字符、不与打字冲突；代价是按住它会暂时改变其它应用的 ⌥ 行为（录音期间本来也不会打字）。
- 保留：组合键与任意单键仍可在设置页配置（R-013）；裸字符键会警告「边录音边打字」（D-021）。

## D-023 隐私模式的范围（不写历史、不落盘、浮层不显示文本）

- 触发：`[privacy] enabled = true`（设置页「通用 → 隐私」，或 `eryu_set_config_json {"privacy_mode":true}`）。
- 生效点：`Store::record_success/record_failure` 全部跳过；失败音频只留内存（`Worker.last_failed`），不写 `pending/last_failed.wav`；
  `save_pending_result` 不写 `pending/last_result.txt`；浮层只显示「已上屏 N 字」与「识别中…（隐私模式）」。
- 不变的部分：热键、上屏、引擎、热词、润色照旧；本次运行内仍可 ⌘R 重试失败音频（内存里有）。
- 理由：隐私模式要能「开着用」，不是关掉功能；落盘才是风险点，功能不是。

## D-024 选区改写优先用辅助功能写回，失败才走粘贴

- 首选：`AXUIElementSetAttributeValue(focused, kAXSelectedTextAttribute, newText)`——不动剪贴板、不模拟按键、不留粘贴痕迹。
- 退路：写剪贴板 + 合成 ⌘V（与上屏同一条路）；再失败则把结果留在剪贴板并提示 ⌘V。
- 指令由应用侧给（润色/正式/精炼/英文/总结），core 只负责「按指令改写」这一次网络调用（`eryu_run_command`，ABI v1.4）。
- 理由：选区操作必须"就地"，粘贴会改变撤销栈、也会在 Electron 应用里触发 750ms 的剪贴板恢复逻辑。

## D-025 选区读取的三态

- 读到文本 → 正常进入指令浮层。
- 焦点元素存在但 `AXSelectedText` 为空 → 提示「没有选中文字」（不是错误，是常见操作）。
- 拿不到焦点元素（AX 未授权）→ 提示去授权，绝不静默失败。

## D-026 热词自动学习不弹窗，只进「待采纳」列表

- 上屏成功后 `EditObserver` 盯该输入框 15s（1s 一次），值不再包含上屏文本时取公共前后缀之间的差异作为候选。
- 过滤：长度比 0.6–1.6、原词 2–24 字、新词 1–24 字、必须含字母/汉字、新词不包含原词（"深度求索"→"深度求索AI" 这类不学）。
- 候选只进内存（设置页「热词」顶部「自动学到的改法」），用户点「采纳」才写 `hotwords.toml`——不打断输入、误学的代价是一次点击。
- 隐私模式不观察；AX 读不到值就直接放弃（不重试、不报警）。

## D-027 上屏方式的取舍：默认粘贴，打字作为兼容手段

- 默认 `paste`：一次 ⌘V，文本再长也是毫秒级；代价是要动剪贴板（已有 changeCount 双校验 + 750ms 恢复）。
- 备选 `keystroke`：`CGEventKeyboardSetUnicodeString` 逐批（16 个 UTF-16 单元）发 keyDown/keyUp，不动剪贴板；
  代价是慢（每批 6ms）、会触发输入法的联想/组词行为、安全输入下必失败。
- 决策权在 core：`config::mode_for(cfg, app)` 先按 `[insert] per_app` 包含匹配，再回落到 `[insert] mode`，
  通过 `submit_request.mode` 告诉壳；壳按自己的能力执行，做不到就返回 `undelivered` 说清原因。
- 不做「先粘贴、失败再自动打字」：一次上屏只尝试一种方式，避免同一段文本进两次。

## D-028 注入后校验：只读一次，不做二次注入

- 粘贴前记住焦点控件（`AX.focusedElement()`），粘贴后 350ms 读一次它的 `AXValue`。
- 读到且包含上屏文本 → `ok`；只包含开头 8 字 → `unknown`（可能被输入框长度限制截断）；完全不含 → `unknown` + 提示从菜单取回。
- 读不到值（终端、自绘控件、WebView 内部）→ 按 `ok` 处理并记日志：**"无法校验"不等于"失败"**，否则会在终端里误报。
- 无论校验结果如何都不再发第二次粘贴（终审约束），文本始终可从「复制上次结果」取回。

## D-029 模型下载：清单内置 + 系统 curl + tar 解包

- 清单内置在 `core/src/models.rs`（id / url / 文件名 / 标志文件 / 预估体积 / 是否压缩包），不做远程 manifest：
  自用工具，改清单就是改一行常量，比维护一个远端 JSON 更可控。
- 下载用系统 `curl`（见 I-024），`-C -` 断点续传，按 `.part` 体积回报进度；解包用系统 `tar -xjf`（macOS/Windows 10+ 的 bsdtar 都支持 bz2）。
- 同一时刻只允许一个下载（`pulling` 槽位），完成后删压缩包；标志文件存在即视为已安装。
- 事件：`model_progress{id,phase,received,total}` / `model_done{id,path}` / `model_failed{id,message}`；ABI v1.5 `eryu_models_pull`。

## D-030 引擎实测对比：默认仍 SenseVoice，Qwen3 作为「英文/多语」精度档

Rust 侧同一套 VAD+流水线实跑（示例 `core/examples/recognize_wav.rs`，本机 M5）：

| 样本 | SenseVoice int8 | Qwen3-0.6B int8 |
|---|---|---|
| zh.wav 4.39s | 「开放时间早上九点至下午五点」256ms / RTF 0.046 | 「菜饭时间：早上九点至下午五点。」1198ms / RTF 0.214（**中文错字**） |
| en.wav 5.08s | 两段英文正确、无标点 | 两段英文正确、带标点，1815ms / RTF 0.254 |
| codeswitch.wav 6.23s（法/英） | `I'M ALONE ALL BY MYSELF ISH TO SELD SOO T ATOY SOO`（后半段崩） | `I'm alone, all by myself. Je suis tout seul, ...`（带标点、可读）3071ms / RTF 0.493 |
| 加载 | 364–414ms | 1309–1336ms |

结论：**中文场景 SenseVoice 更准也快一个量级**；Qwen3 在英文/多语与标点上明显更好，但慢 5–10 倍、内存约 1.6GB。
所以默认保持 SenseVoice，Qwen3 由用户在「设置 → 引擎」按场景切换（这正是「模型热插拔」的用处），不做自动按语言切换。

## D-031 准流式预览改自适应间隔（取代固定 900ms）

- 原实现固定 `PARTIAL_INTERVAL_MS = 900`，且每轮都把「已录整段」重跑一遍（D-020）。录音越长，单轮解码越慢，
  固定间隔会让解码线程越积越多次任务——用户感受是「录到 1 分钟时浮层卡住」。
- 改成：`partial_interval_ms(last_decode_ms) = clamp(last_decode_ms * 2, 350, 1200)`。
  短句 60ms 解码 → 350ms 一轮（出字更快）；20 秒以上单轮 ~600ms → 退到 1200ms，机器不被压满。
- `partial_text` 事件新增 `ms` 字段，`--mic-test` 会打印每次预览的解码耗时，方便实测。
- 单测 `interval_adapts_to_decode_cost` 锁死四档行为。

## D-032 「松手→出字」可观测 + 预览与最终识别不再抢引擎

- 现象：`--mic-test 8` 松手后 864ms 才出结果，而纯识别只要 ~150ms。
- 根因：准流式预览（D-020：整段重跑）和最终识别共用同一个 `Arc<Mutex<Box<dyn AsrEngine>>>`；
  松手时若预览正在跑，最终识别先卡在锁上等着。
- 处理：
  1. `result` 事件新增 `latency_ms`（松手→出字，含热词/润色），菜单显示「上次：松手→出字 Nms」；
  2. `pipeline::run_cancellable(..., cancel: Option<&AtomicBool>)`：预览线程在每段前检查取消位，
     松手时置位并最多等 200ms——预览立刻退出、让出引擎；
  3. 松手后迟到的预览结果直接丢弃（phase 不再是 Listening）。
- 实测（M5，8s 含少量人声的录音，连跑）：864ms → 787ms → 481ms（识别部分 490/468ms 以内）。

## D-033 准流式预览只解最近 20 秒（替代「整段重跑」的无限增长）

- 用户实际会连说 1–2 分钟（m00819）。按 D-020 的「整段重跑」，60s 音频一轮预览要 ~3s，
  到 120s 会话上限时 ~6s——浮层上的字会明显跟不上。
- 改成：录音超过 20s 后，预览只解尾部 20s，文本前面加「…」表示前面还有内容（**最终上屏仍是整段识别**，不受影响）。
- `partial_window(pcm) -> (&[f32], bool)` + 单测锁行为；预览的耗时因此恒定在 ~1s 以内。

## D-034 录音到「会话上限」自动结束并提示（不再悄悄丢开头）

- 原实现：`audio::cap_len` 硬编码 120s，超出就从**开头**丢采样——用户说了 130 秒，前 10 秒无声消失，界面毫无提示。
- 改为：用 `cfg.max_record_secs`（设置页 30–300s 可调）作为预算；录音中一到上限就
  `notice{code:"record_limit"}` + 自动 `up()` 进入识别（跟松手同一条路径），音频不再被截头。
- 修复时踩到：`Machine::up()` 在 Listening 相位会返回 `Step::Finish{task_id}`，**必须**拿它去 `begin_decode`，
  否则会话卡在 Processing 直到 60s 超时（第一次实测 `MICTEST FAIL 超时` 就是这个）。
- 实测（上限临时设 5s，`--mic-test 10`）：`提示 record_limit: 到 5 秒上限了…` → `RESULT 松手→出字 618ms（识别 312ms）` → 结果正常。

## D-035 背景降噪：默认开，但只在采集侧（48kHz 原生）做

- 实现：`core/src/denoise.rs` 用纯 Rust `nnnoiseless`（RNNoise 0.5.2）逐 480 样本帧处理；
  采集回调 `core/src/audio.rs::start_with_opts(device, denoise)` 在 **48kHz 原生流**上直接降噪，再走有状态重采样到 16k。
  没有 48k 的麦克风跳过（不做 16k 上采样再降噪——那会引入伪影）。
- 为什么不在解码侧做：先试过「16k 上采样到 48k → 降噪 → 抽回」，实测在 5dB 白噪声上
  「开» 把「下午五点」听成「下午九点」（de-noising 在 8k 以上全是插值出来的空频带，RNNoise 判断失真）。
- **A/B 实测**（TTS 中文长句 + 白噪声，同一句，48k 素材；用 `examples/denoise48` 生成对比文件，再 `recognize_wav` 识别）：

| 噪声 | 不降噪 | 采集侧降噪 |
|---|---|---|
| 0dB | `今天下午三点…`（**丢了开头「我们」**） | `我们今天下午三点…`（**全对**） |
| −5dB | `下午三点…下一步的排期`（丢「我们」） | `我们今天下午三点…**才需**安排…提前**算**一下`（救回开头，后段退化） |

- 结论：0dB 这种很吵的场景下明显救回内容；−5dB 极端噪声下有好有坏。真实环境（空调/风扇 15–25dB SNR）两者都能识别，
  所以**默认开**并保留开关（设置 → 音频 → 录音时 → 背景降噪），并在 UI 文案里如实写「噪声再大时有好有坏」。
- 单测：`denoise::tests::{keeps_length_and_roundtrips_silence, attenuates_pure_noise}`。

## D-036 润色 profile：直接可编辑，不做内置只读 + fork

- 存储 `~/.eryu/profiles.toml`（0600）：顶部 `active = "日常"`，每条 `[[profile]]` 有 `name / icon / level / prompt`。
- 首次运行写三条内置（日常 light / 邮件 strong+邮件提示词 / 笔记 medium），**但都是普通可编辑条目**，不是只读模板。
  理由：个人自用工具，加「只读 + fork」只会多一层操作；改坏了删掉文件即可恢复（对应 DESIGN §9.6 的 fork 意图用「删文件重生成」满足）。
- `prompt` 不替换通用规则，而是拼在通用规则后面，隔一个 `【这一档的额外要求】` 小标题（`polish::enrich`）——避免用户写的一句附加要求
  把「不改变意思 / 不回答内容里的问题」这些底线规则顶掉。
- 切换即时写盘，但**只在下一个会话生效**：解码线程启动时对 profile 做一次快照（`prof.current()`），录音中途换档不影响本轮，符合「录音中拦截切换」。
- ABI v1.6：`eryu_get_profiles` / `eryu_set_profiles` / `eryu_set_active_profile`（名字不存在返回 -3，UI 不会切到空气里）。
- 测试隔离：`Config::apply_env` 认 `ERYU_CONFIG_DIR`，所以 FFI 测试用临时目录，不碰真实 `~/.eryu`。

## D-037 润色档快捷键槽：只收组合键，默认 ⌃⌥1/2/3

- 槽位存在 `profiles.toml` 每条 profile 的 `slot` 字段；前 3 档默认 `CTRL+ALT+1..3`，空字符串 = 不绑。
- **只接受带修饰键的组合**：默认「按住说话」是单键右 Option，如果槽位允许单键，按 ⌥1 会先触发录音再切档，
  语义直接打架。因此单键/裸字母槽位在注册时被拒绝并给出中文原因（`HotkeySpec.parseBinding` 判 `.combo` 才算）。
- 实现：`app/Sources/EryuApp/ProfileHotkeys.swift` 一次 `InstallEventHandler` + 每个槽一个 `RegisterEventHotKey`
  （signature `'ERYS'`，id=序号），按下即 `onTrigger(name)`；壳里回调 `AppModel.activateProfile(name)` + 状态栏提示。
- 老文件迁移：`profiles.toml` 里所有 `slot` 都空（v1.5 之前生成的文件）→ 按顺序补 ⌃⌥1..3 并回写，不覆盖用户已配的。
- 自测：`--slot-test`（注册 → `debugFire(2)` → 断言触发「邮件」）→ `SLOTTEST OK 槽位=["日常","邮件","笔记"]`；`--diagnose` 显示「润色档：日常（light，CTRL+ALT+1）」。

## D-038 「松手→出字」延时的定义、记录与验收口径

- 定义：从 `Machine::begin_processing()`（松手真正进入识别）到 `on_decoded` 产出文本，**含**等准流式预览线程退出、
  VAD 切段、逐段识别、热词替换、润色（若开）。不含上屏（上屏由壳负责，另有 `submit_*` 事件）。
- 记录：`HistoryEntry.latency_ms`（老历史缺这个字段，`serde(default)` 兼容）+ `Stats.latencies`（最多保留最近 200 个样本）。
- 汇总：`stats_json.latency = {count,min,p50,p95,max}`；设置页「历史与统计」第五张卡显示 `p50 / p95`；CLI `--bench` 直接打印并判定
  `p95 ≤ 1500ms`（DESIGN §5 验收线）。
- 为什么不用「按下热键→出字」：那会把用户说话的时长算进去，数字没有意义。
- 首次实测（6 次真实录音，其中 5 次有效，另一台机器放音被麦克风拾起）：`min=86 p50=184 p95=525 max=525`，判定通过。
  样本量小、且内容来自环境音，只能说明**链路延时**，不构成识别准确率证据（记 I-027）。

## D-039 core 便携化：权限收紧只在 Unix 生效，Windows 降级为 no-op

- 原本散落在 `hotwords.rs` / `store.rs` / `profiles.rs` / `audio.rs` / `lib.rs` 的 `std::os::unix` 调用全部收进
  `core/src/fsutil.rs`：`restrict(path)`（Unix `chmod 0600`，其它平台什么都不做）与 `write_atomic` / `write_text_atomic`。
- 理由：Windows 没有 `chmod` 的等价物（ACL 是另一套模型，收紧要 `SetNamedSecurityInfo`），
  而「文件只能在用户自己的目录里」已经是个人自用工具的合理边界；**权限收紧失败绝不能让写入失败**。
- 顺带统一了原子写（先写 `.tmp` 再 rename），以前 `profiles.rs` 用的临时名是 `profiles.toml.tmp` 而热词用的是
  `toml.tmp`，两个文件同目录会互相覆盖——现在统一由 `write_atomic` 用 `<path>.tmp`，各自独立。
- 验证：`cargo test` 44 项（新增 `fsutil::tests` 两条：原子替换 + 自动建父目录 + Unix 上模式为 0600）。

## D-040 部分成功：只重试失败的那一段，结果只进剪贴板

- 背景：长听写里某一段识别失败时，整段重说代价太大；core 早就会发 `partial_result`，但 UI 只弹一行字。
- 做法：`pipeline::SegmentInfo` 在失败时携带该段音频（成功段为 `None`，不占内存）；
  worker 记住 `failed_segments`，ABI 加 `eryu_failed_segments()` / `eryu_retry_segment(index)`（**v1.7**），
  单段重跑在 `eryu-segment` 线程里做；事件 `failed_segments{indexes}` / `segment_retry_started` / `segment_result{index,text,chars}`。
- **单段结果绝不自动上屏**：注入是「每轮只提交一次」的（终审决定①），再次注入可能粘错位置、也可能覆盖用户刚打的字。
  所以重试结果只写剪贴板 + 状态栏提示「第 N 段重新识别好了（K 字），已复制到剪贴板」，粘哪儿由用户决定。
- 退出安全：`retry_busy: Arc<AtomicBool>`；`Drop for EryuCore` 在 join worker 之后最多等 5s 让它退出。
  不等的话（实测）进程/动态库销毁后线程还在调引擎 → `fatal runtime error: Rust cannot catch foreign exceptions, aborting`。
- 可测性：新增 ABI **v1.8** `eryu_decode_wav(path)`——把一段 16k WAV 当成一次「刚松手」的录音走完整链路，
  不需要麦克风、不需要出声；配 `ERYU_FAULT_FAIL_SEGMENTS=<段号>`（调试用故障注入）可端到端复现「部分成功 → 单段重试」。
- 实测（`/tmp/seg2.wav` = 两句话 + 1s 静音，两段）：
  - 无故障 → `WAVRESULT 第一句话是关于会议室和周三下午的安排第二句话说的是预算和发票金额`
  - 注入第 1 段失败 → `WAVFAILED [1]` → `WAVPARTIAL 2 段里有 1 段没识别成功` → `WAVRESULT 第一句话…安排` → `WAVSEGRESULT <重试得到的文本>`

## D-041 浮层出现在「你正在打字的屏」

- 背景：外接屏/多显示器下 `NSScreen.main` 是「含菜单栏的那块屏」，不一定是用户当前工作的屏；浮层跑到另一块屏等于没显示。
- 选屏顺序：①光标所在屏（`NSEvent.mouseLocation` + `NSMouseInRect`）②前台应用 `AXFocusedWindow` 的位置换算到屏幕坐标后落在的屏（AX 坐标原点在左上、向下为正，需翻转）③`NSScreen.main`。
- 垂直位置固定 `visibleFrame.minY + 88`（Dock 上方），水平居中；屏幕窄于浮层宽度 + 16pt 时夹到边界内。
- 为什么不跟随光标 y 坐标：贴光标会出现「浮层压在输入框上」；固定底部更稳定，也符合「余光可见、不挡内容」。
- 实测（单屏 1470×956 逻辑点）：`[eryu][overlay] 屏幕={{0,0},{1470,956}} frame={{0,73},{1470,850}} 面板={{525,161},{420,74}}`，截图可见胶囊在 Dock 上方居中（`--demo-overlay`）。

## D-042 资源基线：默认引擎 SenseVoice，常驻 ~555MB

- 用户关心「879MB 太挑机器」：那 879MB 是 Qwen3 的**磁盘**体积，不是常驻内存。
- 实测（MacBook Air M5 / 17GB / macOS 26.7.1，`--baseline` 连跑 3 次，引擎同步加载完就测）：
  - 冷启动到可听写：**444 / 582 / 840 ms**（首次含 Gatekeeper 校验，之后稳定在 0.5s 上下）
  - 常驻内存 RSS：**554 / 555 / 569 MB**（模型是 SenseVoice int8，233MB 磁盘）
  - 对照：Qwen3-0.6B int8 磁盘 838MB、加载 1.2–1.7s、峰值内存 1619MB（§11 spike 数据）
- 结论：默认 SenseVoice 对 8GB 机器也够用；Qwen3 作为「英文/多语精度档」由用户手装手切，不设为默认。
- 量法：`mach_task_basic_info` 的 `resident_size`（`task_info`），不用 `ps`（避免 fork 副本干扰）；口径写进 `--baseline` 输出。

## D-043 连按应急：短按后挂起 400ms，第二下=取回上次结果

- 需求（DESIGN §9.4）：连按不该产生「半句上屏」或「录音中再按取消」；双击要能把上次结果取回剪贴板，且**绝不自动注入**。
- 实现（壳侧，`AppDelegate.handleHotkeyDown/handleHotkeyUp`）：
  - 按下 <200ms 就松开 → **不立刻**把 `up` 交给 core，而是挂起一个 400ms 的 `DispatchWorkItem`；
  - 400ms 内又按下 → 取消挂起、`core.cancel()` 结束那个空会话、`copyLastResult()` 把上次结果写剪贴板（`hotkeyDoubleTapped = true`）；
  - 400ms 内没有第二下 → 交回 `core.hotkeyUp()`，仍走原来的失败归因（安静环境先报 `no_audio`，有声音但太短报 `too_short`）。
- 为什么不「先等 200ms 再开始录音」：那会把延迟加到每一次正常录音上，开头几个字会丢。先在 core 里开流，再来判定，判定期间音频已经在缓冲。
- 代价：真正的**极短语音**（<200ms）结果会晚 400ms 出现。这个区间本来就等于「没说清」，可接受。
- 为什么不在 core 里做：双击是「UI 层的连按语义」，且要读剪贴板、弹提示；core 只管一次会话的生命周期。
- 实测：`--double-tap-test` → `DOUBLETAP OK 识别双击=true 剪贴板=双击取回测试文本-DT1`；
  `--single-tap-test` → `SINGLETAP OK 收到太短提示=true 双击标记=false`（单按不会被吞）。

## D-044 浮层显示已录秒数（≥3s 才显示）

- 用户关心「按下就要看见在录音，而且知道它在收」：长句（30s+）时只有文字在变，容易被误以为卡住。
- 做法：core 的录音期 `meter` 事件多带一个 `secs`（`audio::secs_of(&self.recorded)`，15Hz 推送）；
  壳侧 `OverlayController.setSecs` 只在 `starting|listening` 阶段生效，**≥3s 才把状态改成「在听… 12s」**（前几秒保持简洁，避免字一直在跳）。
- 顺手修的坑：`overlay = OverlayController()` 原来在所有 CLI 自测分支之后才创建，导致 `--mic-test` 里浮层永远是 nil（自测看不到浮层）。
  现在紧跟 `self.core = core` 创建，所有自测分支共用同一个浮层。
- 实测：`--mic-test 8` 日志里能看到 `secs=7 → 在听… 7s`、`secs=8 → 在听… 8s`；截图确认胶囊里出现「在听… 8s」。

## D-045 `diagnose` 里放运行时真相

- 用户报问题时，我需要「他机器上现在到底是什么状态」。`eryu --diagnose` / 设置页「诊断」现在包含：
  引擎与两个模型是否就绪、VAD、输入设备与默认设备、历史条数、当前润色档与快捷键槽、热词条数、
  最短录音/会话上限、上屏方式与按应用覆盖条数、隐私/降噪/暂停播放开关、主键、**常驻内存（MB）**、**最近延迟 p50/p95/max**。
- 内存用 `task_info(MACH_TASK_BASIC_INFO).resident_size`（core 与壳同进程，量的是同一个数；`--baseline` 用的是壳侧同一指标）。
- 不写进诊断的内容：配置里的 API Key（只报「有没有」）、完整历史文本、音频路径。
- 实测（本机）：`常驻内存：378 MB`、`最近延迟：22 次，p50 424ms / p95 484ms / max 525ms`。

## D-046 句末标点兜底：只补句号，不猜逗号

- 问题：SenseVoice 输出基本不带标点，长听写出来是「第一句话是…安排第二句话说的是…金额」，读起来吃力。
- 做法：`pipeline::add_final_punct`——文本以中日韩文字结尾、长度 ≥6、且末尾没有标点时，补一个「。」。
  - 代码/命令（`int main`）、英文（`check the log`）、短词（`好的`）都不动。
  - 位置：热词替换之后、润色之前（润色模型看到句号会自然断句）。
- **不做段间逗号**：VAD 按 0.4s 静音切段，用户换气也会切段；自动插「，」会把一句话拦腰截断，误插比不插更烦。
  真正解决要上标点模型（sherpa-onnx 的 `punct-ct-transformer-zh-en`，约 290MB，VocoType 就是这条路），
  已记入 I-031，等用户愿意为标点多花 290MB 再做（默认不装，符合「别太挑机器」）。
- 实测：`--decode-wav` 输出从 `…预算和发票金额` 变成 `…预算和发票金额。`；`scripts/smoke.sh` 第 3b 项固化这条链路。

## D-047 菜单栏图标随会话阶段变化

- 浮层在光标所在屏，但戴耳机/外接屏/浮层被全屏应用盖住时，用户仍需要一个「一眼可见」的录音指示。
- 菜单栏图标按阶段换符号（template image，跟随浅色/深色）：闲置 `waveform`、准备/录音 `waveform.circle.fill`、
  识别 `ellipsis.circle.fill`、上屏 `arrow.up.circle.fill`；同时更新 tooltip 为「Eryu：在听…」。
- 出错时仍走 `setBadge`（`waveform.badge.exclamationmark`，3 秒后恢复）。
- 说明：系统自己的「橙色麦克风指示」也会出现，两者不冲突（一个是我们的状态，一个是系统在告诉你谁在用麦克风）。

## D-048 学到的改法要落盘（`suggestions.json`）

- 原来热词自动学习的候选只活在内存里：重启就没了，用户等于白等一次「学到」。
- 做法：`EditObserver.Suggestion` 加 `Codable`；`AppModel.saveSuggestions()` 写
  `$ERYU_CONFIG_DIR/suggestions.json`（缺省 `~/.eryu/`，与 core 同一目录），0600、原子写、最多留 50 条；
  `loadSuggestions()` 在 `refreshAll()` 里读回，并过滤「已经在热词表里的」。
- 采纳/忽略都立即写盘：采纳 → 进 `hotwords.toml` 并从候选移除；忽略 → 只从候选移除（不进热词表）。
- 文件坏了/不存在都当「没有候选」，不影响启动。实测：手写两条候选 → 重启打开设置页，候选区正确显示且可采纳/忽略。

## D-049 校验没确认上屏时，保留剪贴板

- 旧逻辑：粘贴后 750ms 无条件尝试还原剪贴板（只要 changeCount 还是我们的）。问题是——如果目标应用粘贴得慢、或者按钮/焦点变了导致粘贴没生效，
  剪贴板被还原意味着**这段文字在系统里彻底消失了**，用户只能靠「复制上次结果」抢救。
- 新逻辑：先按 `Injector.verify` 判定，**只有确认文本已经落到输入框（或读不到输入框但按已发送处理？不——这里按「确认」才算）时才还原**；
  判定为 unknown（只上去一部分 / 没看到 / 焦点变了）就**把这段文字留在剪贴板**，浮层与菜单会提示可取回。
  判定 ok 但目标其实是终端（读不到 AX 文本 → verify 直接返回 ok）的情形仍会还原，这是已知折中（I-033）。
- 与终审约束的关系：仍然**不做二次注入**，只是「复原剪贴板」这一步更保守。

## D-050 把资源基线做成冒烟项（回归护栏）

- `scripts/smoke.sh` 第 4b 项跑 `--baseline`，解析 `BASELINE 冷启动=Nms RSS=MB`，断言 **冷启动 <3000ms、RSS <1200MB**。
- 依据：本机实测冷启动 388–840ms、RSS 554–569MB；给的是「别悄悄翻三倍」的宽松上限，不是性能目标（目标是 p95 ≤1500ms 的链路延迟，由 `--bench` 看）。
- 坑：macOS 没有 `timeout` 命令（coreutils 的 gtimeout 才有），冒烟脚本里不能写 `timeout 60 …`，否则静默拿不到输出。

## D-051 「只录我的声音」用声纹门控（3D-Speaker eres2net，阈值 0.62）

- 需求来源：DESIGN §9.7 的「音源锁定」——用户最初想「麦克风只认我的声音」。真正的多源分离做不到（单麦无空间信息），
  但**段级声纹校验**是可做的：每段 VAD 音频先算 speaker embedding，和注册声纹余弦相似度低于阈值就跳过。
- 模型：`3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx`（39.6MB，512 维），走已有的模型下载器（`speaker` 条目，单文件非压缩包）。
- **阈值依据（`core/examples/voiceprobe.rs` 实测，TTS 声音）**：同一人不同句 0.916 / 0.931（2s 片段）；不同人同句 0.522、不同句 0.565 / 0.487（2s）。
  → 取 **0.62**，对「不同人」有 ≥0.06 余量，对「同一人」有 ≥0.29 余量。真实人声会比 TTS 波动大，所以界面上写明可随时关掉。
- 门控语义：**算不出（太短/太安静/模型缺失）一律保留**——宁可多识别，不要莫名其妙吞掉用户的话；只有算出且低于阈值才跳过。
- 保底：如果一段录音里 ≥1/3 的段被跳过（`total >= 3 && skipped * 2 >= total`），判定「可能是自己声音变了/麦克风换了」，
  **整段重跑一次不过滤**，并提示「疑似不认你的声音，改为整段重识别」；全部段都被跳过时明确告诉用户，不静默留空。
- 隐私：声纹是 512 维向量，存 `~/.eryu/voice_print.bin`（0600），不出本机；不进转写历史、不进统计。

## D-052 声纹注册有两条入口

- `eryu_voice_enroll_wav(path)`：用一段 16k 单声道 WAV 注册（自动化/排查用，`--voice-enroll-wav`）。
- `eryu_voice_enroll_mic(seconds)`：**同步阻塞**录 `seconds`（默认 8，clamp 3–20）秒麦克风音频再注册——设置页「录 8 秒当声纹」用这条。
  壳侧必须放后台队列（`AppModel.enrollVoice` 就是这么做的），否则会卡住主线程。
- 注册成功后清空 gate 缓存，下次录音用新声纹；删除声纹（`eryu_voice_clear`）幂等，没注册也会成功。

## D-053 长跑自测 `--soak`（同进程连解 N 次）

- 目的：`--decode-wav` 只证明「一次能解」，证明不了「用一天不涨内存」。`--soak N <wav>` 在同一进程里连续解 N 次，
  每次打印耗时与 RSS，结束时断言 **失败数 0 且 RSS 增长 <120MB**（超时 20s/次）。
- 实测（15 次，`/tmp/eryu-vl/t2.wav` 5.5s）：耗时 326→530ms 后稳定在 ~460–480ms，RSS 567→300MB（**负增长**：
  冷启动时模型/tokenizer 的临时缓冲在几轮后释放）。无泄漏迹象。
- 进了冒烟（第 3c 项，6 次）：`SOAK OK 次数=6/6 失败=0 RSS 568→297MB`。
- 注意：`--soak` 用主线程跑事件回环（不阻塞 runloop），每次「idle」事件触发下一次 decode；失败 3 次提前结束。

## D-054 补标点用本机 ct-transformer（可选 65MB），默认装了就用

- 现状：`pipeline::add_final_punct` 只在句末补「。」，所以长句变成「…评审设计文档请大家提前看一下预算和发票的部分。」。
- 选型：sherpa-onnx 的 **OfflinePunctuation**（ct-transformer，int8 包 **64.7MB**，整包 279MB 不要）。
  `core/examples/punctprobe.rs` 实测：加载 **99ms**、每句 **10–11ms**；
  「我明天下午三点在会议室评审设计文档请大家提前看一下预算和发票的部分」→「…文档，请大家…的部分。」。
- 落地：`core/src/punct.rs`（`DIR_NAME` + `model_path()` + `Punctuator::load/apply` + `Cache`），
  解码线程 **热词之后、润色之前** 调用；`config.punct`（默认 true）关掉或模型缺失就退回 `add_final_punct`。
  `models.rs` 增 `punct` 条目（65MB，tar.bz2）；设置页「音频 → 录音时」有开关，未装时同一行给「下载标点模型 65MB」。
- 代价：常驻内存约 **+90MB**（模型 + 运行时），单次 **+10ms**；A/B（`--soak 6`，同一段两句话）延迟 556ms vs 586ms，差异在噪声内。
- 端到端（`--decode-wav`）：开启 →「今天天气不错，我们出去走走吧，顺便买点水果回来。」；
  关闭 →「第一句话是关于会议室和周三下午的安排第二句话说的是预算和发票金额。」（只有句末句号）。
- 取舍说明：**没有**把标点模型做成默认下载（65MB 对个人工具也不小），但装了默认启用；`--diagnose` 会写「标点模型：开（已装 …）/（未装，只在句末补「。」）」。

## D-055 自测模式：自动化路径不写历史、不落盘（ABI v1.11）

- 问题：跑一次 `scripts/smoke.sh` 就会往用户的 `history.json`/`stats.json` 里塞 3–5 条假记录（
  `--self-test`、`--decode-wav`、`--soak` 都走真实解码链路），几天下来「转写 200 次」全是测试。
- 做法：`EryuCore` 与 `Worker` 共享 `test_mode: Arc<AtomicBool>`，`pub fn set_test_mode(&self, on: bool)`；
  成功路径 `let id = if privacy || test { "" } else { record_success(..) }` 且跳过 `save_pending_result`；
  失败路径与隐私模式同分支（只留内存、不写 wav、不记历史）。
  ABI **v1.11**：`int32_t eryu_set_test_mode(EryuCore*, int32_t on)`（0/-1）。
- Swift：拿到 `Core()` 后按命令行开关批量设：`--self-test / --decode-wav / --soak / --bench / --mic-test /
  --segment-test / --verify-test / --type-event-test / --double-tap-test / --single-tap-test / --edit-observe-test`。
- 验证：`history.json` 条目 200 →（跑 self-test + decode-wav）→ 200；`~/.eryu/pending/` 为空。
  冒烟新增第 2c 项断言「跑 self-test 前后历史条目数不变」。

## D-056 长跑基线取「第一轮之后」，阈值仍是 120MB

- 起因：接入标点模型后 `--soak 6` 报 `RSS 569→724MB（增长 155MB）` 假失败——65MB 模型 + ORT 运行时是**第一轮**
  懒加载的一次性开销，不是泄漏。
- 做法：第一次 `state:idle` 之后再取 `startRSS`；增长阈值保持 `<120MB`（仍是「有没有一路涨」的判据）。
- 实测：`SOAK OK 次数=6/6 失败=0 RSS 796→798MB（增长 2MB）`。

## D-058 模拟麦克风 + `--simulate`：不用授权、不碰真麦克风也能验「按住 → 看浮层流式 → 松手 → 上屏」

- 痛点：浮层是否真的出现、准流式是否真的在滚字、松手是否真的出字——这些以前只能靠用户手按热键（要 TCC），
  自动化测不到；而 `--decode-wav` 只走解码段，既没有按键也没有浮层。
- 做法：
  1. core：`start_session` 若发现环境变量 `ERYU_MIC_WAV`，把该 16k WAV 读进内存，`tick()` 里按**真实时间**
     把样本喂进录音缓冲（等价于麦克风在说话），播完自动「松手」；`meter` 事件照常发（浮层电平条会动）。
  2. 壳：`--simulate <wav>` 注册右 Option（单键 CGEventTap）→ 合成按下 → 等 WAV 时长 → 合成松开；
     事件**全部交给正常 handler**（浮层/菜单/状态照真实路径更新），但注入 `dryRun`，绝不碰前台应用。
- 验证（同一段 TTS）：
  - 5.5s 素材：`SIMPARTIAL` 12 条渐进文本 → `SIMRESULT 385ms` → `SIMULATE OK`；
  - 8.5s 素材：`SIMRESULT 648 / 664 / 764ms`（三次），预览逐字增长到整句；
  - 截图 `/tmp/sim-shot.png` 里能看到胶囊浮层 + 电平条 + 「在听… 松开上屏」+ 已识别片段。
- 进了冒烟（第 3d 项）：要求至少一条 `SIMPARTIAL` 且 `SIMULATE OK`；没有流式则降为 warn（有的机器合成事件可能被拦）。
- 意义：这是目前**唯一**不依赖用户 TCC 就能验证「体验链路」的手段；真实麦克风/真实按键仍需用户点授权。

## D-059 长听写用「渐进封口」而不是松手后整段解码

- 问题：SenseVoice RTF≈0.08（2 线程），19s 素材松手后要 2.0s、135s 要 7.7s，直接违反「松手→出字 p95 ≤1.5s」。
  受模型限制，靠换引擎解决不了（都是 ≤30s 段模型）。
- 做法（`core/src/lib.rs` 的 `tick_partial` / `on_partial` / `begin_decode`）：
  1. 录音中每攒满 **10s**，在 `end-2.5s…end-0.8s` 里找能量低于块均值 25% 的 20ms 帧当切点（`audio::quiet_cut`）；
     找不到就继续等，攒到 20s 才硬切（连续不停顿也不至于尾巴无限长）。
  2. 找到切点 → 把 `recorded[prog_pos..cut]` 丢给后台线程解码，结果**封口提交**：`prog_text += text`、`prog_pos = cut`。
  3. 不够 10s 时同一段音频只做**预览**（`prog_text + 尾巴`），不推进 `prog_pos`——否则尾巴会变，已提交的文本就错了。
  4. 松手：置取消位、等 ≤200ms 让在途任务退出，然后**只解 `recorded[prog_pos..]`**，与 `prog_text` 用
     `pipeline::join_overlap` 拼（只在末尾 12 字内做严格重合去重；歧义保留两侧，绝不删真重复）。
- 实测（`--simulate`，同一 TTS 语料）：5.5s→415ms、8.5s→约 1.0s、19.3s→912ms、74.8s→0.6–1.1s、103s→727ms；
  对照改造前 19.3s→2041ms、135s→7727ms。
- 取舍与教训：
  - **块不能太小**：2s 一块时长句质量明显崩（丢上下文），接缝还会把词切两半；8s 一块实测接缝丢字（「会议室」→「会议识」）。10s + 安静缝是当前最优。
  - **不做音频重叠**：重叠 300ms 后用文本去重分不清「接缝重复」和「本来就说了两遍」（实测出现「投影设设备」「各各自」）。宁可偶尔丢一个字，也不制造重复字。
  - 代价：长听写时后台解码 CPU 占用更持续（预览 + 封口共用引擎锁，`--soak` 与 `--bench` 都在看有没有漂移）。

## D-060 引擎分工：SenseVoice 默认，Qwen3 只做短句精度档（25 条金标语料实测）

- **测法**：25 条 `say` 合成语料（中文 10、英文 10、中英混 5，16k mono），走与线上完全相同的 VAD + 引擎 + 拼接收缩流水线
  （`core/examples/recognize_wav.rs`，并修掉它之前在 16k 上跑降噪、和线上 48k 采集侧降噪不一致的问题）。
  CER 按去标点/空格后字符级编辑距离算。
- **结果**：

  | 引擎 | 中文 CER | 英文 CER | 中英混 CER | 加载 | 75s 音频解码 | 峰值 RSS |
  |---|---|---|---|---|---|---|
  | SenseVoice int8 | 0.029 | 0.124 | 0.152 | 0.36–0.42s | 2.4s（RTF 0.03） | 569MB |
  | Qwen3-0.6B int8 | 0.016 | **0.035** | **0.094** | 1.3s | 27.3s（RTF 0.36） | **2.24GB** |

- **Qwen3 的坑在长音频**：30s 段内会退化成重复（实测「另外，请另外，请大家提前…」并丢句），
  与 30s 硬上限（`context_len` 512）叠加后不适合长听写；短句（≤8s）它确实明显更准，尤其英文（3.5×）。
- **决定**：默认 SenseVoice；Qwen3 保留为「短句精度档」，设置页写清代价；块长按引擎自适应
  （`chunk_secs = 4` for qwen3，否则 10），让短句尾巴也在 1.5s 内。
- **Qwen3 跳过标点模型**：它自带标点，再喂 ct-punc 会在词中间插句号（「周三。三下午」）并叠出「。。」。
- 顺带修：`--decode-wav` 的超时原来固定 25s，Qwen3 长音频会假超时 → 改成 `max(25s, 时长×1.2+20s)`。
- 结论对用户的说法：**要「最准」用 Qwen3 + 短句；要「一直顺」用 SenseVoice**，不做按语言自动切换（识别语言本身不可靠）。

## D-061 睡眠/切用户要把录音收尾，唤醒要重挂热键

- 场景：用户按住右 Option 说话时合盖/睡眠/切换用户 → key-up 事件丢失，会话停在 Listening 直到 120s 上限，
  醒来后还以为在录；Carbon 热键与 CGEventTap 在睡眠后也可能已失效（I-038）。
- 做法：壳订阅 `NSWorkspace.willSleepNotification` / `sessionDidResignActiveNotification`：
  若当前阶段是 `starting|listening` → `core.cancel()` 并提示「系统睡眠，已取消本次录音（已保留录到的部分）」。
  `didWakeNotification` → 按当前配置重新 `applyHotkey`，并刷新设备与配置（模型/设备可能变了）。
- 不做的部分：不尝试在睡眠前把已录内容「提前上屏」——睡眠往往不给时间，注入时机不可控。
- 自测：`--sleep-test`（直接调处理器，不真睡眠）→ `SLEEPTEST OK 睡眠收尾=已取消 唤醒重挂=true`；冒烟第 3f 项守住。

## D-063 降噪从 RNNoise（48k）换成 GTCRN（16k 流式，535KB）

- 触发：D-035 的 RNNoise 档只在原生 48kHz 流上有效，16k 上采样路径会把字听错；而真实麦克风 44.1k/48k 混杂，
  且噪声下仍会丢句首。sherpa-onnx 2.0.1 自带 `OnlineSpeechDenoiser`（GTCRN/DPDFNet/VoxLock），其中
  `gtcrn_simple.onnx` 只有 535KB、原生 16kHz、流式（内部保留状态）。
- 实测（`core/examples/denoise_ab.rs` 加白噪声，再走产品流水线识别）：

  | 条件 | 不降噪 | GTCRN |
  |---|---|---|
  | 0dB 白噪声「今天下午三点在会议室评审设计文档，请把材料提前发到群里」 | `评审受聘文档请把材料提前发到群里`（丢半句） | **整句正确** |
  | -5dB 同上 | `天下午三点…`（丢字） | **整句正确** |
  | -10dB 同上 | 整句正确 | 整句正确 |

  速度：加载 40ms、RTF 0.027（5.5s 音频 179ms）；模型 535KB。
- 落地：`core/src/denoise.rs::Gtcrn`（`load`/`run`）+ `gtcrn_path(config_dir)`；`audio::build` 改为
  「混单声道 → 重采样 16k → GTCRN → 入队」（原来在 48k 上喂 RNNoise）；`models.rs` 加 `gtcrn` 条目（1MB，非压缩包），
  设置页给未装时的「下载 1MB」按钮；`--diagnose` 显示「降噪：开（GTCRN）」或「开（但模型没装，实际不降噪）」。
- 保留 `denoise::Denoiser`（RNNoise）给离线实验（`examples/denoise48.rs`），不再进产品链路。
- 假麦克风（`ERYU_MIC_WAV`）也走同一段降噪，否则 `--simulate` 测不到这条路径。

## D-064 松手时不要丢掉「正在算的那一块」

- 现象：`--simulate` 76s 长听写「松手→出字」抖动大（525ms / 1664ms / 2093ms），冒烟偶尔不达标。
- 原因：松手瞬间若有一块渐进解码在跑，旧逻辑立刻置 `partial_cancel` 并在 200ms 后自己从头解尾巴，
  把这一整块音频**又算了一遍**——最坏情况 ≈ 一块解码（~0.8s）+ 尾巴解码（~0.8s）。
- 做法：`begin_decode` 若发现 `partial_busy`，先不取消，记下 `waiting_commit`（最多 `COMMIT_WAIT_MS=900`）；
  正在跑的那一块把结果送回来时（`on_partial` 里 phase 已不是 Listening），**照样提交进前缀**再继续解尾巴；
  900ms 等不到才回退老路（取消 + 从头解）。
- 实测：同一 76s 语料 4 次 → **399 / 542 / 727 / 1028ms**（改前 525 / 1664 / 2093），冒烟 3e 一次 677ms。
- 注意：提交的是「已算完的音频区间」，不会重复上屏——最终文本仍由 `join_overlap(前缀, 尾巴)` 拼一次。

## D-065 音源锁定：不用注册声纹，锁住「这次录音里最先说话的人」

- 目标（DESIGN §9.7 的原创实验）：旁边有人插话时，只留主说话人；不像 `voicelock` 那样要先录 8 秒声纹。
- 规则：前 0.6s 建基线 → 第一段够长的语音（≥0.6s）算出 embedding 成为目标 → 之后每段比相似度（阈值复用 0.62）
  → 低于阈值丢掉 → **累计丢掉 ≥8s 且至少丢了 2 段**才解锁换目标。
- 为什么解锁条件是「≥8s 且 ≥2 段」而不是「3s 没有匹配」：一个 8s 的插话本身就是一段，
  按「3s 无匹配」会立刻解锁，等于永远滤不掉长插话（实测 v1 就是这样：`连续 8.0s 不像目标声纹，解锁并改用这一段`，
  三个段全留下）。改成累计+段数双条件后，插话段能被丢掉，同时「锁错人」的情况在第二段就会翻转。
- 只在 VAD 分段级别判断：两人同时说话（重叠）分开不了，记为 I-040。
- 实测（两个 TTS 声音交替 + 0.9s 静音拼接，25.7s）：
  - 不开音源锁定：3 句（A / B / A）；
  - 开：2 句（B 那段被丢），日志 `锁定第 1 段；留下 2 段、跳过 1 段`；
  - 探针 `sourcelock_probe a.wav b.wav ABABAB` → 锁定第 1 段、留下 3 段、跳过 3 段；
  - `ABBBBB` → 第 2 段丢掉，第 3 段触发解锁并改锁 B（`连续 4.0s 不像目标声纹`）。
- 落地：`core/src/sourcelock.rs`（`Lock::{new,check,summary}`、`Decision{Locked,Keep,Drop}`、
  `MIN_SEG_SECS=0.6`、`UNLOCK_AFTER=8s`、`UNLOCK_MIN_SEGMENTS=2`）；`Config.source_lock`（`[audio] source_lock`，默认关）；
  解码线程里 `pipeline::run_filtered` + `sourcelock::Lock` 闭包（与声纹门控互斥，后者优先）；设置页「音频 → 录音时」Toggle。
- 依赖声纹模型（39MB，`voicelock::model_path`）；没装则静默不启用（`source_lock_on` 要求模型存在）。

## D-066 准确性用「金标语料 + 产品链路」量，不用例子脚本

- 之前讨论 CER 时用的是 `recognize_wav` 例子，它跟产品链路的差异（示例默认不降噪、不补标点、不加热词）容易让人误读；
  而且散落在 `/tmp` 的脚本别人无法复现。
- 落地：
  - `bench/refs.json` 入库：25 条金标句（zh1–10 中文 / en1–10 英文 / mx1–5 中英混）。
  - `scripts/make-corpus.py`：用 `say` + `afconvert -d LEI16@16000 -c 1` 生成 16k 单声道语料（不进仓库）。
  - `scripts/bench.py`：逐条跑 `Eryu --decode-wav`（真产品链路：降噪、VAD、热词、标点），算 CER（去标点/空白、英文不分大小写）
    与延迟（引擎日志里的「解码 + 热词+标点」）。
- 基线（2026-11-19，本机 M5，25 条）：

  | 引擎 | zh CER | en CER | mx CER | 全部 | p50 | p95 |
  |---|---|---|---|---|---|---|
  | sense（默认） | 0.028 | 0.125 | 0.159 | 0.093 | 402ms | 629ms |
  | qwen3 | 0.016 | 0.035 | 0.098 | 0.040 | 1798ms | 2539ms |

  p95 都是短句（3–8s）；长听写的延迟另看 `--simulate`（76s → 600–1000ms）。
- 以后任何「更准了/更快了」的说法都用这套数字说话。

## D-067 小声说话：先自动增益，再按「包络」判断有没有人说话

- 起因：`--decode-wav` 用同一段语音降到 0.8% 音量（≈ −42dBFS 峰值，离得远/系统输入音量低就是这样）时，
  旧逻辑直接报 `no_audio: 麦克风没有收到声音`——用户明明在说话，却被告知麦克风坏了。
- 两处改动（`core/src/audio.rs`）：
  1. `auto_gain(&[f32]) -> Vec<f32>`：峰值 < 0.08 时整段放大到 0.8（最多 30 倍），正常音量不动。
     放在解码线程送 VAD/ASR 之前（`pcm_for_thread`），录到的原始数据不改（重试仍拿原声）。
  2. `looks_like_speech(&[f32]) -> bool`：20ms 一帧算 RMS，最响帧 ≥ 0.0005 且（≥ 底噪 3 倍 或 ≥ 0.05）。
     替换原来 `peak < SILENCE_PEAK(0.01)` 的绝对阈值——被静音的麦克风是「均匀小噪声」，最响帧和底噪差不多，
     仍然会被判成没说话；而轻声说话有语音包络，能被认出来。
- 实测：
  | 素材 | 旧逻辑 | 现在 |
  |---|---|---|
  | 正常音量 25 条金标 | CER 0.093 | CER 0.093（不变） |
  | 峰值 3%（−30dB）5 条 | CER 0.108 | 正常识别 |
  | 峰值 0.8%（−42dB）5 条 | `no_audio` 报错 | CER 0.080，全部识别 |
  | 均匀噪声 0.0005（模拟被静音） | 判为没声音 | 仍判为没声音 ✅ |
- 单测：`auto_gain_lifts_quiet_speech_but_keeps_loud_untouched`、`looks_like_speech_rejects_flat_noise`。
- 没做 AGC 的自动跟踪：一次录音内不做动态增益，避免把对方的插话也一起放大（音源锁定/声纹判断需要原始相对音量）。

## D-068 「请你自己验一下」要变成一条命令：`--acceptance`

- 问题：真实按键 + 真实麦克风只有用户能测，但「你按一下试试」这种请求没法回收结果——用户不知道看什么，我也拿不到数据。
- 做法：`Eryu --acceptance` 打印逐项检查，用户按住右 Option 说一句话即可，20 秒内没按也算一份报告：
  - 三项权限状态（名称 + 状态，不合并成「权限不足」）；
  - 是否接住按键（收到 `listening`）；
  - 浮层是否真的可见（`OverlayController.isVisible`，不是日志里写了就算）；
  - 流式片段数 + 最后一段文本；
  - 松手是否出结果 + 文本 + `latency_ms`（≤1500ms 判过）；
  - 结果写 `/tmp/eryu-acceptance.json`，退出码 0/1。
- **不注入**（`injector.dryRun = true`）：验收只回答「看得见什么」，不把钱素句子粘进用户正在写的文档（I-035 的教训）。
- 自测路径：`--acceptance --acceptance-synthetic <wav>` 用合成按键 + `ERYU_MIC_WAV` 跑一遍，本机实测六项全 ✅（延迟 673ms）。
- 顺带修：`--simulate` / `--acceptance` 之前不在 `testFlags` 里，跑一次就往历史里写一条；已加入 testMode（不写历史），并清理了 86 条测试记录（备份 `/tmp/eryu-history-before-clean2.json`）。

## D-069 GTCRN 降噪的边界：稳态噪声有效，竞争人声反而更差

- 问题：任何降噪都有人问「开了是不是一定更好」。用 5 条金标句做两组混音对比（`scripts/mix-bg.py` 可复现）：
  - 背景是**英语语音 −12dB**：CER 0.080（不降噪 0.093）→ 基本无影响；
  - 背景是**中文语音 −6dB**：CER **0.138**（不降噪 0.093）→ 明显变差；
  - 同一组中文 −6dB 混音再过 GTCRN：CER **0.384** → 更差。因为 GTCRN 把「像语音的干扰」也当噪声压，压的是目标信号本身。
- 对比稳态噪声（D-063 的 A/B）：0dB 白噪声下不降噪丢半句，GTCRN 整句正确 → 这类场景收益很大。
- 结论：默认保持开（办公室最常见的是空调/风扇/键盘），但设置页文案必须写清「旁边有人说话时关掉更准」，
  并把音源锁定作为那种场合的替代方案。不按场景自动切换——自动判断「干扰是不是人声」本身不可靠。
- 数据表（5 条金标，sense 引擎，产品链路）：

  | 场景 | 不降噪 CER | GTCRN 后 CER |
  |---|---|---|
  | 干净 | 0.093 | — |
  | 白噪声 0dB | 丢半句（无法对齐） | 整句正确 |
  | 英语背景 −12dB | 0.080 | — |
  | 中文背景 −6dB | 0.138 | **0.384** |

## D-070 把真实按键验收接进冒烟（合成按键版）

- `scripts/smoke.sh` 第 3g 项：`Eryu --acceptance --acceptance-synthetic <wav>`，要求输出 `ACCEPTANCE OK`，
  失败时把整段 ACCEPTANCE 明细贴出来（六项哪一项挂了直接可见）。
- 语料用 `say -v Tingting` 现场合成（≈8s 中文句），不依赖 `/tmp` 里的旧文件。
- 已知抖动：第一次接进冒烟时挂过一次——报告里设备名是真实麦克风，说明 `ERYU_MIC_WAV` 没生效（WAV 读不出来时会静默退回真实麦克风）。
  现在会在这种情况下打印 `ACCEPTANCE ⚠️ 模拟麦克风 WAV 读不出来或太短…`，不再静默。
- 实测：`延迟 ≤1500ms：279ms`（冒烟内），单独跑 461–673ms。

## D-071 交付形态：`scripts/dist.sh` 打 zip + 首次运行自动问模型

- **打包**：`scripts/dist.sh` = `build.sh` → `ditto` 到 `dist/Eryu.app` → `ditto -c -k --keepParent` 出
  `dist/Eryu-<版本>.zip`（8.6MB，签名与扩展属性都保留）。因为模型不进包（在 `~/.eryu/models`），
  安装包只有 app 本身；`codesign --verify --strict` 通过，`--self-test` 在 dist 副本里也正常。
- **首次运行问模型**：缺模型时弹一次「还需要下载语音模型」，列出缺哪几个、共多少 MB，两个按钮
  （现在下载 / 稍后去设置下载）。点「现在下载」逐个 `models_pull`，状态栏提示。
  - 判定来源分两处：引擎（sense/qwen3）看 `models` 清单的 `installed`；标点、降噪不是引擎，看
    config JSON 的 `punct_ready` / `denoise_ready`（本次为此新增 `denoise_ready` 字段）。
  - 模型清单是 core 异步回的，首次启动时可能还是空的 → 最多重问 4 次（每次 1 秒），否则弹窗永远不出现
    （第一次实现就踩了这个）。
- **实测**：把 `gtcrn_simple.onnx` 挪走 → 启动弹窗（截图核对）→ 点「现在下载」→ `MODELPROGRESS gtcrn 1/1 MB`
  → 文件回到 `~/.eryu/models/gtcrn_simple.onnx`（535638 字节）。
- **观测型 CLI 不参与单实例锁**：`--self-test`/`--acceptance`/`--simulate`/`--decode-wav`/`--diagnose` 等
  在一个正常实例运行时也要能跑；之前会打印「已有 Eryu 实例在运行，退出」，冒烟 3g 因此拿到空输出。

## D-072 自回声实测：会把「没说话」听成乱码，所以太轻一律不上屏

- 实验（本机，扬声器外放 TTS，麦克风在笔记本上）：
  - 安静房间只录底噪：peak 0.011 → 识别出「再来七个。」
  - 扬声器放「今天下午三点在会议室评审设计文档…」：peak 0.015 → 识别出「最个要去的地方，凶险的很进去了…」（乱码）
  - 正常近场说话（同一句）：peak 0.65 → 正确
  - 关掉降噪再录外放：仍然乱码（「要赶回来的远山哥…」）→ 不是降噪造成的，是信噪比和回声本身
- 关键结论：外放回声**不会**产生"重复文字"，而是让模型在低信噪比下**幻觉**出一句不存在的话；
  如果照常上屏，用户文档里会多出一串乱码。这比"没识别到"糟得多。
- 修法（`core/src/audio.rs` + `core/src/lib.rs`）：
  - `pub const SPEECH_MIN_PEAK: f32 = 0.02;`——低于 2% 峰值一律不当说话（实测底噪 1.1%、外放回声 1.5%、近场说话 10–65%）。
  - 错误分两档：`no_audio`（真没声音，归因到麦克风/设备）与 `too_quiet`（有声音但太轻，提示「靠近麦克风一点或调高输入音量；这次没有上屏」）。
  - 自动增益（D-067）只在这之上生效：0.02–0.08 之间抬到 0.8，不再把 0.01 的底噪放大 30 倍变成一句胡话。
- 冒烟里踩到的坑：3e 收尾 `rm -rf "$WAVDIR"` 之后，3g 还往那个目录写语料 → 空文件 → `--acceptance` 静默退回真实麦克风。
  现在 3g 用独立 `mktemp -d`，并在语料为空时直接判失败（不再靠"反正能跑"）。

## D-073 0.2.0 = M1–M4 的 macOS 交付线

- 版本号从 0.1.0（M1a 打通）升到 0.2.0：M1a/M1b/M1c 完成、M2 九项完成、M3 的 macOS 部分完成（Windows 见 `docs/WINDOWS.md`）、
  M4 的 macOS 部分完成（降噪/AGC/标点/profile/音源锁定/小声说话）。
- 交付物固定在 `docs/RELEASE.md`：产物路径、首次启动三步、验收命令、日常自测表、已知限制、卸载与升级。
  「已知限制」全部对应 ISSUES 条目，避免以后把"选择不做"当成 bug 重开。
- `CHANGELOG.md` 只写用户能感知的变化，不抄提交记录。

## D-074 录音中按 Esc 取消（只旁听，不拦截）

- 场景：按住说话了，发现说错/说漏，想放弃。原来只有菜单里的「取消当前录音」，手离不开键盘时等于没有。
- 做法：`starting|listening` 阶段临时装一个 `KeyTap`（keyCode 53 = Esc，`.listenOnly`），按下就 `core.cancel()`，
  浮层显示「已取消」，不发结果、不写历史、不落 pending；回到 idle 立刻卸掉监听（不在打字时抢 Esc）。
- 权限：与单键热键同源（辅助功能/输入监控），没有就只在日志里记一句「Esc 取消不可用」，不弹窗打扰。
- 浮层提示改成「松开上屏 · Esc 取消」，让这个能力可被发现。
- 自测：`ERYU_SIM_ESC=1 --simulate <wav>` → 合成按下 → 1.2s 后按 Esc → 断言 `阶段=idle 有结果=false`，
  输出 `ESCTEST OK`；冒烟第 3h 项守住。
- 注意：listenOnly 不拦截，Esc 本身仍会传给前台应用（比如正在编辑的文本视图）——这是可接受的，
  因为用户主动按 Esc 时本来就期望「放弃"。

## D-075 把「已知限制」变成可跑的数字（接缝丢字率 / 音源锁定命中率）

- 动机：I-036（接缝丢字）、I-040（音源锁定分不开重叠）都是"已知"，但没有数字就无法判断改动有没有让它变糟。
- `scripts/seam.py`：合成 8 句都含「会议室」的长音频 → `--simulate` → 数关键词出现次数。
  - 实测（本机）：倍速 1 → 7/8（88%）、倍速 4 → 8/8（100%）、倍速 8 → 4/8（50%）。
  - **结论：倍速 ≥8 是测试脚手架的失真**（喂样本太快，渐进封口线程来不及跑完，切块互相踩），
    所以默认倍速 4、测精度时用 1；这个数字只用来对比"同一倍速下前后两次"，不能跨倍速比较（I-047）。
- `scripts/sourcelock_metric.py`：A（Tingting，含「会议室」）与 B（Meijia，含「发票」）按 `ABAB` 拼接 →
  `--decode-wav`（临时开 `source_lock`，跑完恢复）→ 数 A 留了几次、B 漏了几次。
  - 实测：句间留 0.9s 静音 → **A 2/2 留下、B 0/2 漏过**（符合设计）。
  - 不留静音 → B 2/2 全漏：VAD 把两个人的话并成一段，embedding 是混合的，音源锁定按"算不出就放行"处理（I-040 的量化）。
- 这两个脚本都不进冒烟（要跑 TTS + 多轮解码，≈1–2 分钟），归到"发版前手跑一次"的清单里（`docs/RELEASE.md`）。

## D-076 壳契约 `--contract`：把「外壳该做什么」变成可执行检查

- 动机：Windows 壳要照着 `docs/WINDOWS.md` 重写，但"行为对齐"原本只是文字描述，容易各写各的。
- 做法：`--contract` 只碰 core，不碰麦克风/权限/注入：
  1. 读 `Core.abi`（打印十进制，`65547` = 1.11）；
  2. `core.setTestMode(true)` + `injector.dryRun`（不写历史、不上屏）；
  3. `core.debugSubmit("…")` 走完整链路；
  4. 校验每个事件都有 `kind` 且 `seq != 0`，出现 `state`（inserting/idle）、`result`、`submit_request`；
  5. `submit_request` 必须有 `task_id` 与 `mode`，壳按任务号回 `undelivered`；
  6. 8 秒内没齐 → `CONTRACT FAIL` + 缺哪项，退出码 1。
- 实测：`CONTRACT OK ABI=65547 事件=10 状态=true 结果=true 提交=true 问题=无`；冒烟第 2d 项守住。
- 注意：这个分支必须放在 `applicationDidFinishLaunching` 的**早退分支区**（原来误放进 `--acceptance` 函数体内，
  结果 `--contract` 单独跑时什么都不打印——这类"分支放错位置"是 CLI 自测最常见的事故）。
- Windows 壳开工第一天就跑这条：它不依赖任何 macOS 专有东西，能立刻暴露 ABI/字符串所有权/停机顺序的问题。

## D-077 `ERYU_CONFIG_DIR` 必须真正决定「从哪读」，并带上模型路径

- 踩到的 bug：`apply_env()` 在读完 `config.toml` **之后**才执行 → 设了 `ERYU_CONFIG_DIR=/tmp/x`，
  配置仍然从 `~/.eryu` 读，只有写回时才落到 `/tmp/x`。表现是「用临时目录测设备名，结果读的是真实配置」。
- 修法（`core/src/config.rs`）：
  1. `load()` 一开始就定目录：显式参数 > `ERYU_CONFIG_DIR` > `Config::default_dir()`；
  2. `relocate_models()` 把 sense / qwen3 / vad 三个路径一起搬到 `<dir>/models/`（除非显式设了 `ERYU_MODEL_DIR` 等）；
  3. `apply_env()` 只管引擎与显式路径覆盖；新增 `pub fn default_dir()` 供测试复用。
- 顺带验证了三条降级行为（`scripts/matrix.sh` 守住）：
  - 空目录无模型：`--diagnose` 报 `就绪=false`，`--self-test` 和 `--contract` 仍能跑（stub），且**不写历史**。
  - 设备名不存在：`mic_unavailable` + 列出可用设备 + 告诉用户去哪改。
  - `config.toml` 损坏 / 目录不可写：带 `上次加载提示：…` 继续跑，不崩、不静默。

## D-078 文档也要有检查：`scripts/doccheck.sh`

- 动机：文档分层（DESIGN 为 SSOT，docs/* 记录分类）最大的风险是"说法漂移"——代码改了、文档还停在上一版，
  而两者都自称权威。人工复查不可靠，所以把可机械验证的那几条做成脚本。
- 检查项（都可自动判定，不猜语义）：
  1. **ABI 版本**：`core/include/eryu_core.h` 里存在 `v1.11` 段且与 `core/src/lib.rs` 的 `ABI_MAJOR/MINOR` 一致。
  2. **版本号**：`packaging/Info.plist` 的 `CFBundleShortVersionString` == `CHANGELOG.md` 第一条。
  3. **错误码**：`core/src/lib.rs` 里 `emit_error("…")` 出现的每个码都必须在 `docs/ERRORS.md` 里有词条。
  4. **默认值口径**：默认热键 `ALT_R`（右 Option）在 config 与使用手册里一致；关键参数在 DECISIONS 有出处。
  5. **里程碑**：DESIGN 里 M3 仍标 🟡（Windows 未验证），且 `docs/WINDOWS.md`/`ISSUES.md` 有 I-028。
- 首跑就抓到两处真漂移：头文件版本段解析（不能直接 grep「ABI 1.x」）、以及文案表缺 `decode_busy`/`engine_ready`/
  `retry_unavailable`/`segment_spawn` 四个码——已补。
- 现在 `scripts/smoke.sh` 第 6/7 项分别跑矩阵与文档检查，冒烟从 14 项变 16 项。

## D-079 GTCRN 降噪的真实收益表（信噪比口径，离线降噪后解码）

- 之前 D-069 的结论是用"固定倍数混背景"得出的，**倍数不等于信噪比**，所以那张表不能横向比较（D-079 修正）。
- 新脚本 `scripts/denoise_table.py`：按**信噪比**混（噪声 RMS 归一到语音 RMS × 10^(ratio/20)）→ 先用
  `core/examples/denoise_file`（新增，任意 wav 过 GTCRN）离线降噪 → 再分别 `--decode-wav` 算 CER。
- 实测（zh1–zh5，5 条金标，本机）：

  | 噪声类型 | 噪声/语音 | 不降噪 CER | 降噪 CER | 结论 |
  |---|---|---|---|---|
  | 白噪声 | −6dB | 0.023 | 0.040 | 略差 |
  | 白噪声 | 0dB（等能量） | **0.236** | **0.154** | 明显变好 |
  | 白噪声 | +10dB（噪声 3×） | 0.455 | 0.718 | 明显变差 |
  | 背景人声 | −6dB | 0.100 | 0.124 | 略差 |
  | 背景人声 | 0dB | 0.586 | 0.600 | 持平 |
  | 背景人声 | +10dB | 1.009 | 1.009 | 都崩（分不开） |

- 结论：GTCRN 的价值区间很窄——**中等强度的稳态噪声（风扇/空调）有用，太轻时白做、太重时反而更糟，
  背景人声基本无用**。所以：
  1. 默认仍开（覆盖最常见的办公室/家里风扇场景，且最坏情况有「太轻不上屏」兜底）；
  2. 设置页文案改成范围描述，不写"总是更好"；
  3. `scripts/denoise_table.py` 发版前可复跑，数字变了就是模型或链路变了。
- 顺带发现：`--decode-wav` 不过降噪（降噪在采集侧），所以早期"A/B 直接解码"的对比其实没测到降噪——
  这也是为什么必须加 `denoise_file` 这个离线步骤。

## D-080 坏输入契约：可报错、可拒绝，但不许崩也不许静默改配置

- 新增 `core/tests/fuzz_ffi.rs`（6 个用例，跑在真实 C ABI 上）：
  1. **空指针**：`eryu_hotkey_down/up`、`eryu_cancel`、`eryu_set_event_cb` 一律 `-1`；`eryu_string_free(NULL)` 是安全 no-op。
  2. **坏 WAV**：不存在 / 不是 RIFF / 空路径 → `-3` 且 `err_out` 有可读原因（不是空串）。
  3. **坏 JSON**：`{`、`not json`、`[]`、非法引擎名、`min_record_ms:"abc"` —— 要么非 0 返回，要么带原因；
     **之后 `eryu_get_config_json` 必须与之前逐字节相同**（不许被坏输入改坏）。
  4. **超长 + 非法 UTF-8**：35 万个汉字进 `eryu_run_command` 不许崩；非法 UTF-8（`0xFF 0xFE`）进 `eryu_set_hotwords` → `-1` + 原因。
  5. **坏声纹输入**：残缺 WAV 注册 → `-3` + 原因；没模型时 `eryu_voice_state` 仍返回合法 JSON（`enrolled:false`、`model_ready:false`）。
  6. **释放边界**：只承诺「空指针 free 安全」，不承诺 double-free 安全（那是调用方 bug）。
- 测试之间共享 `ERYU_CONFIG_DIR` 环境变量，必须**串行**（`static Mutex<()>` 守卫），否则互相踩。
- 结论：6 个用例全绿，且 `cargo test` 从 62 → 68 项；冒烟第 1 项自动覆盖（它就是 `cargo test`）。

## D-081 崩溃恢复要能端到端验证，而不是"代码看起来对"

- 做法：`ERYU_KILL_ON_SUBMIT=1` 让壳在收到 `submit_request` 的**那一刻** `kill(getpid(), SIGKILL)`。
  选这个时刻有讲究：core 的顺序是 `result` → `save_pending_result` → `submit_request`，
  在 result 时杀太早（暂存还没写），在 submit 之后杀太晚（可能已确认上屏）。
- 发现的两个真问题：
  1. **自测模式默认不落暂存**（D-055），所以崩溃恢复测试拿不到文件 → 增加显式开关 `ERYU_KEEP_PENDING=1`，
     只在 `scripts/crash_recovery.sh` 里用；生产路径不受影响。
  2. **`--decode-wav` 在没模型时会静默用 stub**，输出「测试文本：你好，Eryu。」并报 `WAVTEST OK` ——
     假绿。现在加守卫：诊断里当前引擎 `就绪=false` 时直接失败，并提示下载模型；只想过流程必须显式
     `ERYU_ALLOW_STUB=1`。守卫只看「引擎：」那一行（别的引擎没装不该拦住当前引擎）。
- 结果（`scripts/crash_recovery.sh`）：exit 137、暂存 48 字节、重启取回逐字一致、取回后文件保留。
- 接入冒烟第 5b 项；测试用临时配置目录 + `ERYU_MODEL_DIR` 指向真实模型，不碰 `~/.eryu`。

## D-082 所有"要验识别质量"的自测命令都必须拒绝 stub

- 背景：stub 引擎会固定返回「测试文本：你好，Eryu。」。任何用临时配置目录跑的 `--decode-wav` / `--simulate` /
  `--mic-test` / `--soak` 都会静默走 stub，命令照样打印 OK——这会让识别相关实验全部失真。
- 做法：壳里加 `requireEngine(_:)`，读 `--diagnose` 的「引擎：」那一行，只看**当前引擎**的 `就绪=false`；
  命中就打印原因 + 修法（下载模型）+ 放行开关 `ERYU_ALLOW_STUB=1`，退出 1。
- 应用范围：`--decode-wav`、`--simulate`、`--mic-test`、`--soak`、`--segment-test`。
  刻意**不**拦 `--self-test` / `--contract`：它们验的是事件流与 ABI 契约，stub 正是它们要的环境。
- 例外：`scripts/matrix.sh` 第 2 项（设备名不存在）只验设备解析，显式 `ERYU_ALLOW_STUB=1`。
- 审计结论：现有脚本里只有 `crash_recovery.sh` 用临时配置目录，它已把 `ERYU_MODEL_DIR`/`ERYU_VAD_MODEL`
  指向真实模型；`bench.py`、`seam.py`、`denoise_table.py` 都用默认配置目录（真实模型），不受影响。

## D-083 两分钟上限的端到端判据

- 用 167s 的 TTS 语料（24 句，每句都带序号）走 `--simulate`，一次验三件事：
  1. 到 120s 必须发 `record_limit`（不是静默截断）；
  2. 松手→出字仍 ≤1500ms（这次 908ms，渐进封口已提交 453 字）；
  3. 文本必须从「第一句」开始、到第十八句左右结束——**超出上限的部分不许出现**。
- 顺带确认：`--simulate` 的 `ERYU_SIM_SPEED=6` 只加速喂样本，上限判断用样本秒数（`audio::secs_of`），
  所以加速不影响触发时机。
- 脚本 `scripts/longcap.sh` 接入冒烟第 5c 项；macOS 没有 `timeout` 命令（只有 coreutils 的 gtimeout），
  脚本里不能靠它兜底，要相信 `--simulate` 自己收尾（这条已经踩过两次）。

## D-084 连续说话（没有停顿）时的接缝实测

- 担心的事：渐进封口靠 `audio::quiet_cut` 找安静缝；如果用户一口气说 2 分钟没有停顿，就只能硬切
  （`chunk_secs * 2`），硬切点可能落在词中。
- 做法（`scripts/nopause.py`）：每条句子单独 TTS，再**贴在一起**（句间 0.02s ≈ 无缝），看结果里
  「第K句」标记是否齐全，并要求留下的标记是**连续前缀**（不能中间凭空少一句）。
- 实测（本机，SenseVoice）：
  | 语料 | 句间 | 标记命中 | 延迟 | 结论 |
  |---|---|---|---|---|
  | 65.5s / 12 句 | 0.02s | 12/12 | 929ms | 无丢失 |
  | 110.2s / 20 句 | 0.02s | 20/20 | 546ms | 无丢失 |
  | 121.8s / 20 句 | 0.6s | 20/20 | 706ms | 无丢失（对照） |
  | 166.9s / 30 句 | 0.02s | 22/30 | 913ms | 前 22 句连续，其余被 120s 上限正常截掉 |
- 结论：连续说话不会因为硬切丢句子级内容；延迟也没有因为找不到安静缝而变差。
  （句内个别字仍可能有 ASR 本身的错字，那是识别问题不是接缝问题，见 I-036。）
- 局限：这套指标是**句子级**的，测不出「词中少半个字」——I-051 记明。要测到字级需要逐字对齐的真值时间戳。
- 接入冒烟第 5d 项（16 句版本，约 90s 语料，`ERYU_SIM_SPEED=3`）。

## D-085 上屏失败必须"结果还在"，并且当场告诉你

- 发现的缺口：`.undelivered` / `.unknown` 时代码只给 core 报状态，**界面不提示、剪贴板也没有**——
  用户松手后什么都没发生，字就"没了"（虽然菜单里还能取回，但没有任何引导）。这正是「静默丢字」。
- 改法（`Injector` + `AppDelegate` 的注入回调）：
  1. `.undelivered(why)` → 报 status + **把文本写进剪贴板** + 状态栏「没上屏（原因）：已复制到剪贴板，也可从菜单取回」；
  2. `.unknown(why)` → 同上，文案「粘贴没确认成功」；
  3. core 仍保留 pending（D-014），所以运行期崩溃了也还能取回。
- 验证：`ERYU_FORCE_UNDELIVERED` 让注入器在第一步就返回失败，`--fallback-test <wav>` 走真实链路，
  断言「剪贴板包含结果」且「core 可取回结果」；`scripts/fallback.sh` 进冒烟第 5a 项。
- 剪贴板是全局资源：脚本先备份 `pbpaste`，测完还原，避免影响用户正在复制的东西。

## D-086 设置改完必须立刻生效（并顺手修了 F13–F20 无法配置）

- 用户视角最气的事：改完热键还以为生效了，实际旧的还在、新的没挂。用 `--settings-live-test` 守：
  1. 注册 `F13`，合成按下 → 计数涨；
  2. 换成 `ALT_R`，再按 `F13` → 计数**不许**再涨；
  3. 按 `ALT_R` → 计数涨（新键立刻生效）。
- 引擎切换：`eryu_set_engine("qwen3")` 后必须收到 `engine_ready`（core 走 `emit_error`，kind 是 `error` 不是 `notice`），
  并且 `--diagnose` 的「引擎：」行立刻变成 qwen3；测完切回 sense（`~/.eryu/config.toml` 的 `active` 也回到 sense）。
- **顺手修的真 bug**：`HotkeySpec.keyNames` 只有 F1–F12，用户把快捷键写成 F13/F14… 会得到「格式不对」。
  补了 F13–F20（`kVK_F13…kVK_F20`）。
- 自测分支早于正式热键创建点，必须自己 `Hotkey()` 并接 onDown/onUp，否则 `applyHotkey` 静默返回
  （这个坑在 `--simulate`/`--sleep-test` 已经踩过，这里是同一类）。
- 证据：`SETLIVE OK 热键切换=通过 引擎=引擎：qwen3… 问题=无`；`scripts/settings_live.sh` 进冒烟第 5e 项。

## D-087 隐私模式的第四条漏点：菜单「最近」

- `scripts/privacy_check.sh` 查了磁盘四处（history / stats / pending 文本 / pending 音频），都干净。
- 但读代码发现：`case "result"` 里 `status?.setRecent(event.string)` **无条件**执行——
  隐私模式下浮层已经脱敏、历史也不写，可菜单里「最近：……」还挂着正文预览。
  这属于同一承诺（不显示正文）的漏点，已修成隐私模式传空串。
- 顺带给 `StatusController` 加只读 `recentText`（自测用），新增 `--privacy-test <wav>`：
  跑完断言「菜单最近要么空、要么不含正文前缀」。脚本里再断言一次，防止以后被改回去。
- 证据：`PRIVACYTEST OK 最近菜单=（空）`；`scripts/privacy_check.sh` 七项 → 八项，进冒烟第 5f 项。

## D-088 权限不足时的降级要说清原因，并留退路

- 单键热键（右 Option / F13…）依赖 `CGEventTap`，没有「辅助功能」或「输入监控」时必然失败。
  失败**不许静默**、不许把应用带崩，必须：给中文原因 + 组合键仍可用 + 权限面板点名缺哪项。
- 测试手法：`ERYU_FORCE_TAP_FAIL` 让 `KeyTap.start()` 在第一步返回失败（模拟权限缺失），
  `--degrade-test` 断言三件事：单键失败带原因、随后注册 `CTRL+ALT+SPACE` 成功、`Permission.missingSummary()`
  的文案符合规范（缺项要点名）。
- 证据：`DEGRADETEST OK 单键原因=单键热键需要「辅助功能」或「输入监控」权限（模拟权限不足） 组合键=可用`；
  `scripts/degrade.sh` 进冒烟第 5g 项。
- 注意：这个分支同样早于正式热键创建点，必须自己 `Hotkey()`（第三次踩，已写进 D-086）。

## D-089 设备相关两条必须自动化验证

- **换设备即时生效**（`scripts/device_switch.sh`）：从 `--diagnose` 的「输入设备：」里取两台，
  分别写进临时配置跑 `--mic-test 1`，断言日志里的「采音开始 <设备名>」确实是这次配置的那台。
  实测 A=`MacBook Air麦克风`→`采音开始 MacBook Air麦克风 48000Hz 1ch`；B=`MiRemoteV 2ch`→`采音开始 MiRemoteV 2ch 48000Hz 2ch`。
- **录音中拔麦**（`scripts/mic_unplug.sh`）：`ERYU_FORCE_MIC_FAIL_MS` 在 `start_session` 时算好「到点就模拟中断」，
  `tick()` 命中后走与真实 `capture.has_failed()` **同一条** `mic_unplugged` 收尾路径（保住已录样本、立即解码）。
  壳侧 `ERYU_SIM_TOLERATE_ERRORS=1` 让 `--simulate` 不把这条预期错误当失败，继续等结果。
  实测：1.2s 中断 4.5s 语料 → `SIMRESULT 454ms 这段话说一半麦克风被把。`（中断前那半句保住，后半点没有）。
- 教训：新测试脚本用临时 `ERYU_CONFIG_DIR` 时**必须**同时给 `ERYU_MODEL_DIR`/`ERYU_VAD_MODEL`，
  否则会被引擎守卫拦下（`SIMULATE FAIL 引擎没装好…`）——这是 D-077 迁移后新增的必填项。

## D-090 发版闸门：产物不许在检查没跑的情况下打出来

- `scripts/release_gate.sh` 七步（FAST 模式）：`cargo test` / `doccheck` / `matrix` / `--acceptance-synthetic` /
  `fallback` / `privacy_check` / `degrade`；完整模式再加 `longcap` / `nopause` / `mic_unplug` / `device_switch`。
- `scripts/dist.sh` 默认先跑闸门（`ERYU_SKIP_GATE=1` 仅调试），闸门不过不打包。
- `docs/RELEASE-CHECKLIST.md` 写明机器能判定的（全部脚本化）与机器判定不了的（四项人工确认：
  真实授权 + 真实按键上屏、密码框不触发、暂停音乐）。
- 证据：`ERYU_GATE_FAST=1 ./scripts/release_gate.sh` → `发版闸门通过 ✅（7 项）`。

## D-091 四开关矩阵：声纹门控与音源锁定同时开时，门控优先

- 代码里两者本来就写成互斥优先（`voice_lock_on` 先判），但从没被验证过。`scripts/switch_matrix.sh` 现在固定四件事：
  1. **门控 + 源锁同时开**（放别人的声音）：门控生效、整段跳过（`声纹过滤掉 1/1 段`），不崩、不重复过滤；
  2. **只开源锁**（放自己的声音）：正常出文本（第一段成为目标音源，不被丢）；
  3. **标点开/关**：开 → 有逗号；关 → 句子仍在（只是不分句）；
  4. **诊断可读**：降噪/标点/声纹三项状态都出现在 `--diagnose` 里（含模型路径或「未装」）。
- 副作用修法：临时配置目录里必须把模型**链接**进去（sense/silero/gtcrn/3dspeaker/punct），
  因为 `gtcrn_path`/`punct::model_path`/`voicelock::model_path` 都按**配置目录**下的 `models/` 找，
  只有 `ERYU_MODEL_DIR` 是不够的（D-077 把两者分开后的必然结果）。
- 证据：`开关矩阵通过 ✅（8 项）`；进冒烟第 5j 项、发版闸门第 5 步。

## D-092 场景指引放在设置页，而不是只写手册

- 用户不会先读文档再开开关。四个开关（降噪/标点/声纹门控/音源锁定）的取舍都是「看你当时在什么环境」，
  所以把六条场景建议直接贴在「设置 → 音频」页顶部：办公室正常环境、旁边有人说话、外放音乐、多人会议、旁边人一直插话、密码框。
- 与 `docs/USAGE.md` 的场景表同口径，文档里显式写「改一处要同步另一处」。
- 证据：`docs/scenario-guide.png`（截图核对，六行图标 + 标题 + 「→」+ 建议）。

## D-093 局域网听写接口（M5 路线②）先做「最笨的一版」

- 目标不是做 API 服务，而是让**别的设备也能用这台机器的本地模型**：手机录一段 → POST 过来 → 拿回文字。
- 协议只有两个：`GET /health`（探活 + 引擎名 + VAD 是否就绪）、`POST /transcribe`（body = 16k 单声道 16-bit WAV → `{"ok":true,"text":…,"ms":…}`）。
- 默认 **关**；默认只监听 `127.0.0.1`；`allow_lan = true` 才监听 `0.0.0.0`，同时支持 `token`（请求头 `x-eryu-token` 或 `?token=`），token 不对一律 403。
- **不上屏、不写历史、不动剪贴板**：它只把音频变文字，怎么用是调用方的事。这样它不会和「按住说话」抢状态，也不会污染隐私模式。
- 复用同一个引擎实例（`Arc<Mutex<Box<dyn AsrEngine>>>`），不额外占 500MB；代价是和录音/预览共用一把锁，所以并发的第二个请求会排队。
- 证据：`scripts/lan.sh` → `health 探活 / 无 token 403 / 识别 472ms 文本正确 / 坏 WAV 报 bad_wav`，4 项通过；`core/tests/lan_smoke.rs` 两项（协议 + token）。
- 设置页「音频 → 局域网听写接口（实验）」可配开关/端口/allow_lan/token，改完重启生效（截图 `docs/lan-ui.png`）。
- **顺便给了一个网页版**（`GET /`）：手机/浏览器打开就是「录一段 → 转文字 → 复制」的页面，不需要装 App。
  页面把录音降到 16k、编码成 WAV 再 POST，也支持直接选已有的音频文件（浏览器会解码后重采样）。
  限制写在页面上：浏览器录音要 https 或 localhost，所以用 IP 打开时只能选文件——这是浏览器的规矩，不是我们没做。
  页面本身**不需要 token**（否则手机打开就被 403，也没地方填 token）；`/health` 与 `/transcribe` 仍然要 token。
- **安全默认**：`allow_lan = true` 但 token 为空时**拒绝启动**（`lan_no_token`），而不是默默把端口暴露给同网段；
  日志和状态栏都给出「设 token 或关掉允许局域网」两条出路。
- 还没做（记 I-055）：手机端 App、TLS、并发限流、按 token 计费/限速。

## D-094 手机端先走 `.local` + 快捷指令，不做自签 https

- 浏览器录音要 https 或 localhost；给局域网发自签证书，手机不装描述文件仍会被拒，装了又要用户手动信任 —— 成本高、收益低。
- 替代：①`.local`（Bonjour）让手机免记 IP，一条 URL 永远有效；②网页版保留「选文件」这条在任何环境都能用的路；
  ③真正想「按住就说」用 **iOS 快捷指令**（录音 → POST /transcribe），快捷指令不受浏览器 https 限制。
- 结论：不引入 TLS、不引入证书分发；`docs/LAN.md` 写清三条路与各自限制。

## D-095 应用图标：声波 + 文字光标

- 语义：左边四根薄荷色声波条（正在说）→ 右边两条文字行 + 琥珀色光标（已经变成文字），一眼能看出「语音输入」而不是「录音机」。
- 视觉：深靛→紫罗兰对角渐变底、右上青/洋红极光、内容留白 100/1024（符合 macOS 图标网格），圆角 186。
- 交付：`packaging/icon/Eryu-1024.png`（源图）→ `sips` 生成 16/32/128/256/512 @1x@2x → `iconutil -c icns` → `packaging/Eryu.icns`；`build.sh` 拷进 `Contents/Resources`，`Info.plist` 的 `CFBundleIconFile = Eryu.icns`；运行时再 `NSApp.applicationIconImage` 兜一次（Dock 里更稳）。
- 备选稿留在 `packaging/icon/`（v2 双元素重叠版、v1 纯波形版），最终选 v3。

## D-096 局域网接口：请求日志 + 并发上限

- **日志**：内存环 40 条（`lan::LOG`），每条只有 `at / src / path / code / ms / bytes`——**不记内容、不落盘**，
  与「不上屏、不写历史」同一口径。排查手机连不上时看状态码（403 token 不对 / 503 忙 / 400 WAV 不对）。
- **并发**：`MAX_IN_FLIGHT = 2`。引擎本来就串行，上限是为了让多余请求**立刻拿到 503 `busy`**，而不是排队到浏览器超时。
  超过上限时先把请求体读掉再回 503（直接关连接会让客户端拿到 RST，curl 显示 000）。
  实现坑：拒绝分支必须 `continue`，写成 `return` 会把**监听循环**一起退出（后续连接全挂死）。
- **入口**：ABI v1.12 `eryu_lan_log`（JSON）；`--lan-log` 打印当前进程的摘要；`--lan-serve` 退出时打印
  `LANLOG [...]`（摘要只在进程内存，别的进程读不到）；设置 → 诊断页列出最近 6 条。
- 证据：`scripts/lan.sh` 10 项（含「超上限返回 503 busy」「日志有状态码、没有内容」）。

## D-097 只有一个 Eryu：安装位置唯一 + 未知参数拒绝

- **问题**：项目里会同时存在 `build/Eryu.app`（开发构建）和 `dist/Eryu.app`（打包产物），
  两份二进制内容相同（同一 build、sha 一致），但**各自被打开过就会各自注册到 LaunchServices**，
  Launchpad / Spotlight 于是出现两个「Eryu」，用户无法判断哪个能用。
- **决定**：
  1. 唯一安装位置是 `/Applications/Eryu.app`；`scripts/install.sh` 负责装、并**注销 + 删除**项目内副本
     （`build/` 与 `dist/` 都不留 `.app`，需要时 `build.sh` 重新生成）。
  2. `dist/` 只保留 `Eryu-<版本>.zip`，不再长期保留解包后的 `.app`。
  3. 未知 `--flag` 一律 `exit 2` 并打印提示；`--version` / `--help` 正常输出。
     之前 `--version` 会掉进交互模式（真开麦克风、真注册热键），这是「看起来能用、其实在乱动」的典型。
- 证据：安装后 `lsregister -dump | grep Eryu.app` 只剩 `/Applications/Eryu.app`；`find` 全盘只有一个 `Eryu.app`。

## D-098 权限按「路径」记：只有一个安装位置，且每次启动自证

- **事实**（本次实测）：同一个自签 bundle 的两份副本，用 LaunchServices 启动后读到的权限不同——
  `/Applications/Eryu.app` 与 `build/Eryu.app` 的辅助功能/输入监控是**分别授权**的。
  麦克风按 bundle id 记，所以两份共用；辅助功能（AX）与输入监控（CGEventTap）在 TCC 里带路径语义。
- 因此「明明给了权限，每次还问」的真实原因通常是：**给的是另一份副本**（build/ 或 dist/ 或旧位置）。
  加上我们以前每次缺权限都弹窗，用户看到的现象就是「一直问」。
- 决定：
  1. 永远只有一个可运行副本：`/Applications/Eryu.app`（`install.sh` 负责装并删掉项目内副本）。
  2. 每次启动写 `/tmp/eryu-perm.json`：三项状态 + `ax_probe`（真读焦点控件）+ `event_tap`（真装监听）
     + `can_post_events`（合成事件预检）。授权了但读不到，就说明「给了没生效」或「给错了副本」。
  3. 同一组缺项 24 小时内只弹一次窗（`UserDefaults["prompted:<缺失集合>"]`），其余时候用状态栏提示。
  4. 菜单「权限状态…」随时可看、可复制，方便贴给我排查。
- 取舍：不做自动重授权（TCC 不允许程序自助），只把状态说清楚、把入口指对。
- 补充：**删掉旧副本后，系统里的授权记录不会自己消失**，于是新副本永远「被拒绝」。
  菜单「重新授权（清掉旧副本的授权）…」会调 `tccutil reset Accessibility/ListenEvent com.eryu.app`
  再重新请求一次；用户点一次允许即可。

## D-099 解码前补 300ms 静音 + 权限实时复查

- **尾字被吞**（实测）：合成验收里「请把材料提前发到群里」变成「…提前发到。」。原因不是模型，
  而是松手时音频硬切，VAD 拿不到「静音」这个结束信号，最后一段被截在缓冲区边界。
  做法：送 VAD/ASR 前把尾巴补 300ms 静音（`TARGET_RATE * 3 / 10`，全零）。代价可忽略（300ms 样本），
  收益是最后几个字稳定保住。
- **权限实时复查**：之前文案写「授权后重启一次」，是因为状态只在启动时读一次。现在每 2 秒读一次，
  补齐后自动重新挂热键 / Esc 监听，并提示「权限已生效」。这样用户不用在两个 App 之间来回重启。
- 阈值与语义不变：`SPEECH_MIN_PEAK`、`too_quiet` 判定都在补静音之前做，不会被补的零影响。

## D-100 浮层改毛玻璃 + 跟随光标 + 始终显示最新文本 + 显示当前润色档

用户反馈（m07940）：「四个角是灰色的，很难看，而且浮窗有点点大，也没有半透明」「识别的时候不会跟随进度，
最新的内容不在可视范围」「autoglm 呼出浮窗时会用不同图标显示当前润色设置」。

- **背景换 `NSVisualEffectView(.hudWindow, .behindWindow)`**：之前用 CALayer 填近似不透明色，圆角是图层内部圆角，
  窗口边缘仍露出灰底 → 视觉上「四角发灰」。毛玻璃材质由系统合成，圆角 + 边框 + 阴影都跟着材质走。
- **尺寸 420×74 → 368×66**，正文一行（13.5pt），`lineBreakMode = .byTruncatingHead`：
  文本长了从**头部**省略，末尾永远可见——这就是「跟随识别进度」的可见性保证。
- **位置跟着插入点**：优先 `AXFocusedUIElement` 的 `AXSelectedTextRange` → `AXBoundsForRange` 得到光标矩形，
  放它下方 10pt；读不到就用焦点控件底边中点；再不行才退回「光标所在屏 + 屏幕底部居中」。
  仍按屏夹紧，避免跑出可视区。
- **浮层显示当前润色档**：`profileProvider` 由壳注入（读 `model.profiles` 里 active 那档），
  状态行变成「在听… · 日常」，左侧用该档的 SF Symbol（message/envelope/note…）当图标——照抄 AutoGLM。
- 状态行右移 16pt 给图标留位；hint 仍是「松开上屏 · Esc 取消」。

## D-101 短录音不做渐进提交，只预览最近 5 秒

- 现象：6.4s 的句子，松手时总有一块预览在飞 → `begin_decode` 等它（最长 900ms）+ 尾巴重解 + 标点，
  实测 1.7–2.2s；而整段直接重解只要 0.67s + 0.12s。
- 做法：`prog_pos == 0 && 已录 < 12s` 时，预览只解**最近 5 秒**（`partial_up_to = 0`，不代表覆盖），松手时不再等它。
- 长录音（≥12s）保持原来的 10s 封口提交，那里等待是划算的（省掉几十秒尾巴）。
- 顺带：松手后迟到的预览块，只要 `up_to > prog_pos` 就采纳文本（原来只有 commit 块采纳），少解一遍。

## D-102 「润色开关」与「润色档」分工：力度只属于档，不再有全局力度

用户问（m08086）：「有润色配置了，还需要润色档吗？是不是重复了？」

答案：**是有一处真重复，但不是整体重复**。原来「润色」区选了力度（全局），「润色档」里每档也有力度，
两处都能设、谁也说不清谁生效。按职责切开：

| 层 | 管什么 | 在哪 |
|---|---|---|
| 管道（润色） | 开/关、服务地址、模型、API Key、超时 | 设置 → 润色 → 「启用润色」+ 服务商区 |
| 风格（润色档） | 力度（轻/中/强）、附加要求、快捷键槽、图标和名字 | 设置 → 润色 → 润色档列表 |

- 解码路径本来就按**当前生效档**的 `level` 走（`prof.level`），全局 `polish_level` 实际只当开关用 → 现在正式拆开：
  新增 `polish_enabled`（`[polish] enabled`），开关只管开关；力度只从档里读。
- **老配置迁移**：`[polish]` 里没有 `enabled` 时按 `level != "off"` 推断，保存后补上该键（单测 `polish_enabled_migrates_from_old_level`）。
  `polish_level` 字段保留，只用于迁移与显示，不再决定是否润色。
- 「测试连接」用当前档的力度；档力度是关闭时报「当前润色档的力度是『关闭』」。
- 开着开关但没填 Key 时，设置页给橙色提示，不在每次录音时打扰。

## D-103 浮层定稿：胶囊、宽度跟字走、提示在胶囊上方、档位图标在前

用户第 5 条反馈（m08167）逐条落地：

1. **不再写「在听…」**：胶囊里只有文本；录音时文本末尾跟一串跳动的点（`·` → `··` → `···`，0.36s 一跳）表示「还没说完」。
   状态词只保留在出错/隐私这种必须说话的地方。
2. **操作提示移到胶囊上方**：「松开上屏 · Esc 取消」是独立一行 10pt 灰字，贴在胶囊上沿 6pt 处，不占胶囊内部空间。
3. **宽度跟文字走**：胶囊宽度 = 文本实测宽度 + 左右留白，clamp 170…560pt；不再固定 368，也就没有「前后大片空白」。
   文本超宽时从**头部**省略（`fitHead`），末尾永远是最新内容。
4. **档位 logo 在胶囊最前面**：`[三根电平条] [当前档图标] [文本] [动态点]`；图标可在「润色档 → 编辑 → 图标」里逐档选
   （14 个候选：气泡/信封/便签/文档/铅笔/引用/书籍/闪电/魔法棒/地球/公文包/学术/笔记/闪光）。
5. **胶囊形状**：圆角 = 高度一半（40/2），深色毛玻璃（`hudWindow` + `.darkAqua`），白字——
   放在浅色输入框上也不会糊成一片（之前浅色材质叠在浅色背景上看着像没渲染）。

竖直位置仍跟插入点那一行（放不下翻到上方），水平始终居中。

## D-104 浮层修正：省略号只用一个、提示移到胶囊下方、宽度与位置不抖

用户 m08249 三点：
1. 「省略号本身就表示还在等录入，没必要单独加一个」→ 胶囊里只有 `文本 + …`，省略号 **每 0.5s 闪一次**（alpha 1 ↔ 0.25）；
   删掉上一版的跳动点 `·`、删掉占位 `…`。没有识别文本时不显示省略号，避免「空胶囊里孤零零一个点」。
2. 「提示显示在外面的下面，小字」→ 「松开上屏 · Esc 取消」放到胶囊**下方**（10pt 三级灰，居中），面板高度 40+4+12=56。
   如果光标在下半屏、下方塞不下（含提示行），整个浮层翻到光标上方，保证提示永远可见（之前提示会被状态栏糊住）。
3. 「避免胶囊大小的闪变、位置的闪变」：
   - 宽度**只增不减**，且按 24pt 台阶量化（190 → 214 → 238 …），一次录音里不会来回缩；
   - 改宽时保持**中心 x 与顶边不变**（origin.x 补偿），视觉上左右对称长，不跳；
   - 位置在 `starting`（新会话）时算一次并 `lockedOrigin`，整段录音期间不再重算——即使识别文本变化、光标移动也不动。

## D-105 胶囊宽度封顶 420pt + 底色再压深一档

用户 m08308：「宽度到一定程度后就不再增加，然后前面的内容往前推，始终要保证最近的内容和……能被看到」；
m08346：「胶囊颜色还可以再深20%」。

- 宽度上限 `maxWidth = 420`（约 24 个汉字）：到顶后不再变宽，改用 `fitHead` 把**头部**裁掉并加前缀「…」，
  末尾（最新识别内容 + 闪动省略号）永远在可视区内。实测 47 字输入：`bounds=420 avail=338.5`，
  显示为「…把上次那份预算表也一起发到」——正是「前面的往前推、最新的一直看得见」。
- 底色：`NSVisualEffectView(.hudWindow)` 之上再压一层 `NSVisualEffectView` 子视图 `tint`，
  `black × 0.28`（原来是 0.20，浅色壁纸上偏亮；0.45 实测太黑像块石头）。截图对照：原 #6a6a6a → 现 #5a5a5a。
- 提示行仍在胶囊下方小字；演示模式的时间轴拉长（长句 4.2s、上屏 6.0s），方便截图核对截断效果。

## D-106 胶囊宽度变化加动画；电平条改橙、档位图标放大

用户 m08511：
1. 「展开和收缩的过程中能不能有一个简单的动画过程」→ `resize(to:)` 里改宽不再瞬间 `setFrame`，
   而是 `NSAnimationContext（0.16s, ease-out）` 动画窗口 frame，宽度从旧值平滑长到新值；
   会话开始时（reset）仍直接到位，避免第一帧抖；同一次录音里宽度只增不减，所以只会「长出来」。
2. 「前面的润色图标有点太小，再前面的那个图标是干什么的，蓝色也不明显」：
   - 最左边四根竖条 = **实时麦克风电平**（说话时跳动，用来判断「麦克风有没有收到我的声音」），
     颜色从 `controlAccentColor`（蓝）改成 `systemOrange`，和系统麦克风指示的橙色呼应；
   - 中间的 SF Symbol = **当前润色档的图标**，尺寸 15 → 18、pointSize 12 → 14、白色透明度 0.85 → 0.95。

## D-107 胶囊改实色（弃用毛玻璃）+ 电平条浅橘黄加大

用户连续三轮调色：`m08540`「橙色的太深了，而且还很小…应该是浅橘黄，然后大一些、明显一些，特别是高度」、
`m08589`「颜色还是深了，再浅，然后把胶囊加深10%」。

- **弃用 `NSVisualEffectView`**（这是本条的真正结论）：它的观感跟着桌面壁纸走，同一个 `alpha` 在浅色壁纸上偏亮；
  想压暗只能叠子视图，而子视图会把电平条/文字一起压暗（实测橙色条 #FFD180 被压成 #988058）；
  给 `layer.backgroundColor` 赋值又会把材料整个顶掉（变 #A0A0A0 浅灰）。
  → 改成普通 `NSView` + `layer.backgroundColor = NSColor(srgb 0.22,0.22,0.24, alpha 0.94)`，
  颜色可预期（实测落色 #404048），内容不受影响。
- 电平条：色值 `srgb(1.0, 0.82, 0.50)`（实测 #F8D088 浅橘黄），宽 2.5→3.5pt、圆角 1.75、
  静音时也有 14pt（不再塌成 6pt 小点）、说话时最高 24pt（原来 14pt）。
- 档位图标 15→18pt、符号字号 12→14、白色 0.85→0.95。
- 宽度变化加 0.16s ease-out 动画（D-106），改宽时中心不动。

## D-108 Windows 版按「功能一比一」立计划（不是先堆半成品）

- 用户要求：Windows 版与 macOS 能力/细节一比一，但承认键盘与权限机制不同。
- 判定标准写成四层：功能（R 全表）、交互细节（按住语义/浮层时序/文案）、失败可见（24 个错误码）、
  自动化（同一批 CLI + 闸门脚本）。四层都过才叫「一比一」，不是「能编译」。
- 关键取舍（详见 `docs/WINDOWS.md`）：
  - 热键主路径 **Raw Input**（能拿 keyUp、能分左右修饰键、不吞键，与 macOS listenOnly 语义一致），
    `WH_KEYBOARD_LL` 只作兜底；`RegisterHotKey` 因没有 keyUp 不能做「按住说话」。
  - 壳用 **WinUI 3 + C#**（与 SwiftUI 心智最接近、中文排版与 DPI 达标）；否决 egui（质感不够）与 WebView（与定位冲突）。
  - 没有 TCC，但有五道门：麦克风隐私开关、UIPI、Secure Desktop、SmartScreen、防火墙；
    其中 UIPI 是硬边界 → 判完整性级别后降级「已复制请手动粘贴」，绝不静默丢字。
  - core 加 `cdylib`，`%APPDATA%\Eryu`，敏感文件用 DPAPI 加密（Windows 没有 0600）。
- 里程碑 W0（core 可移植+CI）→W1（最小可用链路）→W2（功能对齐）→W3（Windows 特有边界）→W4（闸门与打包）；
  **必须有一台 Windows 机器才开工**（I-028）。

## D-109 延迟必须可拆解：result 带 wait_ms / engine_wait_ms

- 只报「松手→出字 Nms」无法区分「模型慢」「等预览」「等锁」「CPU 抢占」，导致 I-059 这种空洞只能猜。
- 现在 `result` payload 增加两个字段：
  - `wait_ms`：松手（进入 Processing）→ 真正开始解码之间，等渐进块提交的时间（`COMMIT_WAIT_MS=900` 上限）；
  - `engine_wait_ms`：开始解码前等预览线程放开引擎锁的时间（上限 200ms，`partial_cancel` 置位后它会在段间退出）。
- 验收报告（`/tmp/eryu-acceptance.json`）与 `--simulate` 输出都带上它们；`--acceptance-synthetic` 的延迟结论会写成
  「2125ms（其中等预览块提交 0ms、等引擎锁 200ms）」。
- 理由：延迟是产品核心指标，报告必须能自证瓶颈在哪，而不是给一个无法行动的总数（呼应 D-038）。

## D-110 引导探针文本只显示、不注入

- **决定**：第 3 步的识别结果只写进引导页右侧，不调用注入器；壳在 `submit_request` 分支先交给引导探针，探针认领后直接 `core.submit_result(ok)`。
- **为什么**：引导窗是前台窗，注入要么打空、要么打到用户背后正在用的应用（0.7 的 `--inject-test` 曾把测试文字粘进当时窗口，属同类事故）。这一页要证明的是「麦克风 → 识别 → 看见字」，不是「上屏到别的 App」；真实上屏由 0.7 已有的热键链路负责。
- **后果**：探针结果会进历史（真实录音，合理）；壳不写 pending/剪贴板、不触发编辑学习。判定函数独立成纯函数，`--onboarding-selftest` 可断言。
- **取舍**：曾考虑「引导时先切到 TextEdit 再注入」，被否——要求用户动手切应用，违背 60 秒闭环。

## D-111 迷你窗按钮点击开/停，不改全局热键语义

- **决定**：迷你窗大按钮点一下 `core.hotkeyDown()`、再点一下 `core.hotkeyUp()`，只作用于该按钮；全局热键仍是按住说话。「按住/点击」作为全局偏好是 R-110（0.8.3），本轮不做模式开关。
- **为什么**：R-103 的验收原文是「点击可开始/停止录音」；在按钮上实现切换即可满足，又不会提前把 R-110 的行为开关引入本轮。
- **后果**：按钮状态跟 `state` 事件走（listening → 停止图标；idle → 待机）；若用户在录音中按 Esc 取消，按钮也回到待机。窗口关闭写 `[ui] mini_window=false`，菜单栏「显示主窗」写回 true。

## D-112 引导 5 步与「明显声音」阈值

- **决定**：引导顺序 ①模型 ②麦克风 ③试说 ④上屏方式 ⑤润色；「按键」并入③。「明显声音」= `rms ≥ 0.01`，连续 3 秒全部低于它才提示；core 的 silent 档仍是 0.003（≈ -50 dBFS），0.01 ≈ -40 dBFS，用来区分「有环境底噪」和「真没说话/麦坏了」。
- **为什么**：试说必须先确认麦克风能收到声音，否则失败原因不可归因；换键与试说同页，用户改完键就地验证，少一步也少一次「改了键不知道是否生效」。
- **后果**：麦克风页基于 core 66ms 的 meter 事件计时，离开页面即 `meterStop`，不长期占用设备。`--onboarding-step` 的序号按这 5 步算（0…4）。

## D-113 模型分三档：随包 / 内置 / 用到再下（2026-11-21）

- **背景**：用户装完 Windows 版后反馈「搞了一堆下载的模型」「根本不能录音和生成，只有个测试信号在」「引导让我读一句话，可这时候根本没有模型」。实际原因不是录音坏了，而是安装包不含任何模型、引擎页却把 6 个模型全列出来，用户不知道要下哪个，引导又在没模型时硬要走「试说一句」。
- **决定**：
  1. **随包**：SenseVoice（233MB，默认识别引擎）打进 zip 的 `models/`，`install-win.ps1` 装到 `%APPDATA%\Eryu\models`（已存在不覆盖）。
  2. **内置**：VAD（0.6MB）与降噪（0.5MB）用 `include_bytes!` 编进 exe，首次运行解到数据目录——用户永远不需要知道这两个东西存在。
  3. **用到再下**：标点（65MB）/ 声纹（39MB）/ Qwen3（879MB）不摆在引擎列表里；打开对应开关时自动开始下载（托盘提示进度），引擎页只留「语音模型（二选一）」。Stub 只在自测里出现，UI 不列。
- **为什么**：装机后的第一分钟决定「这东西能不能用」。把必要依赖做进安装包、把可选项藏到使用它的开关后面，比在引导里教用户下载六个模型更符合「个人自用的输入工具」。
- **后果**：zip 从 9MB 变 ~174MB（含模型，压缩后）；`scripts/package-win.sh` 在没有本机模型时会退化为不含模型并提示。引导在第 0 步检查模型是否就绪：没就绪不许进「试说」，「下一步」灰显并说明；模型下载完成事件会立即解除灰显（`onboarding::refresh_asr`）。

## D-114 诊断包：一个 JSON、不含正文、默认传到模型那台服务器（2026-11-21）

- **背景**：Windows 上「不能用」的原因五花八门（模型残缺、麦克风打不开、注入被吞、配置读坏），用户在微信里描述不清楚，我也没有他的日志。
- **决定**：诊断页「导出诊断包」产出一个自包含 JSON（六段，见 DESIGN §12.8），字段只含**环境与错误码**，不含用户说过的文字；上传默认走 `https://officeace.harmonyos.fun/eryu-diag/upload`（与模型镜像同一台机器），地址可在 `[diag] upload_url` 里改，留空 = 不自动上传。
- **为什么不用 zip**：Windows 壳没有压缩依赖，加一个只为打包日志的 crate 不值得；JSON 能被 `Get-Content` 直接看，也能被服务端原样存。
- **代价**：日志尾部 200 行有上限，超长日志会被截断——够定位「最近这次为什么失败」。

## D-115 润色请求必须显式给足输出额度，并检查 finish_reason（2026-11-21）

现象：口述较长时最后一句经常被改残或整句消失。查 `core/src/polish.rs` 的请求体只有 `messages`/`temperature`/`stream`，**没有 `max_tokens`**；服务端按默认额度（常见 512 token）截断，截断落在句子中间，而 `suspicious()` 只按 40% 长度比兜底，半句仍可能上屏。

决定：请求显式带 `max_tokens = clamp(字数*3+256, 512, 8192)`；响应 `finish_reason == "length"` 一律判失败，回退原文并提示。理由：宁可这次不润色，也不能给用户半句话。

## D-117 临时品牌名：中文「字在」/ 英文 zzai（2026-11-22）

**决定**：对外名称改为「字在 AI 输入法」，英文统一 `zzai`；官网 `zzai.harmonyos.fun`（先临时用）。

**范围（这轮只做显示层）**：菜单栏/托盘/窗口标题/提示框/关于/安装包 README 里出现的名字改成「字在」；图标换成新设计（Z 字标）；下载页与下载文件名用 `zzai-*`。

**暂不改**：可执行文件名 `Eryu` / `Eryu.app`、bundle id `com.eryu.app`、安装目录 `%LOCALAPPDATA%\Programs\Eryu`、注册表 Run 键名、配置目录 `%APPDATA%\Eryu`。理由：这些是数据/升级/自启动的锚点，改名会让老用户的历史、配置、开机自启全部失效；等产品名定稿后单独做一次带迁移的改名（登记 R-139）。

**验收**：`osascript … name of windows` 返回「字在 设置」；菜单栏项目「字在 / 编辑」；mac 安装包资源 `Eryu.icns` 换新；Windows 托盘提示以「字在：」开头。

## D-116 热词只让用户写「想听到的词」（2026-11-21）

用户无法预知识别会错成什么字，要求他填「容易听错成什么」是把内部实现推给用户。决定：界面只收目标词，存成 `from == to` 的等价规则；`from→to` 的差异由历史学习与建议采纳自动产生。

## D-118 图标定稿：深蓝底 + 白色麦克风（含 Z）+ 冷色微光（2026-11-22）

- **选定**：`packaging/icons/gen/icon-03b.png`（Codex `image_gen` 生成，1024×1024）。
- **为什么**：麦克风轮廓第一眼就说「这是语音输入」，麦克风头部里的负形 Z 是品牌（zzai），深海军蓝到近黑的渐变 + 一圈冷色微光给「安静的高级」；相对深红原版（icon-03）更适合长时间待在 Dock/桌面里。第一轮一度选过 icon-13a（渐变 Z + 声波），用户最终改选这一版，13a 留在候选目录。
- **怎么落到两端**：`packaging/icons/from-gen.py <png>` 统一出 `packaging/Eryu.icns`（10 档尺寸）、`shell-win/assets/eryu.ico`（16–256）、`packaging/site/icon-{512,192,180}.png`（squircle 透明蒙版）。站点另外替换 `assets/img/{favicon-32,apple-touch-icon,logo}.png`。
- **exe 文件图标**：Windows 侧此前只在窗口/托盘用 `LoadImage` 加载 ico，exe 自身没有资源段（资源管理器里是默认图标）。新增 `shell-win/app.rc` + `shell-win/build.rs`，用 llvm-rc 编出 `.res` 再 `cargo:rustc-link-arg`，资源段含 `RT_ICON(3)`、`RT_GROUP_ICON(14)`、`RT_VERSION(16)`（见 I-176）。
- **未定**：品牌定稿后是否连 exe/安装目录一起改名仍是 R-139（0.9）。

## D-119 电平条用官网那套「灯条」视觉（2026-11-22）

- **定调**：胶囊里的音量指示不再是 5 根琥珀色实心竖条，改成 9 根细条、紫 `#7C6CFF` → 青 `#22D3EE` 横向渐变、圆头、各自带一点外发光；高度仍由真实 dB 驱动（-60dB→0，-20dB→1），再叠两个不同频率的正弦做呼吸感（官网 canvas 同款写法）。
- **为什么**：用户对着官网效果说「你还不如学官网这个版本」——官网那排灯条是品牌视觉的一部分，胶囊作为最高频出现的界面，应该跟它一致。
- **两端一致性**：mac `Overlay.swift`（CALayer 逐条上色 + shadow 做发光）与 Windows `overlay.rs`（像素缓冲里做渐变 + 半径 3px 的加色光晕 + 胶囊形裁剪）用同一组常量（9 根 / 2.5pt 宽 / 2pt 间距 / 4–20pt 高）。mac 侧已截图核对（`docs/overlay-mac-bars.png`）；Windows 侧同参数实现、编译通过，待下次 VM 在线时补截图。
