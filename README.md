# Eryu（字在 zzai）

个人自用的 macOS / Windows 本地语音输入工具。按住快捷键说话，松开文字上屏；听写全程在本机处理。

官网：<https://zzai.harmonyos.fun>　｜　安装包下载：[Releases](https://github.com/flyeasy/zzai/releases)

## 文档权威顺序（冲突时按此裁定）

1. **`DESIGN.md`** — 唯一设计权威（Single Source of Truth）。任何与代码冲突，以它为准；它本身要改，必须**升版本号**并记入 `docs/DECISIONS.md`。
2. **`docs/DECISIONS.md`** — 决策记录（ADR，编号 `D-xxx`）。每条决策写明理由与被它取代的旧结论。
3. **`docs/REQUIREMENTS.md`** — 用户提的需求与变更登记（`R-xxx`），含来源消息号、状态、落到 DESIGN 的哪一节。
4. **`docs/ISSUES.md`** — 实现过程中发现的技术问题（`I-xxx`），含证据与结论。
5. **`docs/PROGRESS.md`** — 阶段台账：做到哪一步、验收项、证据路径。

逆向侦察与审计报告：`DESIGN-recon-*.md`、`DESIGN-*-audit*.md`（历史依据，不作为当前规范）。

## 代码结构

```
core/                 Rust 核心（staticlib）：采音 / VAD / ASR / 长听写 / 热词 / 润色 / 状态机 / 存储
  include/eryu_core.h 冻结的 C 头（FFI 契约）
  src/                ffi.rs（ABI）/ session.rs（状态机）/ events.rs（事件流）/ …
app/                  Swift 壳（SwiftPM）：菜单栏、热键、注入、设置页
  Sources/EryuC/      C 模块（供 Swift import，指向 core 的头）
  Sources/EryuApp/    AppKit/SwiftUI 壳
scripts/              构建 / 组装 .app / 签名 / 冒烟
artifacts/            逆向材料与 spike 产物
design/               Stitch UI 目标稿
```

## 首次运行：需要你点 3 个授权（macOS 强制，程序不能代劳）

| 权限 | 位置 | 不给会怎样 |
|---|---|---|
| 麦克风 | 系统设置 → 隐私与安全性 → 麦克风 | 收不到声音 |
| 辅助功能 | 系统设置 → 隐私与安全性 → 辅助功能 | 没法粘贴上屏、单键热键不响应 |
| 输入监控 | 系统设置 → 隐私与安全性 → 输入监控 | 监听键盘/合成 ⌘V 可能投递失败 |

应用启动时会弹窗列清缺哪几项，点「去授权」会依次弹系统窗口；勾选 **Eryu** 即可。
签名身份已固定为自签名证书 `Eryu Dev`（D-018），重建不会改变签名身份。

### 为什么「明明给了权限，还是不行 / 还一直问」

**辅助功能与输入监控是按「哪个 app 文件」记的**：同一个 Eryu 的两份副本（`build/Eryu.app`、
`dist/Eryu.app`、`/Applications/Eryu.app`）在系统设置里是**分别授权**的。
给了一份，启动另一份照样是「被拒绝」——这就是以前「一直问、又好像没生效」的根源（D-098）。

所以现在只保留一个可运行副本：

```bash
./scripts/install.sh     # 装到 /Applications/Eryu.app，并删掉项目里的副本
```

要确认到底哪项没生效，看这两个地方（结论一样）：

- 菜单栏 Eryu 图标 → **权限状态…**（可复制）
- 每次启动都会写 `/tmp/eryu-perm.json`：三项权限 + 能不能真读到焦点控件 + 能不能真装键盘监听

> ⚠️ 从终端直接跑二进制会**继承终端的授权**，读到的是假阳性（I-018）。判断权限只信「双击/`open` 启动的副本」写出的报告。

## 怎么用

见 [`docs/USAGE.md`](docs/USAGE.md)：日常三条、什么场景关哪个开关、三种「没反应」怎么查。

## 备份与恢复

