# 新会话交接单（0.8 优化）

> 用法：在一个**新会话**里（工作目录 `~/Projects/Eryu`）把下面「启动提示词」整段贴进去即可。
> 本文件同时是交接记录：谁开的会话、做到哪，都追加在末尾。

---

## 启动提示词（复制这一段）

```
你是 Eryu 项目的开发会话（继续 macOS 0.7.0 / Windows W10 之后的工作）。

开工前按顺序读，不要跳：
1) docs/KICKOFF-0.8.md（本文件，含边界与节奏）
2) docs/GOVERNANCE.md（版本/需求/进展管理办法，强制）
3) DESIGN.md（唯一设计权威，重点 §3 架构、§4 流程、§5 里程碑、§12 对标升级）
4) docs/BACKLOG-0.8.md（需求清单 R-101…，本轮只做 0.8.1 的三条）
5) docs/ISSUES.md 里「未解决」的条目
6) docs/PROGRESS.md 最后 20 行

本轮目标（0.8.1，只做这三条，不要扩范围）：
- R-101 引导里强制完成一次「按住→说→看到字上屏」闭环，未成功不让下一步（可整体跳过）
- R-102 引导麦克风页：实时电平条 + 设备选择 + 「未检测到明显声音」提示
- R-103 引导完成后落到迷你主窗（大麦克风按钮 / 按 F2 或点击开始 / 今日节省 / 设置入口）

硬规则：
- 先改文档再改代码；需求先写进 docs/REQUIREMENTS.md 并同步进 DESIGN.md §12，才动手。
- 每个可交付小步完成后：代码提交 + 一行 PROGRESS + CHANGELOG「未发布」段；四者缺一不算完成。
- UI 改动必须附截图（macOS: screencapture -x -R；Windows: VM CopyFromScreen）。
- 不许把 Windows 的事混进这一轮；Windows 需求在 BACKLOG 里挂着。
- 不确定就停下来问用户，不要自行扩大范围。

平台事实（别重新踩坑）：
- macOS：无 Xcode.app，只有 CLT；scripts/build.sh 构建到 build/Eryu.app；签名身份固定 "Eryu Dev"（自签，TCC 跨重建保持）；安装用 scripts/install.sh 覆盖 /Applications/Eryu.app，别留副本（用户明确不满）。
- 自测：scripts/smoke.sh、--self-test、--acceptance、--decode-wav、--simulate、--diagnose、--dump-config。
- Windows：本机交叉编译 `PATH=/opt/homebrew/opt/llvm/bin:$PATH SHERPA_ONNX_LIB_DIR=/tmp/winlibs/sherpa-onnx-v2.0.1-win-x64-static-MD-Release-lib/lib cargo xwin build --release --target x86_64-pc-windows-msvc`（在 core/ 与 shell-win/ 各一次）；VM 测试用 vmrun（lucien/Eryu2026）。
- 模型镜像默认 https://officeace.harmonyos.fun/eryu-models（core 已支持系统代理 + [models] proxy）。

结束时：更新 PROGRESS + 提交 + 在本文件末尾追加「本轮结束状态（时间、提交号、验证命令输出、遗留）」。
```

---

## 上下文速览（给新会话的判断依据）

- 产品：个人自用的本地语音输入法（macOS + Windows），按住单键说话、松手上屏，不上传音频。
- 现状：macOS 0.7.0 可用；Windows W10 绿色单文件（含引导、设置七页、选区改写、注入校验、暂停媒体、失败段重试、模型镜像+代理）。
- 差距结论（来源 `rea-targets/REA-分析报告.md`）：VocoType 的竞争力在 ①引导里强制第一次成功 ②三层后处理（确定性替换 → 标点 → 可选 LLM 纠错）③迷你主窗 + 悬浮球 ④多引擎；AutoGLM 值得学的是三窗模型（main/panel/toast）与「交互浮层不抢焦点」。
- 用户最不满的两点：Windows UI 观感（已修一轮，仍需按截图迭代）、首次体验没有「60 秒内成功一次」。

## 本轮结束状态

（新会话在这里追加）

### 0.8.1（2026-11-20 20:15 +08:00）

