# Eryu — 本地语音输入工具 设计文档 v0.39

> 定位：**个人自用**的 macOS + Windows 本地语音输入工具，对标 VocoType / AutoGLM 桌面版 AutoTyper（侦察见 `artifacts/`、`DESIGN-recon-*.md`）。
> 核心承诺：语音数据不出本机；按住快捷键说话，松开文字上屏。
> 差异化：AutoGLM 听写全链路上云，Eryu 全链路本地。

## 版本演进（决策速查，细节见对应章节）

| 版本 | 变更 | 依据 |
|---|---|---|
| **v0.39** | **0.8.2 首次可用**：模型分三档（SenseVoice 随 zip、VAD/降噪编进 exe、标点/声纹/Qwen3 用到再下，D-113）；引擎页只列两个识别引擎；引导在模型未就绪时不放行「试说一句」；正文/说明字号上调（R-124/R-125） | 用户 m12352 五条反馈 / `docs/ISSUES.md` I-144 |
| **v0.38** | **0.8.1 首次体验闭环落地**：引导（mac 5 步：模型→麦克风自检→试说→上屏方式→润色；Windows 6 步多一页「热键」，试说在第 5 步）；试说页强制真实闭环（探针只显示不注入，D-110）；迷你主窗 170×230（点击开/停只作用于按钮，D-111）；新增 `[ui] mini_window` / `[ui] onboarding_probe_done` | 用户 m00001 / docs/BACKLOG-0.8.md R-101…103 |
| **v0.37** | **0.8 对标升级**：新增 §12（首次体验闭环 / 三层后处理 / 迷你主窗+悬浮球 / 多引擎），配套 `docs/BACKLOG-0.8.md`、`docs/GOVERNANCE.md`、`docs/KICKOFF-0.8.md` | `rea-targets/REA-分析报告.md`（VocoType 1.6.2 / AutoGLM 1.11.0）+ 会话 `session-46a54bdc` |
| v0.2 | Rust workspace、cpal 16k mono、trait AsrEngine、双上屏、热词、里程碑 | 初稿 |
| v0.5 | 终审四项：每轮只提交一次注入 / 剪贴板快照版本+任务号双校验 / 首版范围冻结 / Apple Speech 能力检查+权限矩阵 | `DESIGN-final-audit.md`（NO-GO→GO） |
| v0.6 | 用户六项：跨设备模型 / 模型热插拔 / 托盘隐藏 / 连按应急 / 历史 / 润色 profile / 降噪 | §9 |
| v0.7 | AutoGLM 体验吸收（失败态设计密度）、OpenLess 技术参考吸收 | §10、§10.5 |
| v0.8 | **模型定档（实测）**：SenseVoice 默认、Qwen3 降为短句高精度可选；**VAD + 长听写流水线进 M1**；统计+设置页进 M1；Windows 挪 M2 末/M3；keystroke 移出 M1；M1 拆 a/b/c；修掉四处参数不一致 | §11 实测、用户 m00819 裁决、`DESIGN-audit-deepseek.md` |
| v0.10 | **Codex 终审 v0.9 吸收**：C 头文件先行 + 字符串所有权 / ABI / 回调有序 / 停机屏障 / panic 停用实例；**注入改由壳串行执行**（core 只发提交请求、task_id 去重、unknown 不重发）；热键改 `RegisterEventHotKey` 独占；TCC 身份与原生链接列为构建门槛；长听写段上限与会话上限分离、接缝不靠最长公共子串；M1a 验收改为可测项；Qwen3 延至 M1b | `DESIGN-final-audit-v09.md` |
| v0.9 | **架构定案**：SwiftUI 薄壳 + Rust core（单进程、C ABI/JSON 事件流、热键归壳、core 顶层 catch_unwind）| §3、§3.1 |
| **v0.36b** | **0.5.0 发行**：局域网接口请求摘要（不记内容）+ 并发上限 2 → 503；`--lan-info` 三地址；闸门 FAST → 9.2MB zip | zip sha256 fec76e0e… | |
| **v0.36** | **局域网接口可见性**：请求摘要环（不记内容）+ 并发上限 2 回 503 `busy`；ABI v1.12 `eryu_lan_log`；诊断页/`--lan-log` 可查 | lan 10 项 | |
| **v0.35b** | **手机端可用**：`.local`（Bonjour）+ iOS 快捷指令；不做自签 https（D-094）；`--lan-info` 给三种地址 | lan 8 项 | |
| **v0.35** | **0.4.0 发行**：局域网听写接口 + 网页版 + 安全默认；`dist` 全链路（闸门 9 项 → 8.7MB zip → 解包验签/版本/无 `/Users` 路径/`--self-test`） | zip sha256 f88a95b7… | |
| **v0.34f** | **局域网安全默认**：`allow_lan` 但无 token → 拒绝启动并报 `lan_no_token`（不是默默暴露） | lan 7 项 | |
| **v0.34e** | **局域网接口网页版**：`GET /` 返回可用的手机页面（WebAudio 录音→16k WAV、选文件解码重采样、token、结果复制），浏览器限制（https/localhost 才能录音）写在页面上 | 截图 + lan 5 项 | |
| **v0.34d** | **局域网听写接口落地**（D-093）：M5 路线②的第一版——`GET /health` + `POST /transcribe`，默认关、只听 127.0.0.1、可选 token、复用同一引擎实例、不上屏不写历史；设置页音频分页可配置 | lan 4 项 + core 2 项测试 | |
| **v0.34c** | **场景指引进设置页**（D-092）：音频分页顶部「什么场景开什么」六条，与 `docs/USAGE.md` 同口径 | 截图核对 | |
| **v0.34b** | **四开关矩阵**（D-091）：门控优先于源锁（别人声音 1/1 段被跳过、不崩）、单独源锁正常出文本、标点开关只影响分句；临时配置目录须链接模型进 `models/` | 开关矩阵 8 项通过 | |
| **v0.34** | **0.3.0 发行**（D-090 收口）：`dist.sh` 全链路跑通（闸门 7 项 → 8.7MB zip → 解包验签/版本/无 `/Users` 路径/`--self-test`/诊断）；清理自测残留历史 3 条 | zip sha256 61e97043… | |
| **v0.33g** | **发版闸门**（D-090）：`release_gate.sh` 7/11 步全脚本化，`dist.sh` 默认先过闸门；修 `--acceptance-synthetic` 漏判导致挂死（I-054） | 闸门通过 7 项 | |
| **v0.33f** | **设备切换/断开**（D-089）：换设备即时生效（日志核对设备名）；`ERYU_FORCE_MIC_FAIL_MS` 模拟中途拔麦，走真实 `mic_unplugged` 收尾并保住已录内容 | 5h/5i 全绿 | |
| **v0.33e** | **权限降级**（D-088）：单键热键注册失败必须给中文原因 + 组合键可兜底 + 权限提示点名缺项；`ERYU_FORCE_TAP_FAIL` + `--degrade-test` | DEGRADETEST OK | |
| **v0.33d** | **隐私模式收口**（D-087）：菜单「最近」不再显示正文（原来无条件显示）；`--privacy-test` + `privacy_check.sh` 八项证据 | PRIVACYTEST OK | |
| **v0.33c** | **设置即时生效**（D-086）：换热键后旧键立刻失效、新键立刻可用；切引擎立刻生效并回 `engine_ready`；顺修 `HotkeySpec` 只认 F1–F12 导致 F13+ 无法配置的 bug | `SETLIVE OK` | |
| **v0.33b** | **上屏失败兜底**（D-085）：`.undelivered/.unknown` 时把结果写剪贴板 + 状态栏说明 + 菜单可取回（原来只报状态，界面无提示）；`ERYU_FORCE_UNDELIVERED` + `--fallback-test` 端到端验证 | FALLBACK OK | |
| **v0.33** | **连续说话接缝**（D-084）：无停顿贴拼语料实测 20/20 句齐全（546ms）、167s 无停顿只被上限截断且保持连续前缀；`scripts/nopause.py` 进冒烟第 5d 项 | 4 组语料 | |
| **v0.32** | **两分钟上限可验**（D-083）：167s 语料一次验「到点提示 + 只保留前 120s + 延迟 908ms」；`scripts/longcap.sh` 进冒烟第 5c 项 | 908ms / 18 句 | |
| **v0.31** | **自测不许假绿**（D-082）：`requireEngine` 统一拦住 simulate/mic-test/soak/segment-test/decode-wav 的 stub 回退；矩阵第 2 项显式放行；审计确认 bench/seam/denoise_table 都用真实模型目录 | 空目录被拦、放行可用 | |
| **v0.30** | **崩溃恢复可验证**（D-081）：`ERYU_KILL_ON_SUBMIT` 在提交确认前 SIGKILL；`ERYU_KEEP_PENDING` 让自测也落暂存；`--decode-wav` 在引擎未装时不再静默用 stub 出假文本。`scripts/crash_recovery.sh` 逐字取回，冒烟第 5b 项 | exit 137 / 48B / 取回一致 | |
| **v0.29** | **坏输入契约**（D-080）：`core/tests/fuzz_ffi.rs` 6 项——空指针一律 -1、坏 WAV -3 带原因、坏 JSON 不改配置、35 万汉字不崩、非法 UTF-8 被拒、无声纹时状态仍合法；测试串行化 `ERYU_CONFIG_DIR` | `cargo test` 68 项 | |
| **v0.28** | **降噪收益表**（D-079）：按信噪比混噪声 + 离线降噪后解码，得到 GTCRN 的真实区间（白噪声等能量 0.236→0.154 变好；噪声 3× 0.455→0.718 变差；背景人声持平）；新增 `core/examples/denoise_file.rs` 与 `scripts/denoise_table.py`；修正 D-069 的"倍数≠信噪比" | 6 组 × 5 条 CER | |
| **v0.27** | **文档一致性可检**（D-078）：`scripts/doccheck.sh` 检查 ABI 版本段、发布版本号、错误码全覆盖、默认值口径、里程碑状态；接入冒烟第 7 项（矩阵第 6 项）。首跑抓出 `decode_busy/engine_ready/retry_unavailable/segment_spawn` 四个缺码 | 冒烟 16 项全绿 | |
| **v0.26** | **极端状态矩阵**（D-077）：`ERYU_CONFIG_DIR` 改为「决定从哪读」并带动模型路径；新增 `scripts/matrix.sh`（缺模型/设备不存在/配置损坏/目录不可写）7 项全绿；降级行为写进 §5.0 旁注 | `scripts/matrix.sh` | |
| **v0.25b** | **阶段状态复核**（§5.0 新表）：M1a/M1b/M1c/M2/M4 ✅，M3 🟡（Windows 待机器），M5 ⬜；并记录「AEC / 繁简 = 有意不做」的出处。回归基准：CER 0.093、p95 497ms（与 D-066 基线一致） | `scripts/bench.py` 25 条 | |
| **v0.25** | **限制指标化**（D-075）：`scripts/seam.py` 量接缝丢字（1× 88%、4× 100%、8× 50%——8× 是脚手架失真，I-047）；`scripts/sourcelock_metric.py` 量音源锁定（有静音 2/2 留 0/2 漏；无静音 2/2 全漏，量化 I-040） | 两个脚本本机实测 | |
| **v0.24** | **录音中 Esc 取消**（D-074）：仅 starting/listening 期间旁听 Esc（listenOnly，不拦截），取消后不上屏/不写历史；浮层提示「松开上屏 · Esc 取消」。冒烟 3h 覆盖 | `ERYU_SIM_ESC=1 --simulate` | |
| **v0.23** | **自回声与「太轻不上屏」**（D-072）：实测外放 TTS 被微波拾到（峰值 1.5%）会幻觉出乱码句，安静房间底噪（1.1%）也会；新增 `SPEECH_MIN_PEAK=0.02` 与 `too_quiet` 错误档，自动增益只在 2%–8% 区间生效。AEC 不做（需 process tap/虚拟声卡，I-046） | 外放/安静房间/近场三组 `--mic-test` 与 `--decode-wav` | |
| **v0.22** | **`--acceptance` 真实按键验收**（D-068）+ **降噪边界实测**（D-069）：GTCRN 对稳态噪声（空调/风扇/白噪）有效，对竞争人声（旁边有人说话）更差（中文 −6dB 背景 CER 0.138 → 0.384），设置页文案写明「那种场合关掉或开音源锁定」 | `--acceptance`（合成按键六项全 ✅）、`scripts/mix-bg.py` + `scripts/bench.py` | |
| **v0.21** | **小声说话**（D-067）：送 VAD/ASR 前自动增益（峰值 <0.08 时抬到 0.8，最多 30 倍，原始数据不改）；「有没有人说话」改为语音包络判断（20ms 帧 RMS，最响帧 ≥0.0005 且 ≥底噪 3 倍），替换绝对静音阈值。0.8% 音量素材从「no_audio 报错」变为 CER 0.080；均匀噪声仍判静音（I-041） | `scripts/bench.py --dir /tmp/eryu-tiny`、`--decode-wav /tmp/muted.wav` | |
| **v0.20** | **音源锁定实验落地**（D-065）：不用注册声纹，锁住本次录音最先说话的人；解锁条件「累计 ≥8s 且 ≥2 段」（3s 单条件会把长插话全放过）。VAD 段级判断，重叠说话分不开（I-040） | `sourcelock_probe` A/B 交替、`--decode-wav` 混合素材 | |
| **v0.19** | **松手不再丢正在算的块**（D-064）：`begin_decode` 等 `COMMIT_WAIT_MS=900` 让在途块提交后再解尾巴；76s 长听写 4 次 399–1028ms（改前 525–2093ms） | `--simulate`、冒烟 3e |
| **v0.18** | **降噪定档 GTCRN**（D-063）：535KB、16k 流式、RTF 0.027；0dB 白噪声下整句救回（RNNoise 只救回一半）。采集链改为「重采样 16k → GTCRN → 入队」；`models.rs` 加 `gtcrn` 条目；`--diagnose` 能看出模型有没有装 | `denoise_ab`、`--simulate` |
| **v0.17** | **引擎分工定案**（D-060）：25 条金标语料 CER 实测（sense 中 0.029/英 0.124/混 0.152；qwen3 0.016/0.035/0.094），Qwen3 长音频 27.3s/2.24GB/接尾退化 → 默认 SenseVoice，Qwen3 短句档；块长按引擎自适应（4s/10s）；qwen3 跳过标点模型；`--decode-wav` 超时按音频长度缩放 | `--decode-wav`、`recognize_wav` |
| **v0.16** | **「只录我的声音」落地**（§9.7.1）：声纹门控 3D-Speaker eres2net（39.6MB，512 维）+ 阈值 0.62；算不出保留、跳过过多整段重跑；注册/删除在本机完成（D-051/D-052，ABI v1.9/v1.10） | `voiceprobe` 实测 0.92/0.51；§11 |