见 [docs/BACKUP.md](docs/BACKUP.md)：每个冻结版本都会在 `~/Backups/Eryu/` 留一份 App + 完整 git 历史，
可用 `git clone <bundle>` 与 `--self-test` 双重验回。

## 构建与自测

```bash
./scripts/build.sh            # cargo + swift + 组装 + 签名 → build/Eryu.app
./scripts/smoke.sh            # 不需要 TCC 的自检（ABI 符号 / 事件流 / 链接 / 签名）
open build/Eryu.app           # 首次启动需授权麦克风与辅助功能

# 不按热键也能验证链路（终端里跑，不需要辅助功能）
build/Eryu.app/Contents/MacOS/Eryu --mic-test 3     # 真麦克风 → 识别 → 打印结果
build/Eryu.app/Contents/MacOS/Eryu --self-test      # 固定文字 → 事件流端到端
build/Eryu.app/Contents/MacOS/Eryu --diagnose       # 引擎/模型/设备/历史/权限
build/Eryu.app/Contents/MacOS/Eryu --dump-config     # 当前生效的 config.toml
build/Eryu.app/Contents/MacOS/Eryu --print-last      # 取回崩溃前没上屏的那条
```

日常用法：按住 **右 Option**（或你在「设置 → 通用」里录的任意单键/组合键）说话，松开上屏。
屏幕上会有底部胶囊浮层：电平条 + 「在听… 松开上屏」+ 边说边出的准流式文本（900ms 一次，不上屏）。

菜单栏：复制上次结果 / 重试上次失败的识别 / 选区改写…（⌘E）/ 设置…（⌘,）/ 统计与历史…（⌘h）/ 取消当前录音（⌘.）。

设置页六分页（通用 / 引擎 / 麦克风 / 热词 / 润色 / 统计历史 / 诊断）：

- **引擎**：SenseVoice（默认，233MB）与 Qwen3（精度档，879MB）；没装可直接点「下载」，完成后一键切换（D-029）。
- **上屏方式**：粘贴（默认）或逐字打字，可对单个应用覆盖（终端里打字更稳）。
- **热词**：听错的说法 → 正确写法；上屏后你若改了词，会自动进「自动学到的改法」等你采纳（D-026）。
- **润色 / 选区改写**：需要填一个 OpenAI 兼容的 Key（默认 DeepSeek），首次开启会提示「这段文字会离开本机」。
- **隐私模式**：不写历史、失败音频不落盘、浮层不显示识别文本。

调试与自测命令（不需要 TCC 的都能在终端直接跑）：

| 命令 | 验什么 |
|---|---|
| `--verify-test` | 注入后校验的判定（读到输入框文本才报 ok） |
| `--type-event-test` | keystroke 键盘事件构造（UTF-16 回读一致） |
| `--edit-observe-test` | 热词自动学习（改词 → 候选） |
| `--selection-test` | 选区读取 + AX 就地写回 |
| `--hotkey-selftest` | 单键热键按下/松开 |
| `--demo-overlay` / `--demo-palette` | 浮层与指令面板截图 |
| `--pull-model <id>` | 走完整 ABI 下载一个模型 |

## 现在到哪了

M1a（核心链路）、M1b（失败可见 + 设置页 + 统计 + 历史）已完成；M1c（权限兜底）除 TCC 真机验收外完成。
逐项证据见 `docs/PROGRESS.md`。

**需要你点两次的系统授权**（agent 不能代点）：
1. 首次启动的「Eryu 想访问麦克风」→ 允许
2. 系统设置 → 隐私与安全性 → 辅助功能 → 勾选 Eryu（否则只能识别、不能自动粘贴）

签名与 TCC 的取舍见 `docs/DECISIONS.md` D-003 与 `docs/ISSUES.md` I-013（ad-hoc 签名每次重建都会重新授权；
建一张名为「Eryu Dev」的代码签名证书后，`scripts/build.sh` 会自动改用它，授权就不再丢）。

### 自测命令（agent 也能跑）