- 提交：`d042576` `mac: 引导五步与试说闭环 + 迷你主窗（R-101/102/103）`；前序文档/core 提交 `af23b7e`、`2c6def8`、`7925b4c`、`b3fe4ab`。
- 范围：只做 R-101 / R-102 / R-103（D-110/111/112）；0.8.2、0.8.3 未动。
- 验证命令与结果：
  - `cargo test --manifest-path core/Cargo.toml`：65 + 4 + 6 + 2 全过（含 `onboarding_state_survives_toml`）。
  - `./scripts/smoke.sh`：全部通过（含 5a–5k、6 极端矩阵、7 文档一致性）。
  - `--onboarding-selftest`（纯判定）：8 条 `PROBE OK`、`ONBOARDING SELFTEST 判定函数 OK`，exit 0。
  - 端到端（`ERYU_MIC_WAV=/tmp/eryu-probe.wav`，TTS 读「今天天气不错，我们开始吧」）：`submit_request … len=12`、`ONBOARDING SELFTEST OK 第 3 步出字：今天天气不错我们开始吧。`，exit 0。
  - `--dump-config`：`[ui]` 可见 `onboarding_done`、`onboarding_probe_done`、`mini_window`（I-101 已修）。
  - 截图：`docs/onboarding-1-model.png`、`onboarding-2-mic.png`（含「未检测到明显声音」）、`onboarding-3-probe.png`（未出字、下一步灰）、`onboarding-3-probe-ok.png`（绿色已成功识别、下一步可用）、`onboarding-4-output.png`、`onboarding-5-polish.png`、`mini-window.png`（170×230 待机；点击后变「正在听，点击停止」，再点经 `no_audio` 回待机）。
- 遗留：
  - Windows 端未跑本轮（引导/迷你窗只在 macOS 壳实现）；VM 声卡已在且录音链路补验通过（见下「Windows VM 声卡验证」），Windows 引导/迷你窗要到下个版本再做。
  - 新发现 I-102：单实例锁 fd 被 curl 继承，App 退出后仍报「已有实例在运行」，本轮只登记未修。
  - I-071 长听写延迟、I-074 按应用覆盖真实验证仍未做（不属本轮）。

### Windows VM 声卡验证（2026-11-20 20:45 +08:00）

- 结论：VM（Windows 11 ARM，VMware Fusion）自带 VMware 虚拟声卡即可用，**不需要**装第三方虚拟声卡。`Win32_SoundDevice` = `High Definition Audio Device`（HDAUDIO VEN_15AD/DEV_1975），「扬声器 / 麦克风 (High Definition Audio Device)」两个端点 state=1。此前 VB-Cable 的 x64 驱动在 ARM 上不加载（加进驱动库但不生成设备），已放弃。
- 验证命令：host `osascript -e 'set volume output volume 100'` + `say -v Tingting "今天天气不错，我们开始吧"`；guest `C:\eryu-green\Eryu.exe --mic-test 5`（VM 内便携版）。输出：`[eryu] 采音开始 麦克风 (High Definition Audio Device) 44100Hz 2ch` → `MICTEST OK 结果=天天气不错我们开始吧今天天气不错。`。验证后 host 音量已恢复 19。
- 坑：`--pull-model sense` 在 VM 里下到的 `model.int8.onnx` 尺寸正确但 SHA256 不符 → `Protobuf parsing failed` → 引擎回退 stub（I-103）；从 host `vmrun copyFileFromHostToGuest` 覆盖正确模型（237MB，约 2 分 40 秒）后才真实识别。
- vmrun 实操：`runScriptInGuest` 在此 guest 会挂死；一律用 `runProgramInGuest "$VMX" -noWait 'C:\Windows\System32\cmd.exe' '/c C:\Users\lucien\x.bat'`，写 CRLF 的 `.bat`、产物 `copyFileFromGuestToHost` 回 host，GBK 输出用 `iconv -f GBK -t UTF-8` 读。

### Windows 对齐轮（2026-11-20 23:2x +08:00）