---

- **v0.12（2026-11-19）**：单键热键定案（任何单键都能按住说话，含右 Option / Fn / F13 / 裸键）；
  录音浮层定案（底部胶囊 + 电平 + 准流式文本，纯 AppKit 绘制，见 I-017）；润色落地（ABI v1.3 `eryu_polish_test`）；
  稳定性签名（自签名证书 `Eryu Dev`，TCC 跨重建保持，见 D-018）。

- **v0.13（2026-11-19，M2 收官）**：M2 九项全部落地——热词表（最长匹配 + 英文词边界，ABI v1.2）、
  润色分级（ABI v1.3 `eryu_polish_test`）、录音浮层（纯 AppKit）、准流式预览（900ms 整段重跑）、
  单键热键（默认右 Option）、隐私模式（不写历史/不落盘/浮层脱敏）、部分成功提示、
  选区指令管线（⌘E，5 条指令，AX 就地替换，ABI v1.4 `eryu_run_command`）、热词自动学习（编辑追踪 → 待采纳）。

- **v0.14（2026-11-19）**：应用内模型下载落地（DESIGN §9.2 第一版）：`core/src/models.rs` 内置清单 +
  系统 `curl` 断点续传 + `tar` 解包（I-024：GitHub 对 rustls 握手不友好，ureq 只留给 API），
  进度事件 `model_progress`，ABI v1.5 `eryu_models_pull`。实测 sense-voice 158MB 包 → 247MB 目录，35 秒。

- **v0.15（2026-11-19，M3/M4 推进）**：可用性一轮——浮层跟随光标所在屏（D-041）、`--baseline` 资源基线（D-042：
  冷启动 444–840ms / RSS 554–569MB）、连按应急取回上次结果（D-043）、浮层显示已录秒数（D-044）、
  `diagnose` 自证运行时（D-045：常驻内存 + 延迟分位）、句末标点兜底（D-046）、菜单栏图标随阶段变化（D-047）；
  另落地润色 profile + 档位快捷键槽（D-036/D-037）、延时可测（D-038）、core 便携化与 Windows 落地清单（D-039）、
  部分成功单段重试（D-040，ABI v1.7/v1.8）。**仍然只差用户点三项 TCC** 才能验收真实上屏。
  **M3 起步**：上屏方式可选（粘贴/逐字打字）+ 按应用覆盖（`[insert]`）+ 注入后校验（只读一次、不二次注入）。
  实测：32 单测 + 2 FFI 冒烟、`scripts/smoke.sh` 五项全绿；自测命令 `--verify-test` / `--type-event-test` /
  `--edit-observe-test` / `--selection-test` / `--hotkey-selftest` / `--demo-overlay` / `--demo-palette`。

## 1. 逆向侦察结论（设计依据）

| 维度 | 竞品做法 | Eryu 采纳/调整 |
|---|---|---|
| 外壳 | VocoType=Tauri 2（WebView）；AutoGLM=原生 Swift | **定案：SwiftUI 薄壳 + Rust 核心**（单进程、静态库 + C ABI，见 §3.1）；Windows M3 用 WinUI/C# 薄壳复用同一 core |
| ASR | VocoType=FunASR ONNX 全家桶；AutoGLM=云端 WS | 全本地 sherpa-onnx；**默认 SenseVoice int8，Qwen3 为短句高精度档**（§4.2 实测） |
| 模型分档 | VocoType：Paraformer 238MB / SenseVoice 242MB / Fun-ASR-Nano 891MB / Qwen3-1.7B 1517MB | 采纳「分档+按需下载」；我们默认 166MB 档，重档可选 |
| 长音频 | VocoType 打包 FSMN-VAD 切段；Qwen 官方 Toolkit 用 VAD 切 120s | 采纳：VAD 边录边解（§4.4，M1） |
| 热词 | Paraformer Contextual（付费）/ 云端热词 v2 | 免费方案：识别后 AC 自动机替换 +（M2）注入后编辑追踪自动学词 |
| 润色 | 多 Provider + 服务端提示词 | 单 OpenAI 兼容 Provider，可选；提示词规范采自 AutoGLM（§4.1） |
| 上屏 | AutoGLM=剪贴板→CGEvent→逐键三重回退；OpenLess=剪贴板+粘贴 | M1 只做 paste（含 change-count 校验）；keystroke 留 trait，M3 再补 |
| 录音 | 全局 hold-to-talk + VAD 断句 | 采纳 push-to-talk 为主 |