| 命令 | 验证什么 |
|---|---|
| `--self-test` | 事件流 + 上屏协议（不按键、不发声） |
| `--mic-test 5` | 真麦克风 → VAD → 识别 → 打印 `PARTIAL` 准流式文本 |
| `--hotkey-selftest` | 合成一次「右 Option 按下/松开」，验证热键→录音链路（注入 dryRun） |
| `--decode-wav <wav>` | 把 16k WAV 当成一次录音走完整链路；配 `ERYU_FAULT_FAIL_SEGMENTS=<段号>` 复现「部分成功 → 单段重试」 |
| `--segment-test` | 部分成功重试的 ABI 链路（不存在的段返回 -3 与中文原因） |
| `--voice-state` | 声纹状态（JSON：是否注册/维度/模型在不在/阈值） |
| `--voice-enroll-wav <wav>` | 用一段 16k WAV 注册声纹（设置页「录 8 秒当声纹」走麦克风） |
| `--voice-clear` | 删除声纹（幂等） |
| `--demo-overlay` | 浮层视觉：在听 → 流式文本 → 上屏中 → 收起（截图核对） |
| `--perm-check` | 三项权限真实状态（必须用 `open` 启动） |
| `--inject-test` | 剪贴板写入 → ⌘V → 还原（要求应用能成为前台，否则安全跳过） |
| `--double-tap-test` / `--single-tap-test` | 连按语义：双击取回上次结果 / 单按仍报太短 |
| `--simulate <wav>` | 模拟一次真实听写：合成右 Option 按下/松开 + 用 WAV 当麦克风，浮层/流式/松手出字全走真实链路（不写历史、不上屏） |
| `--soak N <wav>` | 同进程连解 N 次，看内存/延迟有没有漂移 |
| `--baseline` | 冷启动到可听写的毫秒数 + 常驻内存（RSS） |
| `--bench`（松手→出字 p50/p95 与验收判定）、`--diagnose` | 引擎/模型/设备/历史/热词 |

## 测准确性（金标语料）

```bash
python3 scripts/make-corpus.py                     # 用 say 生成 25 条 16k 语料到 /tmp/eryu-bench/wav
python3 scripts/bench.py                           # 默认引擎（SenseVoice）
python3 scripts/bench.py --engine qwen3            # 短句精度档
```

输出按类别（zh/en/mx）给 CER、p50、p95。基线数字见 `docs/DECISIONS.md` D-066。

## 安装到「应用程序」

```bash
./scripts/dist.sh        # 产出 dist/Eryu-<版本>.zip（9MB 左右，已签名）
./scripts/install.sh     # 装到 /Applications，并清掉项目里的副本（只留一个 Eryu）
open /Applications/Eryu.app
```

**为什么要 `install.sh` 而不是直接 `cp`**：`build/Eryu.app`、`dist/Eryu.app` 只要被打开过，
Launchpad 就会各自登记一次，于是出现「两个 Eryu」——两份二进制其实一模一样，点哪个纯看注册顺序。
`install.sh` 会注销并删掉项目内副本，系统里只留 `/Applications/Eryu.app`。
开发/冒烟要用 `build/Eryu.app` 时再跑 `./scripts/build.sh` 生成即可。

首次启动会：①Gatekeeper 问一次「无法验证开发者」（自签证书导致，允许即可）；
②问要不要下载语音模型（SenseVoice 247MB + 标点 65MB + 降噪 1MB）；③问三项系统权限。

## 第一次真机验收（一条命令）

```bash
open /Applications/Eryu.app    # 先打开一次，按提示给三项授权
./build/Eryu.app/Contents/MacOS/Eryu --acceptance
```

然后按住右 Option 说一句话、松开。它会逐项打印：权限、有没有接住按键、浮层有没有出现、
流式片段、最终文本、延迟，并写一份 `/tmp/eryu-acceptance.json`。

> 注意：从终端跑时权限读数会继承终端（记为 I-042），所以更可靠的是看「浮层/按键/结果」三项。