提交（自 I-104 起）：`6e03cf8`（死锁）、`e769534`（P0/P1 稳定性）、`81cc5ef`（设置页蓝框 I-105）、`d885f1f`（单实例 + 电平测试）、`be70928`/`073268e`（托盘菜单 + 自测）、`4cf73aa`（历史页 + I-106 自绘开关）、`b28d7e9`（补标点/音源锁定/下载进度/自检）、`651bace`（按应用覆盖）、`1606081`（关于版本号 + caret 翻转）、`0bdfde8`（声纹页）、`a3ab2ff`（润色测试连接）、`e09857e`（热词建议）、`d2a7a75`（唤醒重装钩子）、`3c07827`（自测静默 + 逐页冒烟）、`94cefa0`（真实热键 E2E）、`d6e02cf`（迷你主窗）、`2054a4f`（局域网页，I-107 待验）、`a6c4a04`（打包脚本 + 全页冒烟）。

关键验证（VM，Win11 ARM + VMware HDAudio）：
- 真实按键（F13）→ 采音 → 识别 → 上屏：Notepad 出现「今天天气不错我们开始吧。」，进程 `Responding=True`（`docs/win-e2e-real.png`）。
- `--self-test`、`--onboarding-probe-selftest`（8 条，与 mac 同阈值）、`--settings-pages-smoke`（9/9 页）、`--tray-menu-dump`、`--voice-state`（512 维已登记）全绿。
- 发布包：`dist-win/Eryu-0.8.1-win-x64.zip`（单文件 exe + 一页说明），在 VM 里解压后 `--self-test` 通过。

遗留：I-107（局域网页 UI 已加、VM 内链路未验通）；热词建议/声纹/LAN 的 mac 侧对等性未逐一比对；VM 仍不能驱动 UAC 提权层面的检查（I-081 已解，SendInput 合成键在正常桌面可用）。

### Windows 对齐完成（2026-11-21 05:05 +08:00）

自 `6e03cf8` 起共 78 个提交，覆盖：崩溃/卡死（I-104/I-108/I-109）、稳定性（钩子只投递、出锁弹框、注入超时、GDI/事件泄漏）、注入（I-112/I-113：UIA 优先、迷你窗不抢前台、脱离消息回调注入、钩子放过合成键）、UI（`ui.rs` 统一字体、无边框圆角窗、ⓘ 悬停提示、分段按钮、DPI 收缩）、能力对齐（按应用覆盖/声纹/局域网 I-107/热词建议/历史三项/迷你主窗/引导 6 步含试说/润色预设·测连·检测模型·档位指令·同意弹窗/诊断 TOML 编辑/权限状态/附加模型下载）。

最终验证（VM，装到 `%LOCALAPPDATA%\Programs\Eryu`）：
- `--self-test` → `SELFTEST OK 结果=自测：你好，Windows。`
- `--settings-pages-smoke` → `PAGES done`（9 页）
- `--onboarding-probe-selftest` → 8 条 `PROBE OK`（重合率 1.00/0.55/0.45/0.18/0.00/0.55/1.00/0.00）
- `--tray-menu-dump` → 14 项，含「权限状态…」「测试麦克风 3 秒」
- `--perm-report` → `麦克风：允许`
- 端到端（`--hotkey-sim --hotkey-sim-force` + `ERYU_MIC_WAV`）→ 记事本出现「今天天气不错我们开始。」，`Responding=True`
- mac：`scripts/build.sh` + `scripts/smoke.sh` 全绿；`scripts/install.sh` 只留 `/Applications/Eryu.app` 一份
- 发布包：`dist-win/Eryu-0.8.1-win64.zip`

遗留：真机手测（真实麦克风 + 人手按键，VM 无法代替）；单击/双击模式（R-110，0.8.3 计划内，两端都未做）；注入在管理员窗口里受 UIPI 限制（已写进权限状态说明）。

### Windows 对齐复验（2026-11-21 16:05 +08:00）

自 05:05 那轮之后，按「用户能在真机上用」的标准又跑了一整轮，修掉的真问题（都已进 `docs/ISSUES.md`）：