## 2. 产品范围

**不做**：账号/付费/许可、在线模型商店、音视频转写/SRT、**完整新手引导**（用户裁决：暂时不做）、自动更新、遥测、多语言 i18n。
**做**：菜单栏常驻（无 Dock 图标）、快捷键录音→上屏、中英混说、长听写（1–2 分钟）、VAD 断句、热词替换、可选 LLM 润色、**设置页 + 使用统计（M1）**、转写历史（本地环形）、TOML 配置。

## 3. 架构

```
┌──────────────────────────────────────────────────────┐
│ eryu-core（Rust staticlib，双平台，无 UI 依赖）          │
│  audio:  cpal 采集（16k/mono/f32，有状态重采样）          │
│          回调只做格式转换+入有界缓冲，禁推理/分配/IO       │
│  vad:    silero-vad 实时切段                            │
│  asr:    trait AsrEngine { transcribe(pcm,&sr)->String }│
│  longform: VAD 段 → 队列 → 逐段解码 → 缝接（§4.4）        │
│  hotword / polish / clip 快照判定 / 焦点复核             │
│  session: 状态机（§4.5）+ 任务号 + 提交请求（§3.1-8）     │
│  store:   config / history / stats / clips              │
│  ffi:     薄 C ABI（§3.1），事件以 JSON 串上抛            │
├──────────────────────────────────────────────────────┤
│ Eryu.app（SwiftUI 薄壳，macOS）                         │
│  主线程：NSStatusItem 菜单栏 + 设置页/统计页 + 权限引导    │
│  热键：RegisterEventHotKey（独占，pressed/released）      │
│  注入：NSPasteboard + AX 目标复核 + CGEvent Cmd+V         │
│  → 调 eryu_hotkey_down()/up()、回 submit_result           │
│  展示：订阅 core 事件流（状态/结果/错误码/进度）           │
└──────────────────────────────────────────────────────┘
```

### 3.1 进程内 FFI 契约（定案：单进程，不做 daemon）

**为什么是 SwiftUI 薄壳**（A 方案，否决 egui 与 Tauri）：菜单栏、设置、统计、权限弹窗、麦克风电平、通知全是系统原生能力，SwiftUI 做这些最省事、质感最接近 AutoGLM；包体几乎不涨（无 WebView、无自带中文字体）；代价是多一门语言 + 一层 FFI，被下面的契约限制在很小范围内。

**为什么不做 daemon**：M1 单进程最省事；`eryu_core` 对外只暴露 `EryuCore` 一个结构体 + 事件流，将来要拆 daemon（或复用给手机端局域网）是机械改动，不是重写。

**为什么热键不放 Rust**：`global-hotkey` 在 macOS 上必须主线程构造，否则 `register` 返回 Ok 但永不触发（已知坑），且不提供可靠的按住/松开语义。改由壳用 **`RegisterEventHotKey`（Carbon）** 注册「修饰键+普通键」的独占热键（pressed/released 语义 + 可拒绝保留键冲突），把 down/up 转成 core 事件；`NSEvent` 全局监听与 CGEventTap 作为备选（前者收不到本 App 自身事件且不能吞键，后者需处理超时禁用），详见 §6。

| C ABI（`extern "C"`） | 作用 |
|---|---|
| `eryu_core_new(config_dir) -> *mut EryuCore` / `eryu_core_free` | 生命周期；core 自己起工作线程 |
| `eryu_set_event_cb(core, cb, ctx)` | core → 壳：`{"type":"state\|result\|error\|progress\|stats",...}` JSON 串（回调在 core 线程，壳侧转主线程） |
| `eryu_hotkey_down() / eryu_hotkey_up()` | 壳 → core：按住说话开始/结束 |
| `eryu_copy_last()` / `eryu_retry_last()` / `eryu_cancel()` | 待复制取回、重试、取消 |
| `eryu_get_config() / eryu_set_config(json)` | 设置页读写（返回校验错误 JSON） |
| `eryu_get_stats() / eryu_get_history(limit)` | 统计页/历史 |
| `eryu_models_list() / eryu_models_pull(id)` | 模型管理（进度走事件流） |
| `eryu_diagnose() -> json` | 自检（麦克风/模型/权限/热键） |

**硬规则（Codex 终审 v0.9 吸收，编码前必须先成立）**：

1. **先冻结一份可编译的 C 头文件**（`eryu_core.h`，人工维护、随库提交）。所有函数第一参数是 opaque `EryuCore*`；命令一律「入队即返回」或同步返回错误码，不阻塞调用线程。
2. **字符串所有权**：入参 JSON 用「指针+长度」，只在调用期间有效；core→壳回调里的字符串同样只在回调期间有效，**壳必须先复制再转主线程**；查询类返回的字符串由 Rust 分配，**只允许 `eryu_string_free` 释放**。空指针/空串/非法 UTF-8 一律视为参数错误并返回错误码，不 UB。
3. **ABI 握手**：`eryu_abi_version()` 返回定宽整数（major.minor），壳在 `eryu_core_new` **之前**比对 major，不一致直接报错退出（静态链接也可能头文件与库不匹配）。
4. **回调必须有序且可停机**：单独一条串行派发队列；每条事件带 `schema/session/generation/seq`；`progress` 可合并，`result`/`error` **必须送达**。壳收到后先转 `MainActor`，再按 generation 判断是否过期丢弃。
5. **音频回调里禁止编码 JSON、禁止持 core 锁**（只做格式转换入有界环）；溢出必须打缺口标记，不许继续报"整段成功"。
6. **停机屏障**：关闭流程 = 拒绝新命令 → 撤销订阅 → 等在途回调结束 → 释放 ctx → 释放 core；已排进主队列的闭包只持有独立数据，不得再碰 core 指针。
7. **每个 C 入口、每个工作线程入口各自 `catch_unwind`**；panic 转 error 事件并**停用该实例**（不假装恢复）。abort/native 崩溃（ONNX Runtime 内部）无法转事件——壳侧要有"core 已死"的兜底文案与重启入口。
8. **谁提交**：core 产出文本后发 `submit_request{task_id,text,policy,target_app}`；**壳串行执行** 剪贴板+AX+CGEvent 并回 `submit_result{task_id,ok|undelivered|unknown}`；core 按 task_id 去重，**同一任务号只接受一次提交结果**，收到 `unknown` 一律进「待复制」，绝不重发。

**技术选型**（版本锚点 = OpenLess v2.0.2 生产验证组合）

- 语言：Rust core（`crate-type = ["staticlib"]`）+ Swift 壳（Xcode 工程，`cargo build --release` 后链接 `.a`）
- ASR：`sherpa-onnx` Rust binding **锁 2.0.1**（与本次实测的 Python 2.0.1 / 官方二进制同版本，含 qwen3 `max_total_len` 参数；OpenLess 用的 1.13.8 已落后一代）
- 音频：`cpal` 0.15.x（OpenLess vendored 0.15.3 打过补丁，遇采集问题抄其 `OPENLESS_PATCH.md` 思路）
- 剪贴板/注入：**由 Swift 壳执行**（`NSPasteboard` + AX + `CGEvent`，见 §3.1 交付契约）；core 只发「提交请求」。Windows M3 由壳用 `SendInput` 实现同一契约
- **数据目录统一**：macOS `~/Library/Application Support/eryu/`，Windows `%APPDATA%\eryu\`；内含 `config.toml` / `hotwords.toml` / `profiles.toml` / `models/` / `history.db` / `clips/`
- 模型：`Application Support/eryu/models/`（manifest + sha256 + 断点续传，§9.2）



## 4. 关键流程

### 4.1 Push-to-talk 上屏（主路径）

流水线次序固定：**采音 → 单声道/16k 重采样 →（VAD 切段）→ ASR → 热词替换 →（可选）润色 → 上屏**。

1. 按下快捷键 → 采集；**录音真正就绪后托盘才亮**（防"首字丢了"假象）
2. 松开 → 停止采集。状态机（§4.5）：忙时再按去重；录音上限 120s；**短于 `min_record_ms`（默认 400ms）**丢弃并提示（文案双分支：有声 vs 无声）；设备拔出/休眠 → 终止并说明原因；取消后的迟到结果按 session_id/generation 丢弃
3. 热词替换（AC 自动机，最长匹配 + 英文词边界）
4. （可选）润色：去填充词、保留个性标记、不回答转写中的问题、不加新信息、同语言、`<CONTEXT_INFORMATION>` 槽位；级别轻/深；超时 3s 回退未润色稿
5. 上屏（M1 仅 `paste`；**由壳执行**，core 只发提交请求）：
   - 提交前复核目标：按下时记录的 PID + 窗口 + 控件，与投递前再取一次比对；不一致 / 安全输入已开启 / 已知密码框 / 权限失效 → **不动剪贴板、不动键**，直接「待复制」
   - 快照 change-count（macOS `NSPasteboard.changeCount`；**快照须覆盖所有 item/type**；被拒、懒加载、超预算 → 待复制；快照时已被外部改动 → 本轮不注入）
   - 写剪贴板（记写后版本 + 任务号）→ CGEvent Cmd+V
   - **每轮最多提交一次**：`unknown` 一律停止，不自动二次注入，不做"失败重试一次"
   - 750ms 后复核「写后版本+任务号」：仍是本次写入才恢复；否则**放弃恢复并提示"听写占用过剪贴板"**（changeCount 只证明所有权变化，不证明目标已读取，所以不做更强承诺）
6. 状态三分：**已发送**（结果未知，不重试）/ **待复制**（未注入，结果留内存+历史）/ **识别失败**
7. **待复制要能立刻取回**：菜单栏显示具体原因 + 结果预览 + 「复制」按钮；按下才覆盖剪贴板，用户自己回目标粘贴；下一轮开始后仍可取回上一条（历史关闭则留内存）。通知与日志不显示原文。

### 4.2 引擎与模型（实测定档，见 §11）

| 引擎 | 模型包 | 体积（解包） | 峰值内存 | RTF | 定位 |
|---|---|---|---|---|---|
| `sensevoice`（**默认**） | `sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09` | 247MB | **635MB** | **0.014–0.025** | 中英粤日韩、自带标点/ITN、无 30s 硬限（>30s 质量衰减，靠 VAD） |
| `qwen3-asr`（精度档） | `sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25` | 972MB | **1619MB** | **0.16–0.25** | **单次 ≤30s**（context 512 tokens 硬限）；短句/专名更准；不用于长听写 |
| `firered2` | `sherpa-onnx-fire-red-asr2-zh_en-int8-2026-02-26` | 839MB | 待测 | 待测 | 备选对比 |
| `paraformer-zh` | 官方 int8 | ~238MB | 待测 | 待测 | 中文强、无英文混说 |
| `apple-speech` | 系统 Speech.framework | 0 | ~系统 | ~0.3s | ⚠️ 非保证可用：`supportsOnDeviceRecognition` 运行时检查 + `NSSpeechRecognitionUsageDescription`；不可用则明示缺模型 |
| `silero-vad` | `silero_vad.onnx` | 636KB | ~20MB | — | 长听写切段（M1） |

- 引擎抽象 = §3 trait；config `engine = "sensevoice"`；缺模型明示（不静默降级）
- **切换语义**：`eryu models pull <id>`（sha256 校验、临时解包后原子启用）；切换在下次录音生效，不阻塞当前任务；失败回退旧模型
- **常驻 vs 卸载**：默认常驻预加载（创建 recognizer ~0.4s sense / ~1.7s qwen3）；设置项「空闲 10 分钟卸载」（下次首按多等重建时间）
- **延迟基线**：11.4s 音频 sense 202ms / qwen3 2401ms；51s 音频 sense 1196ms（整段）/ 698ms（VAD 8 段）。M1 验收：**5s 短句热态「松键→上屏」p95 ≤ 1.5s**
- **线程数**：sense 1/2/4 线程无差异（0.018–0.020），8 线程反而变慢；qwen3 8 线程 RTF 0.372（退化）→ 默认 2–4 线程
- **准度对照（v0.17，25 条 TTS 金标语料，CER）**：sense 中文 0.029 / 英文 0.124 / 中英混 0.152；
  qwen3 中文 0.016 / 英文 0.035 / 中英混 0.094。qwen3 短句确实更准，但 75s 连续语音要解 27.3s、峰值 2.24GB，
  且 30s 段尾部会退化（重复/丢句）→ **分工定案**：默认 SenseVoice，Qwen3 只做短句精度档（D-060/I-037）；
  块长按引擎自适应（qwen3 4s、其它 10s）。**qwen3 跳过 ct-punc**（自带标点，再补会在词中插句号）。

### 4.3 热词
`hotwords.toml`：`"艾瑞" = "Eryu"`；前缀树替换 + 全词匹配，防误伤子串；英文按词边界。

### 4.4 长听写流水线（M1，解决「经常说 1–2 分钟」）

问题：所有离线 ASR（Qwen3/Whisper/SenseVoice/Paraformer）都是 **≤30s 段**模型，整段喂长音频会截断或自我重复。
解法：**边录边解**，而不是松键后一次性算。

1. 采集时下游跑 silero-vad：检出语音段 → 静止 `min_silence`（默认 400ms）封口 → 该段入解码队列
2. 解码线程逐段识别，结果带**段号 + 绝对样本区间**暂存
3. 松键/到限时 `flush`：停采 → flush 重采样器 → VAD 尾段 → 队列剩余，**只终结一次**；迟到 keyUp 无副作用
4. **段上限与会话上限分开**：`max_speech_duration`（送模型 ≤30s）与会话 120s 是两件事；连续说话时 400ms 静音可能不出现，故在段预算内**优先选低能量处切**（含重叠后的模型输入也须 ≤30s），最迟强切
5. 段间**重叠 300ms**（试验参数）+ 接缝处理：**不靠"最长公共子串"删字**（"好｜好"可能是真重复；ITN/标点/中英分词会让同一语音得到不同文本；全段匹配还可能删掉远离接缝的词）。做法=保留两侧、只对共享时间窗做单调对齐；无 token 时间证据时优先低能量切点，**歧义保留两侧**，用人工标注的跨界词/真重复/数字夹英文逐例调参
6. 某段失败 → 只丢该段（部分成功保留），**M1a 就有最小 UI**：显示"第 n 段待重试"，停止自动上屏，可复制当前稿；重试只替换该段并重拼
7. 超过 120s 强制封口切段（防无限录音）
8. 缓冲：120s×16k×4B ≈ **7.68MB** 只是裸 PCM，另需设备环 + 重采样 + VAD + 识别器驻留；会话缓冲预分配、队列只存样本区间

收益：长听写延迟被摊平；部分成功；失败可重试；不需要换更重的模型。

**延迟目标（口径写清）**：短句（≤10s，日常输入）松手→出字 **p95 ≤ 1500ms**（M1a 验收）；**长听写（60–120s）目标 ≤ 3500ms**——松手后要解的是尾巴（≤4s 提交块已挪前，见 D-101/D-071）、仍要排队等预览线程让出引擎锁，机器忙时实测 1.9–2.9s 波动。两者不能用一个阈值衡量（I-077）。

**实现状态（v0.17，D-059）**：录音期每攒满 **10s** 找一次安静缝（`audio::quiet_cut`：在 end-2.5s…end-0.8s 内找能量低于块均值 25% 的 20ms 帧），找到就当成「已说过去的一块」**封口提交**（`prog_pos` 前进，文本攒进 `prog_text`）；没找到就继续等，攒到 20s 才硬切。**预览与封口分离**：不够 10s 时同一段音频只做预览（`prog_text + 尾巴`），不推进 `prog_pos`；松手时取消在途任务、只解 `recorded[prog_pos..]`，与 `prog_text` 用 `pipeline::join_overlap` 拼接（只对末尾 12 字内的严格重合去重，避免误删真重复）。实测：19s 素材从 2041ms 降到 912ms，75s 从 7.7s 降到 0.6–1.1s，103s 727ms（冒烟第 3e 项守住 ≤1500ms）。

### 4.5 统一参数与状态机（消除此前四处不一致）

| 参数 | 值 | 用途 |
|---|---|---|
| `min_record_ms` | 400 | 短于此丢弃（文案双分支：有声/无声） |
| 录音上限 | 120s | 超时强制封口 |
| 双击判定 | <400ms | 取回上次结果到剪贴板（**不自动注入**） |
| `max_audio_seconds` | 30 | 单段上限（送模型前保证） |
| 剪贴板恢复延迟 | 750ms | 恢复前复核版本+任务号 |

**热键语义（v0.10 改：状态配对，取代首版多重计时仲裁）**：按下第一个 down 即**锁存主键**并忽略 autorepeat；判定释放只看主键 keyUp，**不要求当前修饰键掩码仍匹配**（用户常先松修饰键）；纯修饰键组合 M1 禁用；`Starting` 期收到 up → 挂起等就绪后立即停（`pending_stop`），不得要求用户再松一次。锁屏/休眠/监听失效 → 终止并保留可取回内容；超时后**先等释放再武装**。原来 250/450/150ms 三连防抖只作为参数化兜底留档，不进 M1 主路径。

状态机：`Idle → Starting → Listening → Processing → Inserting → Idle`；**Inserting 拒绝取消**（防半截文本）；录音中再按 = 继续录（不取消、不重开）。

### 4.5.1 热键形态（用户 m01947/m02028：不要只会组合键）

| 形态 | 例子 | 实现 | 说明 |
|---|---|---|---|
| 组合键 | `CTRL+ALT+SPACE` | Carbon `RegisterEventHotKey`（pressed/released 两种事件） | 不会和打字冲突；系统占用（⌘Space）注册失败会指名原因 |
| 单修饰键 | `ALT_R`（右 Option）、`FN` | `CGEventTap`（`.listenOnly`）听 `flagsChanged` | 推荐：不产生字符，永不和输入打架 |
| 单功能键 | `F13` | `CGEventTap` 听 `keyDown`/`keyUp` | 推荐：不产生字符 |
| 裸字符键 | `A`、`;`、`SPACE` | 同上 | 能用，但**只旁听不拦截**：按住它时会边录音边把字符打进当前应用；设置页会明确警告 |

规则：单 token 一律按「单键」处理（修饰键/功能键/字母/数字键码）；带 `+` 的按组合键；
单键热键需要「辅助功能」（`CGEvent.tapCreate` 失败会返回中文原因，不静默）；
长按的自动重复（`keyboardEventAutorepeat`）忽略；安全输入（密码框）期间系统不投递事件，属系统行为。

### 4.6 录音浮层与准流式预览（用户 m01947：按下去就该看得见、边说边出字）

- **浮层**：屏幕底部居中胶囊 420×74，`.borderless` + `.nonactivatingPanel` + `level = .statusBar`，
  `ignoresMouseEvents`，全空间可见（`.canJoinAllSpaces`），不抢焦点。
  内容 = 5 根电平条（跟随 `meter` 事件）+ 状态行（准备中/在听…/识别中…/上屏中…/已上屏/没上屏）+ 松开提示 + 最多两行识别文本。
- **绘制方式**：纯 AppKit（`CALayer` + `NSTextField` + 30Hz `Timer`）。**不要用 `NSHostingView`**：
  透明无边框面板里 SwiftUI 内容实测整块不合成（I-017）。
- **准流式（不是真流式）**：`PARTIAL_INTERVAL_MS = 900`，录音中每 900ms 把「已录到的音频」整段重跑
  VAD + 识别，只发 `partial_text` 事件（不上屏、不进历史、不落盘），松手后再走正式链路。
  代价 = 多跑几次短音频（SenseVoice RTF≈0.05，可接受）；好处 = 不需要流式模型、接缝逻辑与正式链路完全一致。
- 事件：`meter{rms,peak,silent,live}` ~15Hz；`partial_text{text,chars}` ~1.1Hz。

## 5. 里程碑

| 阶段 | 内容 | 验收 |
|---|---|---|
| **M1a** 核心链路（**前置门槛见下**） | macOS：组合键 hold-to-talk → sense 识别 → paste 上屏；菜单栏状态；TOML 配置；VAD 长听写流水线；段级部分成功最小 UI；单实例锁；权限矩阵 | 见下方「M1a 验收（可测）」 |

**M1a 编码前置门槛（按此顺序，未过不进业务代码）**：①固定 bundle ID + 签名身份 + 安装路径 + 能力声明，留存首个构建的 designated requirement ②Xcode clean build 链 arm64 staticlib，`otool -L` 无开发机私有路径，Finder 启动并解一条固定 WAV ③事件复制/顺序/退订/停机用模拟事件测过（含迟到回调与 worker panic）④已签名壳上热键按下/释放、同键冲突、TCC 拒绝与重建各测一遍 ⑤固定文字打通单次 paste、同 App 换框、Secure Event Input、菜单手动复制。

**M1a 验收（可测，替代不可测的口头表述）**：
- **短句**：固定 5s 素材、热态连续 30 次，从 `keyUp` 到测试控件**文本实际变化**的 p95 ≤ **1.5s**；同时记录漏贴 / 双贴 / 首尾字丢失 / 批间漂移（只算成功样本的 p95 会掩盖漏贴，故必须同时报失败率）
- **长听写**：60s 与 120s 各测「有停顿」「连续说话」「末字贴边」三种，到限后只有一个终态；金标核对段级结果，记录尾延迟与峰值内存
- **失败可见**：注入故障下成功—失败—成功序列正确；重试期间开新轮，旧结果不得进新目标
- **权限**：撤销/重授/被杀后重启，已持久化结果仍可复制；提交状态 unknown 只恢复草稿 |
| **M1b** 失败可见 + 设置页 + 统计（**Qwen3 精度档在此开放**） | 失败态全套（§10.1）、设置页（麦克风电平测试/模型管理/热词/权限矩阵/引擎切换）、统计页、转写历史 | 每个失败分支有名字+下一步动作；结果永不静默丢失；设置页保存失败保留旧配置 |
| **M1c** 权限兜底 | 权限丢失检测+指名提示、承诺式引导、`eryu diagnose`、单实例锁、崩溃 pending_result 落盘 | 撤销权限后能恢复；崩溃不丢结果 |
| M2 ✅ | 热词自动学习（注入后编辑追踪）、选区指令管线、静音其他应用、隐私模式、部分成功 UI、润色（分级+context 槽位）、keystroke 评估 | 热词可学习且可关；选区指令五态文案；润色不改变语义 |
| M3 🟡 | Windows 移植（SendInput/托盘/消息循环热键/`GetClipboardSequenceNumber`）+ keystroke 模式 | macOS 部分✅（keystroke/按应用覆盖/注入后校验）；Windows **未验证**——本机无 Windows 工具链，见 `docs/WINDOWS.md`（I-028） |
| M4 ✅ | 降噪分档（GTCRN 默认，§9.7）、AGC/VAD 阈值、AEC、润色 profile（§9.6）、音源锁定实验（§9.7 补充）、繁简 OpenCC | 噪音环境识别不被带偏（但竞争人声更差，D-069）；profile 切换即时生效。**AEC 明确不做**（I-046）；**繁简明确不做**（目标用户简体） |
| M5（远期） | 手机端（本地小模型 / 局域网调桌面服务）、鸿蒙 | 另行立项（未开工） |

### 5.0 阶段状态复核（v0.25，按代码与冒烟证据，不按叙事）

| 阶段 | 状态 | 证据 | 还差什么 |
|---|---|---|---|
| M1a | ✅ | `scripts/smoke.sh` 1–5 项、`--self-test`、`--decode-wav`、`--acceptance`（合成按键六项全 ✅） | **用户真机按一次右 Option**（TCC 只能本人点） |
| M1b | ✅ | 设置页六分页 + 统计卡（p50/p95）+ 历史，截图核对；`docs/ERRORS.md` 覆盖失败态 | — |
| M1c | ✅ | 权限矩阵、`--diagnose`、单实例锁、`pending_result` 落盘 + `--print-last`、崩溃恢复 | — |
| M2 | ✅ | 热词（表+自动学习+候选落盘）、选区改写、暂停音乐、隐私模式、部分成功重试、润色三档 profile | — |
| M3 | 🟡 | macOS：keystroke、按应用覆盖、注入后校验；Windows：设计文档 + 能力对照表 | 一台 Windows 机器（I-028） |
| M4 | ✅ | GTCRN 降噪、AGC、VAD 阈值、音源锁定（指标脚本）、太轻不上屏、标点、`SPEECH_MIN_PEAK` | AEC/繁简是有意不做（I-046） |
| M5 | ⬜ | — | 远期，另行立项 |

### 5.1 UI 范围（用户裁决 m00819：统计+设置页必须有）

- **M1 有设置页 + 统计页**（此前"无设置窗口"作废；落点=M1b，M1a 只有 TOML + 菜单栏）；菜单栏状态弹窗仍保留：最近转写预览、复制按钮、识别中/缺模型/缺权/焦点变化状态映射
- `design/settings.png` / `popover.png` / `hotwords.png` = 目标稿：popover 的「重新上屏」砍掉（焦点会变，风险高）；热词 CRUD=M2；引擎下拉=M1b（多引擎共存）
- **外壳已定案：SwiftUI 薄壳 + Rust core**（§3.1）。否决理由留档：egui 中文控件质感差一档、要自带字体；Tauri 多 ~100MB WebView 内存且与"体验优先"目标不符。Windows M3 用 WinUI/C# 薄壳接同一 core（C ABI 复用）。
- 验收含「保存失败/新键注册冲突时保留旧配置」与「弹窗即时改动 vs 窗口待保存的生效语义」

### 5.2 M1a 第一周执行顺序（Codex 终审建议，直接照做）

1. 固定签名身份、bundle ID、安装路径、能力声明，留存首个构建的 designated requirement
2. Xcode clean build 链 arm64 staticlib；验证 ABI、Finder 启动、SenseVoice 单条 WAV（用 `/tmp/eryu-spike/rustprobe` 既有代码打底）
3. 实现事件复制、顺序、退订、停机；用模拟事件覆盖迟到回调与 worker panic
4. 在已签名壳验证热键按下/释放、同键冲突、TCC 拒绝与重建；失败先解决热键后端再往下走
5. 固定文字打通单次 paste、同 App 换框、Secure Event Input、菜单手动复制
6. 接 cpal、就绪信号、SenseVoice，跑短句端到端并记录首字/可见文本/延迟
7. 接会话 PCM、VAD、限长、flush、段级重试；先用固定长音频核对接缝，再接实时麦克风
8. 目标 Air 上跑 30 轮短句 + 60s/120s 实录，记录目标控件内容、峰值、权限状态、失败样本，再判 M1a

## 6. 风险与未决

- sherpa-onnx 2.0.1 Rust binding 的 API 与实测 Python 有差异（结构体 vs 关键字参数），M1a 首次编译时对齐；qwen3 `max_total_len` 需确认可传
- Windows 全局快捷键与输入法焦点冲突：M3 验证
- 无风扇 Air 上持续推理会降频：M1a 记录 30 次连续录音的 p95 漂移
- 自建 CI 无 GPU/mic：ASR 单测用固定 wav + 金标文本；端到端靠手动清单
- **FFI 边界**：Rust panic 必须被 core 顶层 `catch_unwind` 拦住转 error 事件（否则整个菜单栏 App 崩）；`eryu_abi_version()` 握手；静态库链接需 Xcode 配置 `LIBRARY_SEARCH_PATHS` + 系统库（CoreAudio/AVFoundation/Accelerate）与 cpal 的框架依赖
- **热键后端（v0.10 定案）**：M1a 优先用 **RegisterEventHotKey（Carbon，独占 + pressed/released）**，默认配「修饰键 + 普通键」；保留键（⌘Space / ⌃Space 等）直接拒选并给替代建议。`NSEvent` 全局监听**收不到本 App 自身事件、也无法吞键**，只作为接受透传时的备选（local + global 双监听）。CGEventTap 只在上述都满足不了时启用（主动 tap 要处理超时禁用/重新启用/收尾；listen-only 不能吞键）。两条路都**无法保证安全输入时收到 release** → 该场景终止并保留可取回内容
- **原生链接是首个构建门槛**：Python/CLI 实测不能证明 Rust crate 同版可用。必须锁定 binding / C API / ONNX Runtime 版本 + arm64 + deployment target + 头文件库作为**同一构建输入**；用 `cargo rustc -- --print native-static-libs` 核对 cpal 所需 CoreAudio/AudioToolbox 等框架与 C++ 运行时；Xcode 干净构建后 `otool -L` 查无本机私有路径
- **TCC 身份先定**：bundle ID / 签名团队 / 安装路径 / 沙盒策略固定后，用 `codesign -dr -` 对比两次构建实测覆盖升级；ad-hoc 签名每次重编译要重新授权（用同一 Apple Development 身份则不必预设每次失效）。麦克风配 `NSMicrophoneUsageDescription` + audio-input entitlement；分别查麦克风、AX 信任、`CGPreflightPostEventAccess`、（上 tap 时）`CGPreflightListenEventAccess`——**不要首启动一次索要全部权限**
- **签名/公证**：个人自用走本地 ad-hoc 签名即可，但辅助功能授权绑定签名身份，**重新编译后需重新授权**——开发迭代要预留这个成本
- SwiftUI 壳与 core 的线程约定：core 的回调在 core 线程，壳统一 `DispatchQueue.main.async` 转主线程，禁止在回调里同步等主线程（死锁）

## 7. 调研依据

- **OpenLess**（Open-Less/openless，4129★，AGPL-3.0，Tauri 2）：同类最成熟开源实现。REA 审计发行包零暴露（evidence `ev_87e9db96`）。**只参考架构与版本组合，不复制代码**（AGPL 传染）。
- **AutoGLM 桌面版**（`/Applications/AutoGLM.app`，com.zhipu.agent v1.19.0，原生 Swift 65MB，RSS 27.5MB）：REA 审计零暴露（`ev_09ebf260`）。云端听写链路、服务端热词 v2、personas v3、润色提示词、三重上屏回退均已挖出（§10）。
- **VocoType**（商业，DMG 47MB / app 124MB）：参考模型分档与双上屏；不复制任何资源。
- **Qwen3-ASR 官方 Toolkit**：长音频用 VAD 切段（API 3 分钟限，默认 120s）——支撑 §4.4。
- 独立审查：`DESIGN-review-codex.md`、`DESIGN-final-audit.md`、`DESIGN-audit-deepseek.md`（P0 技术硬伤已并入 §4.5 与 M1a 验收）。

## 8. 法律边界

不反编译/复制 VocoType 代码与资源；仅用公开开源组件（sherpa-onnx Apache-2.0、FunASR 模型遵循 ModelScope 许可）与公开行为模式。

## 9. 增补需求

### 9.1 跨设备模型尺寸
- 每台设备独立 `config.toml` 选引擎：强机可上 qwen3（短句档）；弱机/无风扇机默认 sense（166MB）。
- **手机端/鸿蒙（M5 远期）**：小艺无公开本地 ASR API，HiAI 仅面向 HarmonyOS 开发者。现实路线：① 本地小模型 ② 局域网调桌面 Eryu 本地 HTTP 服务。

### 9.2 模型注册表（M1b 起可用，M4 完整）
- `models/`：manifest（id/版本/大小/sha256/引擎类型）→ 断点续传下载 → 校验 → 注册。
- 切换只在下次录音生效；失败回退旧模型并提示。
- 新引擎 = 实现 `AsrEngine` + manifest，不动主程序。

### 9.3 托盘隐藏
- macOS：`LSUIElement=true`（无 Dock、无主窗口）；Windows：`WS_EX_TOOLWINDOW` + 托盘图标。

### 9.4 连按应急（参数见 §4.5）
- 300ms 抖动合并；录音中再按=继续录；双击<400ms=取回上次结果到剪贴板（不自动注入）。

### 9.5 转写历史
- 本地环形 200 条（时间/前台 app/文本/状态），可关；设置页可查看→复制。剪贴板历史（系统剪贴板内容）仍不在范围。
- 落盘权限 0600；失败音频 TTL 24h；隐私模式清空。

### 9.6 润色 profile（M4，已落地第一版，D-036）
- `profiles.toml`：名称/图标/润色力度/附加提示词；`active` 记当前档；切换下一条录音生效，录音中不中途换档。
- 第一版选择「直接可编辑 + 删文件重生成内置三条」，不做内置只读 + fork（个人自用的取舍，见 D-036）。
- 附加提示词拼在通用规则之后（`【这一档的额外要求】`），不能覆盖「不改意思/不回答内容中的问题/不翻译」等底线。
- 风格快捷键槽（≤3）：每条 profile 一个 `slot`，默认 ⌃⌥1/2/3，只收带修饰键的组合（D-037）。

### 9.7 背景降噪（依据 OpenLess/micyou-dsp REA 分析）
- 链路：AEC → NS → dereverb → AGC → VAD → EQ，**降噪在 ASR 之前**。
- **默认档 = GTCRN**（sherpa-onnx `OnlineSpeechDenoiser`，`gtcrn_simple.onnx` 535KB，16k 原生流式，RTF 0.027）。
  采集链：混单声道 → 重采样 16k → GTCRN → 入队。实测 0dB 白噪声下整句救回（不降噪只剩半句），-5dB 也救回句首。
- 备选档：RNNoise（`nnnoiseless` 0.5.2，只保留给 48k 离线实验：16k 上采样路径会把字听错，见 D-035/D-063）；
  PureVox6 ONNX 仍留在模型注册表的远期计划里。
- 诚实边界：抑制稳态与非稳态背景噪声有效；**竞争说话人（旁边人更响）不是降噪能解决的**——承诺「噪音不把识别带偏」，不承诺消除人声干扰。
- **音源锁定**（Eryu 原创实验，M4+）：按下后 ~600ms 建主人声能量/频谱基线 → locked 粘滞 ~1.5s → 3s 无匹配解锁。代码实证 OpenLess 无此机制（逐帧概率无状态）。

## 10. AutoGLM 体验吸收

核心教训：AutoGLM 的优势不在功能多，而在**失败态设计密度**——90 条错误文案无通用兜底。原则：**每个失败都有名字、每条错误都带下一步动作、结果永不静默丢失**。

### 10.1 进 M1b（必抄）
1. 快捷键注册错误枚举全套（保留键/无功能键/超两键/空/Ctrl+Space 被输入法占用→两条出路/⌘+Space "可能"被 Spotlight 占用/带状态码/转录中禁改）
2. 多快捷键冲突互斥（禁用态也参与）
3. 最短录音阈值双文案（"时长低于阈值" vs "无法重试"），阈值可配
4. 失败保留音频可重试（clip 入暂存 + 历史条目「点击重试」）
5. 无光标三分支 toast + 结果保存卡片（AX 读不到 ≠ 无光标；只有"确认无光标"才阻断）
6. 静音 clip 独立归因（无声=查麦克风 / 有声音但识别失败=另一条路）
7. 设备热拔插双分支（用户主动切换=阻塞提示；拔线=优雅结束）
8. 忙碌拒绝可见（不静默丢弃）
9. 权限运行时丢失检测（指名哪个权限、影响什么）+ 设置页麦克风电平测试
10. 承诺式权限文案（"听写全程在本机处理" + 用途清单 + **诚实边界**：模型下载需联网、可选的润色若指向远端会在启用时披露并二次确认）
11. 危险操作三级确认（单条/批量/清空，文案带对象名）
12. 开机静默启动选项

### 10.2 进 M2
- 选区指令管线（AX 直读→模拟 ⌘C→放弃三回退；撤销窗口可配）
- 注入后编辑追踪 → 自动热词（长度上下限；可关；toast「已自动添加热词」）
- 录音时静音其他应用
- 热词管理全套（批量预处理/校验/去重/脏状态拦截）
- 部分成功保留 UI、隐私模式（隐藏历史+禁止重试）、高级参数（ms/s 级）、对比式润色 onboarding
- **完整新手引导（练习输入框+四步）**：用户裁决暂不做，缓到 M2 之后

### 10.3 进 M3-M4
- 润色 profile：内置只读 + fork、⌥+数字切换 + toast、每日首次提示带动作、基础提示词可编辑、依赖可见链
- 状态机测试用例取自 recon C 节：6 态 + 注入矩阵 {dictation,selection}×{no_cursor,saved} + 历史 5 态

### 10.5 OpenLess 技术参考吸收
1. **SessionPhase 补全**：`pending_stop`、`Inserting 拒取消`、迟到事件按 session_id/generation 丢弃
2. **热键三防**：debounce 250ms / 冷却 450ms / 修饰仲裁 150ms（§4.5）
3. **点击穿透防闪帧**：终态不立即 set_ignore_cursor_events，等下一交互帧
4. **静音语义二分**：从未说话→取消（不产生历史）；说完后静音→正常提交（能量阈值 0.02、3 块确认）
5. 佐证：上游已撤掉"输入框实时写入"→ 支持我们"实时只做胶囊显示、终局一次性注入"
6. M2-M4 备选池：流式插入 12ms flush/前缀不匹配整文兜底、模型三态（下载≠加载）+SHA256、Windows TSF IME、繁简 OpenCC

## 11. Spike 实测数据（2026-11-19，决定 §4.2 定档）

环境：MacBook Air（Mac17,3）/ Apple M5 / 17.18GB / macOS 26.7.1 / 无风扇。
方法：sherpa-onnx 2.0.1（Python 绑定 + 官方 osx-arm64 静态二进制，两者 ONNX Runtime 相同）；`/usr/bin/time -l` 读 peak memory footprint；16k mono 输入。

| 指标 | SenseVoice int8 | Qwen3-ASR 0.6B int8 |
|---|---|---|
| 磁盘（解包） | 247MB | 972MB |
| 峰值内存（native） | **635MB** | **1619MB** |
| load（创建 recognizer） | 0.38–0.45s | 1.2–1.7s |
| 11.4s 音频解码 | **202ms**（RTF 0.018） | 2401ms（RTF 0.21） |
| 51s 音频 | 1196ms（整段）/ 698ms（VAD 8 段） | **截断失败**（输出 "language"） |
| 5 文件平均 CER | 0.170 | 0.296（注：参考文本本身有误，zh 一条把 qwen3 的繁体正确输出判错，故 CER 不可直接下结论） |
| 4→8 线程 | 无差异/变慢 | 8 线程退化到 RTF 0.372 |

**30s 到底是什么**（实证，非猜测）：
- qwen3 报错原文：`context_len (677) exceeds max_total_len (512). Truncating audio placeholders: audio_token_len=662 -> keep_audio=497`。51s 音频 ≈ 13 tokens/秒 → 30s ≈ 390 tokens + prompt 约 100 ≈ 512 上限。**这是模型的上下文硬限，不是作者故意设卡。**
- Qwen 官方 Toolkit 处理长音频就是 VAD 切段（API 限 3 分钟、默认 120s）；sherpa-onnx 文档的长音频示例同样是 VAD 切分；Whisper/SenseVoice/Paraformer 也全是 ≤30s 段模型。
- 结论：**不存在"换个模型就不用切分"**；正确做法是 §4.4 的 VAD 流水线。

**Rust binding 侧已复核（2026-11-19，`/tmp/eryu-spike/rustprobe`）**：`sherpa-onnx` crate **2.0.1** 在本机 arm64 编译链接通过（`SHERPA_ONNX_LIB_DIR` 指向官方 static-lib 包，可绕过 GitHub 下载失败）；SenseVoice int8 实测 `load 411ms`、11.4s 音频 `decode 636ms`（RTF 0.056，2 线程）、峰值 RSS **596MB**，与 Python 侧同量级。API 注意：`OfflineStream::get_result()` 返回 `Option`；`Wave` 在 crate 根（`sherpa_onnx::Wave`），不能走私有 `wave::` 路径。

**口径修正**：qwen3 的 512 token / 30s 是**模型硬限**；30s 对 SenseVoice 只是**产品预算（切段策略）**——SenseVoice 51s 整段也能出结果。5 文件 CER（0.170 vs 0.296，且参考文本有误）**不足以宣称精度档**，M1b 开放 Qwen3 前需用本人真实录音复测。磁盘 166MB（压缩包）/ 247MB（解包）需分别标注。

**未测（M1a 补）**：热键 Released 语义、paste 是否污染剪贴板、Secure Event Input 下 CGEvent 行为、光标判定三态、长听写流水线端到端延迟、连续 30 次录音的降频漂移。

## 9.7.1 「只录我的声音」——已落地的声纹门控（v0.16）

- 原设想的「能量竞争 + 粘滞」只能区分响度，分不出谁在说话；单麦克风没有空间信息，真正可行的是**段级声纹校验**。
- 实现：3D-Speaker eres2net base（39.6MB，512 维）+ 余弦阈值 0.62；每段 VAD 音频在送 ASR 前判一次，
  算不出就保留；跳过比例过高时整段重跑不过滤并提示。详见 D-051。
- 注册：设置页「音频 → 只录我的声音 → 录 8 秒当声纹」；声纹存 `~/.eryu/voice_print.bin`（0600），不上传。
- 未做：说话人分离（两人同时说）、跨设备声纹迁移、每档润色 profile 绑定不同声纹。


## 12. 0.8 对标升级（2026-11-20，依据 REA 分析）

> 依据：`rea-targets/REA-分析报告.md`（VocoType 1.6.2 的 Tauri/Rust 结构、AutoGLM 1.11.0 的 Electron 三窗模型、两者 VM 运行截图）
> 与用户原话「你的 Windows 版本太差了，又丑又不可用，连 MVP 都不如」。
> 需求编号与状态以 `docs/BACKLOG-0.8.md` 为准；本条只写「要变成什么样」和「为什么」。

### 12.1 首次体验：60 秒内必须成功一次

- **顺序（6 步，D-112 修订）**：①模型 ②麦克风自检 ③热键 ④上屏方式 ⑤试说一句 ⑥润色。踩过的坑：早期文档写成「5 步、第 3 步试说」，实际壳里热键是独立一页（`STEP_HOTKEY=2`），试说是第 5 步（`STEP_PROBE=4`）——文档与实现不一致会让人按错步去测。试说页仍显示当前热键，可就地换键。
- **强制闭环**：第 5 步（试说）不是设置项，而是一次真实的「按住 → 说一句话 → 看到文字上屏」。页面左例句「今天天气不错，我们开始吧」（去标点 11 字，读全句即满分），右侧随 `partial_text` 逐字出现识别结果；说话两条路都走真实录音链路：按住当前热键，或按住页面里的话筒按钮。
- **成功判据**（验收口径）：右侧最终文本 ≥4 个字符，且与例句的**字符多重集合交集 ÷ 例句字数 ≥ 0.5**（标点/空格不计入；多说的字不扣分）。
- **未成功**：给出具体原因（没听到声音 / 太短 / 识别失败 / 麦克风权限），「下一步」禁用，可重试；整轮仍可「稍后再说」。
- **稍后再说**：写 `onboarding_done=true`，但 `onboarding_probe_done=false`；下次启动从试说页回来，直到成功一次（0.7 → 0.8 的迁移用户同样走一次）。
- **不注入（D-110）**：探针文本只显示在引导页，壳吞掉 `submit_request`、不调用注入器，core 照常收到 `submit_result(ok)` 完成闭环。理由：引导时前台是引导窗，注入要么打空、要么打进用户背后正在用的应用；这一页要证明的是「麦克风→识别→看见字」。
- **配套（R-102）**：麦克风页实时电平（core 66ms 一帧）+ 设备下拉 + 连续 3 秒 `rms<0.01` 提示「未检测到明显声音，换个麦克风或靠近一点」（阈值依据见 D-112）。
- **可测**：`--onboarding-step N` 直达某步；`--onboarding-selftest` 断言判定函数；截图 `docs/onboarding-2-mic.png` / `onboarding-3-probe.png` / `onboarding-3-probe-ok.png`。

### 12.2 文本后处理：三层，从便宜到贵

```
ASR → ① 确定性替换（replace_map / 热词 / 专有名词，0ms）→ ② 标点（本地模型）→ ③ 可选 LLM 纠错（网络，可关）
```

- ① 必须能在断网、无 Key 时工作，且**可导入导出**（个人自用也有迁移需求）。
- ② 已有（标点模型）；不足是只在句末补「。」，问号/感叹号待补。
- ③ 已有（润色档）；要补的是**「纯转录」档**（不联网）与**口语自我修正规则**（「啊不对/我的意思是」删错前内容）——后者是 VocoType 默认提示词里的明确条款，属于「听懂人话」的关键差别。
- 智能数字转换、语气词过滤各做一个开关，默认关；两者都有出错风险，文案里如实写。

### 12.3 交互与窗口：学 AutoGLM 的三窗，不学它的登录墙

| 窗 | 作用 | 我们的约束 |
|---|---|---|
| main | 迷你主窗（170×230，可拖动不可缩放）：麦克风大按钮 + 「按 <当前热键> 或点击开始」+ 今日节省 + 设置 | 可关；关掉只留菜单栏/托盘；`[ui] mini_window` 记住 |
| panel | 录音浮层 | **不抢焦点**（macOS `.nonactivatingPanel`；Windows `WS_EX_NOACTIVATE`），否则打断打字 |
| toast | 全局提示（失败/已复制） | 置顶、3s 自隐、可点击「复制上次结果」 |

- 迷你主窗（R-103，D-111）：点一下大按钮开始录音、再点一下停止并上屏；录音中按钮变停止图标、文案「正在听，点击停止」，`state=idle` 回待机。**只作用于该按钮**，不改全局热键的按住语义（全局点击模式是 R-110，0.8.3）。
- 「今日已节省 X 分」= `stats.today.chars ÷ 1000 × 40 分`（与统计页同口径，向下取整）。
- 关窗 = 隐藏 + `[ui] mini_window=false`；菜单栏「显示主窗」写回 true 并立即显示；引导完成时无条件显示一次并写回 true。
- 另补两种录音模式：**按住说话**（默认）与**按一下开始/再按停止**（长句场景更省力）。
- 悬浮指示器可关（有人嫌挡视线）。

### 12.4 引擎与性能

- 保持「默认快、可选准」：SenseVoice 默认、Qwen3 短句精度档；**不引入自动按语言切换**（实测切换成本高于收益）。
- 0.9 再评估 whisper.cpp（多语）与 Paraformer 原生上下文热词，评估标准是「同一批金标 CER + 峰值内存 + 首字延迟」三项，不达标不换。
- 引擎切换要有可见进度，不能让用户以为卡死。

### 12.5 不做什么（明确边界）

- 付费/license/设备指纹：个人自用无意义。
- 唤醒词：按住说话更可靠，且不误触发。
- MCP 服务器：VocoType 有，但个人自用没有消费方；记在 BACKLOG 的「远期」。
- 登录墙：AutoGLM 的反面教材。

### 12.6 落地顺序（与 BACKLOG 一致）

0.8.1 首次体验闭环（R-101/102/103）→ 0.8.2 文本后处理（R-104…109）→ 0.8.3 交互与统计（R-110/111/113/114）→ 0.9 引擎与发布（R-115…123）。

### 12.7 首次可用：模型怎么给用户（v0.39，R-124）

- **原则**：装完就能说话；用户不该为了「输入法」先理解 ASR/VAD/标点是什么。
- **三档**：
  | 档位 | 内容 | 交付方式 | 用户感知 |
  |---|---|---|---|
  | 随包 | SenseVoice（233MB，默认识别） | zip 内 `models/`，安装脚本拷到数据目录 | 装完直接可用 |
  | 内置 | VAD（0.6MB）、降噪（0.5MB） | `include_bytes!` 编进 exe，首次运行解出 | 不存在 |
  | 用到再下 | 标点（65MB）、声纹（39MB）、Qwen3（879MB） | 打开对应开关/在引擎页选 Qwen3 时下载 | 需要时才下载 |
- **引擎页只列两个识别引擎**（SenseVoice / Qwen3）；标点/降噪/声纹只在用到它们的开关后面自动下载，不占列表。
- **引导门禁**：模型未就绪时「下一步」灰显并说明；模型就绪后模型页只显示「已就绪」。理由是 12.1 的「试说一句」必须真的能出字，否则这一步是在惩罚用户。
- **代价**：zip 从 ~9MB 变成 ~174MB；`package-win.sh` 在没有本机模型时会退化并提示（不阻断构建）。

### 12.8 Windows 诊断包与上传（v0.40，R-127，D-114）

目标是「用户在 Windows 上遇到问题时，我不在他机器前也能定位」，而不是再加一屏给人看的东西。

- **原则**：诊断是**一次点击**的事，不要用户去翻目录、拼日志。
- **一包**：诊断页「导出诊断包」生成一个 JSON（`%APPDATA%\Eryu\diag\eryu-diag-<时间>.json`），内容固定七段：
  1. `app`：产品版本（`APP_VERSION`，与 mac `Info.plist` 同源）、平台、架构、导出时间；
  2. `host`：Windows 版本（按 build 号纠正 Win10/Win11 的注册表偏差）、DPI、屏幕尺寸、exe 路径；
  3. `config`：`config.toml` 全文，但 `api_key` / `token` / `password` / `secret` 一类值一律替换成 `***`（空值保持 `""`，能看出「设没设」）；
  4. `diagnose`：核心自检原文（引擎/模型/VAD/输入设备/历史条数/润色档）；
  5. `errors`：最近 20 条 `{code, msg}`（只取首行）。**错误会落盘到 `eryu-errors.log`**：问题往往发生在上一次运行，诊断包是下一次运行导出的，只留内存会丢证据；
  6. `history`：只统计条数与状态分布（`inserted/pending/undelivered`），**不含正文**；
  7. `log_tail`：`eryu-cli.log` 尾部 200 行（日志超过 512KB 自动截到最近 200KB）。
- **不含正文**：历史只出条数，热词表只出条数。理由：诊断包会被上传，内容最小化。
- **上传**：设置 `[diag] upload_url`（默认 `https://officeace.harmonyos.fun/eryu-diag/upload`，和模型镜像同一台服务器；留空 = 只导本地）。`POST` JSON，`200–299` 才算成功；失败把本地文件路径和原因告诉用户，文件不删。服务端契约见 `docs/WINDOWS.md`「诊断包上传」。
- **入口**：设置 → 诊断页两个按钮「导出诊断包 / 上传诊断包」，上传在后台线程（15 秒超时），结果写回按钮右侧一行；命令行 `--diag-bundle` / `--diag-upload` 给自动化用。
- **错误码**：所有面向用户的失败已经带 code（`docs/ERRORS.md`），诊断页「状态」第一行固定显示 `最近错误：<code>（<时间>）`，复制/上传都带上，方便一句话对上号。