- **稳定性/不会卡死**：I-127（上屏成功后持锁调 `observer::watch` 自锁）、I-140（引导页 `WM_DRAWITEM` 阻塞锁改 `try_lock`）、I-141（内置 core dll 指纹用「长度+首末字节」→ 升级后仍跑旧 dll；改 FNV-1a 内容哈希）、I-142（上屏等待 2s 太短，文字已进文档却被判超时 → 历史永远「待复制」+ 每次启动弹取回；等待放宽到 6s 且「迟到的 ok」也认）。
- **键盘可达**：I-133（设置页/引导页 `WS_TABSTOP`+焦点描边+Tab/回车，`edit()`/`listbox()` 补漏）、Ctrl+Tab / Ctrl+1..9 切页（`switch_page` 统一鼠标/键盘路径，顺带修鼠标切页电平条不停）。
- **开关可信**：I-135（`WM_COMMAND` 先 match 后 flip → 落盘旧值，全部自绘开关逐项复验）、I-134（开润色但档位 off → 默认 light）。
- **观感**：I-138（150% 下列表滚动条露白槽）、I-139（胶囊提示行按壁纸亮度选字色）、I-115/I-120/I-126（徽标、键帽、电平条字形）。
- **上屏**：`inject.rs` 校验加 UIA 读回兜底（ValuePattern → TextPattern），WinUI 记事本也能确认 `ok`。

复验证据（VM，装到 `%LOCALAPPDATA%\Programs\Eryu`，exe `A5F7CA836E71AD72…`）：`--self-test` OK、9 页冒烟 OK、`--onboarding-probe-selftest` 8 条 OK；模拟听写 → 记事本落字 + `history.json` `status: inserted` / `app: Notepad.exe`，重启不再弹取回；连跑 5 轮模拟听写 3 轮成功（其余落在识别中按设计忽略），内存/句柄无增长；设置窗 ✕ 隐藏不退出、再点图标能唤回；mac `build.sh`+`smoke.sh` 全绿、`cargo test --lib` 67 passed。

剩余（只能由用户本人完成）：
- 真机手测：按 `docs/win-manual-test.md` 的 30 条走一遍（真实麦克风、真实按键、输入法拼写中按热键、管理员窗口注入边界）。
- I-102（`InstanceLock` 的 `O_CLOEXEC`）留到 0.8.2；R-110（按住说话/按一下开始再按停止）按计划在 0.8.3，两端都未做。


## 首次可用复验（2026-11-21 17:42，回应 m12352 五条）

- **模型**：SenseVoice 随 zip（`models/`，226MB），VAD+降噪 `include_bytes!` 进 exe，标点/声纹/Qwen3 改成「用到再下」；引擎页只列两个识别引擎（D-113、R-124、I-144）。
- **装完能用**：全新 purge → 解压 → `install-win.ps1 -Desktop` → 直接启动，首屏即「语音模型已就绪」（`docs/onboarding-win-firstrun.png`）；升级场景补 `model.int8.onnx` 判定（I-146），VM 造半截模型后重装被补齐并真实识别出「今天天气不错。」。
- **引导门禁**：模型未就绪不进「试说一句」，「下一步」灰显并说明；`model_done` 事件即时解锁（截图 `docs/onboarding-win-probe.png`）。
- **字号与清晰度**：正文 13→14、说明 11→12、小节 14→15；胶囊分层窗口改灰度抗锯齿 + 提示行 alpha 曲线 `x^1.7`（I-145，截图 `docs/overlay-win-capsule.png`）；`--dpi-report` 实测 `awareness=2 / system=96`，排除 DWM 缩放致虚。
- **排查入口**：诊断页第一屏改只读状态 +「复制状态/打开日志/运行自检/麦克风权限」，TOML 降级为「高级」（I-147，截图 `docs/settings-win-diagnostics.png`）。
- 打包 `dist-win/Eryu-0.8.1-win64.zip`（169MB，含模型不含样例音频）；VM 装机构建 `--self-test`、9 页冒烟、Tab 焦点全绿。

待用户确认：④「重影」具体在哪个窗口（设置/胶囊/托盘），⑤「过度设计」具体指哪一块；真机麦克风与真实按键仍需本人手测。