## 12.9 托盘菜单的信息层级（v0.41，R-129）

托盘/状态菜单顶部只放两行纯文本，且都不带图标：

1. `Eryu · 就绪`（状态；录音中/识别中/出错时换词）
2. `上次：<前 18 字>（<N> 字）`（没有历史时这一行不出现）

不写「ALT_R 按住说话」这种提示——快捷键在设置与首次引导里都说过，菜单里重复只会让人看不懂那两个符号。

## 12.10 模型来源与代理（v0.41，R-130 / R-131）

- 「下载地址」不再作为常规输入项：默认走内置镜像（`https://officeace.harmonyos.fun/eryu-models`），页面只给一条可点击的「模型来源」链接；愿意自己下载的人点链接去拿，放进模型目录即可。
- 「下载代理」保留输入框，旁边 ⓘ：留空 = 走系统代理；内网需要时填形如 `http://127.0.0.1:7890`。
- 高级场景（换镜像）仍可通过 `config.toml` 的 `[engine] mirror` 改，不在界面上要求用户理解 URL。

## 12.11 局域网连接引导（v0.41，R-132）

开启「允许其它设备访问」时：

- token 为空则给一个「生成」按钮，点一下生成十六进制串；也允许自己填。
- 页面直接显示本机地址 `http://<本机 IPv4>:8793/transcribe` 与一条可复制的 curl 示例（含 `x-eryu-token` 头）。
- 地址取第一个非回环 IPv4；没有网络时显示「先连上网络」。

## 12.12 热词只填「想听到的词」（v0.41，R-133 / D-116）

用户只写自己要听到的词（如「深度求索」「艾瑞」）。界面单列输入，一行一个：

- 保存时写入 `from = 该词`、`to = 该词`（等价规则），该词同时进入润色提示里的「正确写法」清单。
- 「容易听错成什么」仍由程序从历史里学（采纳建议时补成 from→to 规则），不需要用户自己想。

## 12.13 润色完整性（v0.41，R-135 / D-115）

- 请求体带 `max_tokens`：按输入长度估（`字数 * 3 + 256`，clamp 512..8192），不再让服务端用默认小额度。
- 响应检查 `finish_reason`：`length` 视为失败（被截断），**这一句用原文**，浮层说明「润色没写完，这次贴的是原话」。
- 其它失败（超时/网络/格式）同样回退原文；绝不把半句或改残的句子写进目标程序。

## 12.14 设置页输入：行身份要稳，快捷键要通（v0.42，R-137 / I-174 / I-175）

设置页里有两类输入，最容易在「看起来能用」的情况下坏掉，规则写死：

- **列表行的身份与内容解耦**。热词这类可编辑列表，行 id 必须由创建时分配的 UUID 决定，不得由「内容拼接」计算。否则每敲一个字都会让 SwiftUI 认为行被替换，焦点和输入法组合态一起丢（mac 实测：只能打进第一个字符）。Windows 侧自绘控件按固定 id 复用，不重建页面，天然规避；但任何「编辑时重建页面」的写法都视为违反本条。
- **自家窗口也是注入目标**。mac 的菜单栏常驻 app 默认没有主菜单，而 SwiftUI 文本框的 ⌘V/⌘C/⌘A 靠「编辑」菜单命令沿响应链派发——没有主菜单，这些键在自家窗口里全部静默失效，语音上屏的 ⌘V 也会落空。因此启动时必须装一个隐藏主菜单（应用：隐藏/退出；编辑：撤销/重做/剪切/拷贝/粘贴/全选）。
- **验收方式**（两端一致）：① 连续键入 ≥5 个字符，焦点与输入法不断；② 手工粘贴（mac ⌘V / Windows Ctrl+V）进设置里的输入框；③ 光标在该输入框时按热键听写，文本落到框里、历史为 `inserted`。三条都要有截图或日志证据，凭「代码看起来对」不算。
