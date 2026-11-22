# 阶段台账
| 10i | 引擎精度 A/B（25 条金标）+ 块长按引擎自适应 + Qwen3 跳过标点 | 完成（D-060，见 I-037） | `--decode-wav` 与 `recognize_wav` |
| 10j | 睡眠/唤醒处理（录音中收尾 + 唤醒重挂热键） | 完成（D-061；真实合盖待验 I-038） | `--sleep-test`，冒烟 3f |
| 10k | 降噪换 GTCRN（16k 流式，535KB）：0dB 噪声整句救回 | 完成（D-063） | `denoise_ab` + `--simulate` |
| 10l | 松手不丢「正在算的那块」：延迟 2093ms→≤1028ms | 完成（D-064，I-039） | `--simulate` ×4，冒烟 3e |
| 10m | 接缝补白三种方案实测对比（保留基线） | 完成（I-036 更新） | `/tmp/t80.wav` ×5 |
| 10n | 音源锁定（实验）：锁最先说话的人，跳过插话段 | 完成（D-065，I-040） | `sourcelock_probe`、`--decode-wav /tmp/mix_ab2.wav` |
| 10o | 金标基准脚本（25 条，产品链路）：sense CER 0.093 / qwen3 0.040 | 完成（D-066） | `scripts/bench.py` |
| 10p | 小声说话：自动增益 + 语音包络判断（替换绝对静音阈值） | 完成（D-067，I-041） | 0.8% 音量 5 条 CER 0.080；均匀噪声仍判静音 |
| 10q | `--acceptance` 一条命令验收真实按键（权限/浮层/流式/结果/延迟） | 完成（D-068，I-042） | 合成按键自测六项全 ✅、延迟 673ms |
| 10r | 降噪边界实测：稳态噪声有效、竞争人声更差（CER 0.138→0.384） | 完成（D-069，I-043） | `scripts/mix-bg.py` + `scripts/bench.py` |
| 10s | 验收接进冒烟（3g：合成按键六项） | 完成（D-070，I-044） | 冒烟 279ms 全绿 |
| 10t | 交付：dist.sh 打包 + 首次运行问模型（下拉即用） | 完成（D-071，I-045） | dist zip 8.6MB、签名校验通过；gtcrn 挪走→弹窗→下载成功 |
| 10u | 自回声实测 + 太轻不上屏（SPEECH_MIN_PEAK 2%，no_audio/too_quiet 分档） | 完成（D-072，I-046） | 外放 TTS 峰值 1.5% 不再出乱码；近场 65% 正常 |
| 10v | 发布形态：版本 0.2.0、RELEASE.md、CHANGELOG.md | 完成（D-073） | `dist.sh` 出 0.2.0 包 |
| 10w | 录音中按 Esc 取消（浮层提示 + 冒烟 3h） | 完成（D-074） | `ERYU_SIM_ESC=1 --simulate` → ESCTEST OK |
| 10x | 失败文案表（docs/ERRORS.md，24 个码 → 文案 → 下一步） | 完成（R-052） | 覆盖 no_audio/too_quiet/submit_*/segment_*/voice_*/record_limit |
| 10y | 指标脚本：接缝完整率（88–100%）、音源锁定命中（2/2 留、0/2 漏） | 完成（D-075，I-047） | `scripts/seam.py`、`scripts/sourcelock_metric.py` |
| 10z | 阶段状态复核（DESIGN §5.0）+ 基准回归（CER 0.093 / p95 497ms，与 D-066 一致） | 完成 | `scripts/bench.py --engine sense` |
| 10z2 | 使用手册 docs/USAGE.md（场景 → 开关建议、三种没反应排查） | 完成 | README 链接 |
| 10z3 | 壳契约 `--contract`（ABI/事件 envelope/submit 往返）+ 冒烟 2d | 完成（D-076） | `CONTRACT OK ABI=65547 事件=10` |
| 10z4 | 极端状态矩阵 scripts/matrix.sh（7 项全绿）+ `ERYU_CONFIG_DIR` 真读修复 | 完成（D-077，I-048） | 空目录/坏设备/坏 TOML/不可写 四种 |
| 10z5 | 文档一致性检查 scripts/doccheck.sh（首跑抓出 4 个缺码 + ABI 解析错）+ 冒烟第 6/7 项 | 完成（D-078） | 冒烟 16 项全绿 |
| 10z6 | 降噪收益表（信噪比口径）：白噪声 0dB 0.236→0.154；3× 噪声 0.455→0.718；人声基本持平 | 完成（D-079，I-049） | `scripts/denoise_table.py` + `denoise_file` |
| 10z7 | FFI 容错测试 fuzz_ffi.rs（空指针/坏WAV/坏JSON/非法UTF-8/超长/坏声纹）6 项全绿，单测 58 + FFI 4 + 容错 6 | 完成（D-080） | `cargo test` 全绿 |
| 10z8 | 崩溃恢复端到端（SIGKILL 137 → 暂存 48B → 重启取回一致）+ decode-wav 防 stub 假绿 | 完成（D-081，R-059） | `scripts/crash_recovery.sh`，冒烟 5b |
| 10z9 | 引擎守卫扩展到 simulate/mic-test/soak/segment-test；审计全部脚本的临时目录用法 | 完成（D-082，I-050） | 空目录 `--simulate` 被拦、放行开关可用、矩阵仍全绿 |
| 10z10 | 两分钟上限端到端：167s 语料 → record_limit → 前 120s 文本 → 908ms | 完成（D-083，R-061） | `scripts/longcap.sh`，冒烟 5c |
| 10z11 | 连续说话接缝：110s 无停顿 20/20 句齐全、546ms；167s → 22 句连续前缀 | 完成（D-084，R-062，I-051） | `scripts/nopause.py`，冒烟 5d |
| 10z12 | 上屏失败兜底：强制 undelivered → 剪贴板有结果 + 菜单可取回 + 状态栏说明 | 完成（D-085，R-063） | `scripts/fallback.sh`，冒烟 5a |
| 10z13 | 设置即时生效：换热键旧键失效/新键生效、切引擎立刻可用；顺修 F13–F20 无法配置 | 完成（D-086，R-064） | `scripts/settings_live.sh`，冒烟 5e |
| 10z14 | 隐私模式八项验证：磁盘四处空 + 菜单不显示正文（顺修漏点 I-052） | 完成（D-087，R-065） | `scripts/privacy_check.sh`，冒烟 5f |
| 10z15 | 权限降级：单键失败给原因、组合键兜底、权限提示点名缺项 | 完成（D-088，R-066） | `scripts/degrade.sh`，冒烟 5g |
| 10z16 | 修 `--contract` 被单实例锁挡（I-053）+ longcap 延迟偶发重试；冒烟 26 项全绿 | 完成 | 冒烟日志 |
| 10z17 | 换设备即时生效（A→B 日志核对）；录音中拔麦保住已录内容（1.2s 断，仍出前半句） | 完成（D-089，R-067） | `device_switch.sh`、`mic_unplug.sh`，冒烟 5h/5i |
| 10z18 | 发版闸门：7 步（FAST）/11 步（全量）脚本化，dist 默认先过闸门；修 `--acceptance-synthetic` 挂死（I-054） | 完成（D-090） | `release_gate.sh`、`docs/RELEASE-CHECKLIST.md` |
| 10z19 | 0.3.0 发行：dist 全链路（闸门 7 项 → zip 8.7M → 解包验签/版本/无 /Users 路径/self-test/诊断）；清掉 3 条自测残留历史 | 完成 | `dist/Eryu-0.3.0.zip`、sha256 61e97043… |
| 10z20 | 四开关矩阵：门控优先于源锁（别人声音整段跳过）、单独源锁正常、标点开/关、诊断三项可读 | 完成（D-091，R-068） | `switch_matrix.sh`，冒烟 5j |
| 10z21 | 场景指引贴进设置页「音频」顶部（六条，与 USAGE 同口径） | 完成（D-092） | `docs/scenario-guide.png` |
| 10z22 | 局域网听写接口（M5 路线②）：`GET /health` + `POST /transcribe`，默认只听 127.0.0.1、可选 token、复用同一引擎、不上屏不写历史；设置页可配置 | 完成（D-093，R-069） | `scripts/lan.sh` 4 项、`core/tests/lan_smoke.rs` 2 项 |
| 10z23 | 局域网接口网页版：`GET /` 返回录音页（WebAudio 录 16k WAV + 选文件解码重采样 → POST → 结果/复制），页面注明 https 限制 | 完成（D-093 补，R-070） | `docs/lan-web.png`、lan 5 项 |
| 10z24 | 局域网安全默认：allow_lan 但无 token 拒绝启动（`lan_no_token`），lan 用例 7 项 | 完成（R-069b） | `scripts/lan.sh` |
| 10z25 | 0.4.0 发行：局域网接口 + 网页版 + 安全默认；dist 闸门 9 项 → zip 8.7M → 解包验签/版本/无 /Users/self-test | 完成 | `dist/Eryu-0.4.0.zip` sha256 f88a95b7… |
| 10z26 | 手机端可用路径：`--lan-info` 三种地址 + `docs/LAN.md` 三条路 + lan 用例 8 项 | 完成（R-071） | `--lan-info`、`scripts/lan.sh` |
| 10z27 | 局域网接口：请求日志（不记内容）+ 并发上限 2 → 503 busy；ABI v1.12 `eryu_lan_log`；诊断页显示最近 6 条；lan 用例 10 项 | 完成（D-096/R-072） | `core/src/lan.rs`、`--lan-log` |
| 10z28 | 0.5.0 发行：局域网可见性 + 并发上限；闸门 FAST 通过 → zip 9.2M → 解包验签/版本/`--lan-log`/self-test | 完成 | `dist/Eryu-0.5.0.zip` sha256 fec76e0e… |
| 10z29 | 安装唯一性：`scripts/install.sh`（装 /Applications + 注销并删项目副本）；未知 CLI 参数拒绝（`--version/--help` 正常） | 完成（D-097，R-073） | `lsregister -dump` 只剩 /Applications；`--versoin` → exit 2 |
| 10z30 | 权限自证（`/tmp/eryu-perm.json` + 菜单「权限状态…」 + 24h 去重弹窗）；润色预设/粘贴 Key/检测模型（ABI v1.13） | 完成（D-098，R-074–R-076） | `--perm-report` 在 `/Applications` 副本上三项已授权、`build/` 副本 AX/输入监控被拒 |
| 10z31 | 尾字补静音（D-099）+ 权限 2 秒实时复查（无需重启）；合成验收六项全绿 | 完成 | 六项检查输出：延迟 766ms、结果 29 字含「群里」 |
| 10z32 | 浮层毛玻璃 + 跟随插入点 + 最新文本 + 润色档图标（D-100）；短录音只预览 5s（D-101） | 完成 | 截图 docs/overlay-glass.png；`cargo test` 72 项全绿 |

状态：`未开始` / `进行中` / `待用户验收`（需要 TCC 点击或真实设备）/ `完成`。

## M1a 核心链路（当前阶段）

| # | 步骤（DESIGN §5.2） | 状态 | 证据 |
|---|---|---|---|
| 1 | 固定 bundle ID / 签名身份 / 安装路径 / 能力声明 | 完成 | D-002/D-003；`com.eryu.app` + ad-hoc（smoke 第 5 项） |
| 2 | 链接 arm64 staticlib：cargo + swift clean build、`otool -L`、启动、解一条 WAV | 完成 | `scripts/build.sh`；smoke 第 4 项无 `/Users` 依赖；`recognize_wav` 实测 |
| 3 | 事件复制 / 顺序 / 退订 / 停机；迟到回调与 worker panic | 完成 | `core/tests/ffi_smoke.rs`（含 panic 不穿 FFI）；smoke 第 1 项 |
| 4 | 已签名壳上验证热键按下/释放、同键冲突、TCC 拒绝与重建 | 待用户验收（需点击） | I-003；菜单「测试：录 3 秒并识别」可绕过热键验证其余链路 |
| 5 | 固定文字打通单次 paste、同 App 换框、Secure Event Input、菜单手动复制 | 完成（真实粘贴待用户验收） | `--self-test` 走通识别→submit_request→注入回调；`--inject-test` + `--verify-test` 实测粘贴与回读；Secure Input 下 keystroke 直接报 undelivered |
| 6 | 接 cpal + 就绪信号 + SenseVoice，短句端到端记延迟 | 完成 | `Eryu --mic-test 3` → `MICTEST OK 结果=…`；引擎加载 359ms，采音 48k→16k 重采样 |
| 7 | 会话 PCM + VAD + 限长 + flush + 段级重试 | 完成 | `core/src/vad.rs`（D-007 逐窗）、`pipeline.rs`、`audio.rs` cap 120s；55.7s/9 段实测 2.36s；段级重试见 D-040（ABI v1.7/v1.8） |
| 8 | 目标机 30 轮短句 + 60s/120s 实录验收 | 待用户验收 | I-008；需真实口述样本 |

### M1a 已验证的真机数字（2026-11-19，MacBook Air M5）

- 引擎加载（SenseVoice int8）：320–410ms（启动时预加载）
- 5.59s 语音：VAD 1 段，识别 257ms，RTF 0.046
- 55.73s 语音：VAD 9 段，总 2.36s，RTF 0.042，接缝不删字
- 采音：MacBook Air 麦克风 48kHz 单声道 → 有状态重采样 16k，4s 无丢帧
- 打包产物 `build/Eryu.app`：ad-hoc 签名，`--self-test` 与 `--mic-test` 均通过

## M1b（失败态 + 设置页 + 统计）——进行中

| # | 内容 | 状态 | 证据 |
|---|---|---|---|
| 1 | 配置持久化 + 首次运行写出默认 config.toml | 完成 | D-012；`Eryu --dump-config` |
| 2 | 历史（环形 200，0600 原子写）+ 统计（次数/字数/语音时长/省下打字） | 完成 | `core/src/store.rs` 5 单测；历史页截图显示 6 次 44 字 |
| 3 | 失败态：失败音频留 24h 可重试（`pending/last_failed.wav`） | 完成 | `asr_failed` + `retry_failed`；菜单「重试上次失败的识别」 |
| 4 | 失败态：静音/无声音归因（`no_audio` 指名设备；`empty_result` 分静音/太小） | 完成 | D-013；`audio::peak` + `SILENCE_PEAK` |
| 5 | 失败态：`too_short` 报实际时长与阈值 | 完成 | `too_short` 文案带 ms |
| 6 | 失败态：设备不在时列出可用设备（不静默回退默认） | 完成 | `audio::start_with` |
| 7 | 设备热拔插：录音中拔掉 → 保住已录部分继续识别 | 完成 | `Capture::has_failed` → `mic_unplugged` + 立即收尾（M1c 第 7 项） |
| 8 | 设置页五个分页（通用/引擎/音频/历史与统计/诊断） | 完成 | 四张截图 `/tmp/eryu-tab-*.png`；UI 目标稿见 DESIGN §5.1 |
| 9 | 引擎热切换（后台重建，下次录音生效）+ 未安装标橙 | 完成 | `engine_ready` / `engine_switch_failed` / `model_missing` |
| 10 | 麦克风电平测试（meter 事件 ~15Hz，仅 rms/peak） | 完成 | `eryu_meter_start/stop`；静音提示文案 |
| 11 | 快捷键录制 + 冲突原因枚举 | 完成（真实按下待用户验收） | `HotkeySpec` + `Hotkey.describe`；I-003 |
| 12 | 历史单项复制 / 单项删除 / 清空（带数量确认） | 完成 | 历史页截图 |
| 13 | 诊断页（可复制、可打开系统设置、可编辑 config.toml） | 完成 | `eryu_diagnose` + `setConfigToml` |
| 14 | CLI：`--diagnose` / `--dump-config` | 完成 | 已实测输出 |
| 15 | 开机启动（SMAppService） | 完成（ad-hoc 下可能被拒，见 I-009） | `LoginItem.swift` |
| 16 | 真实按住热键→说→松开→上屏（需 TCC 授权） | 待用户验收 | I-003/I-010 |

### M1b 已验证

- `cargo test`：15 单测 + 2 FFI 冒烟全绿；`scripts/smoke.sh` 5 项全绿（符号表含全部 v1.1 函数）
- `Eryu --mic-test 3`：真实麦克风→SenseVoice→result→submit_request，`MICTEST OK`
- 设置窗口四个分页均渲染正常（截图核对：通用/引擎/音频/历史与统计/诊断）
- 首次运行自动写出 `~/.eryu/config.toml`（679 字节以下，0600）

## M1c（权限兜底 + 失败态收尾）——完成（8/8，唯一待用户动作=真实按键验收）

| # | 内容 | 状态 | 证据 |
|---|---|---|---|
| 1 | 权限分三项独立体检 + 启动只报缺项 | 完成 | D-015；`Permission.missingSummary()` |
| 2 | 诊断页权限矩阵（状态 / 缺了会怎样 / 去设置 / 麦克风主动请求） | 完成 | `PermissionMatrix`；截图核对 |
| 3 | 承诺式权限文案（Info.plist usage description 写明「只在本机处理，不上传」） | 完成 | `packaging/Info.plist` |
| 4 | `eryu diagnose` 等价位 CLI | 完成 | `Eryu --diagnose` |
| 5 | 单实例锁 | 完成 | `InstanceLock`（flock，`--self-test` 不占锁） |
| 6 | 崩溃 pending_result 落盘 + 启动恢复 + 取回后清暂存 | 完成 | D-014；`--print-last` 实测读回「这是一条崩溃前没确认上屏的测试文本」 |
| 7 | 设备热拔插：录音中流报错 → `mic_unplugged` + 保住已录部分立即收尾 | 完成（待真机拔设备验证） | `Capture::has_failed` + `tick()` 检测 |
| 8 | TCC 跨重建保持：build.sh 优先用稳定身份「Eryu Dev」，缺失时回退 ad-hoc 并提示 | 完成 | D-018 已建证书；`codesign -dvvv` 显示 `Authority=Eryu Dev`；smoke 第 5 项核对 |

## M2——完成

| # | 内容 | 状态 | 证据 |
|---|---|---|---|
| 1 | 热词表：core 单遍最长匹配 + 英文词边界 + 文件热加载 | 完成 | D-016；7 个单测；`recognize_wav` 实测「深度求索 → DeepSeek」 |
| 2 | 热词管理 UI（设置页「热词」分页：增删改 + ⌘S 保存 + 未保存提示） | 完成 | `HotwordsSection`；ABI v1.2 |
| 3 | 菜单栏「最近：…」转写预览 | 完成 | `StatusController.setRecent` |
| 4 | 热词自动学习（注入后编辑追踪 → 待采纳候选） | 完成（`EditObserver`；`--edit-observe-test` 学到「深度求索 → DeepSeek」） | D-026 |
| 5 | 选区指令管线（选中文字 → 指令改写 → 就地替换） | 完成（`SelectionLane` + `CommandPalette`；ABI v1.4 `eryu_run_command`；`--selection-test` 实测读 20 字 / 写回 OK） | D-024 / D-025 |
| 6 | 部分成功 UI（段级失败可见 + 只上屏成功段） | 完成（浮层显示「有段落没识别成功，已上屏其余部分 · ⌘R 重试」；历史保留可重试） | `/tmp/eryu-tab-*.png` |
| 7 | 润色（分级 + context 槽位） | 完成（四档 + `<CONTEXT_INFORMATION>` + 失败回退原文 + 首次披露；ABI v1.3 `eryu_polish_test`） | D-012 后续 / DESIGN §9.6 |
| 8 | 隐私模式 | 完成（D-023；实测 40→40 条不写历史、pending 不落盘） | `privacy_tests` 2 个单测 |
| 9 | 静音其他应用（录音时暂停 Music/Spotify） | 完成（逻辑就绪，真实播放未验；I-021） | `MediaPause` |

## M2 状态（2026-11-19 更新）

| 项 | 状态 |
|---|---|
| 热词表（core 最长匹配 + 词边界） | 完成 |
| 设置页热词分页 | 完成 |
| 菜单最近转写预览 | 完成 |
| 录音浮层（底部胶囊 + 电平 + 准流式文本） | 完成（纯 AppKit，见 I-017） |
| 单键热键（右 Option / Fn / F13 / 裸键） | 完成（CGEventTap listenOnly） |
| 润色（四档 + `<CONTEXT_INFORMATION>` + 失败回退） | 完成（core + 设置页 + 首次披露） |
| 稳定签名身份 Eryu Dev | 完成（TCC 跨重建保持，D-018） |
| 部分成功 UI（浮层提示 + ⌘R 重试） | 完成 |
| 热词自动学习（编辑追踪 → 候选 → 采纳） | 完成（`--edit-observe-test` 通过） |
| 选区指令管线（⌘E → 5 条指令 → AX 就地替换） | 完成（`--demo-palette` 截图 + `--selection-test` 读写通过） |
| 隐私模式（不写历史 / 不落盘 / 浮层脱敏） | 完成（D-023，2 个单测 + 实测） |

### M3（进行中）

| # | 项 | 状态 | 备注 |
|---|---|---|---|
| 1 | 上屏方式：keystroke（逐字打字） | 完成（事件构造自测通过；真实投递待用户授权后验收 I-022） | `Injector.injectKeystroke`，D-027 |
| 2 | 上屏方式按应用覆盖 | 完成（core `mode_for` + 设置页「按应用覆盖」） | `[insert] per_app` |
| 2b | 注入后校验（读到输入框文本才报 ok） | 完成（`--verify-test` 命中=ok / 未命中=unknown） | D-028 |
| 3 | 应用内模型下载（DESIGN §9.2） | 完成（`eryu_models_pull` ABI v1.5；curl 下载 + tar 解包 + 进度事件；实测 sense 158MB→247MB 用时 35s） | D-029 / I-024 |
| 4 | Windows 端口 | 待做 | DESIGN §3 |
| 5 | Apple Speech 引擎（macOS 原生可选） | 待做 | DESIGN §4.2 |
| 6 | 手机端 | 远期 | DESIGN §9.1 |

### M4（进行中）——体验与工程收敛

| # | 项 | 状态 | 备注 |
|---|---|---|---|
| 1 | 背景降噪（RNNoise，采集侧 48k） | 完成 | D-035；0dB 白噪声下救回开头几个字，−5dB 有好有坏，文案如实 |
| 2 | 润色 profile（名称/力度/附加要求/快捷键槽） | 完成 | D-036/D-037；三档内置，⌃⌥1/2/3 切换 |
| 3 | 延迟可观测与验收口径 | 完成 | D-038；`--bench` p50/p95，菜单显示上次耗时 |
| 4 | 句末标点兜底 | 完成 | D-046；不做段间逗号（I-031 标点模型待定） |
| 5 | 浮层跟随光标屏幕 + 已录秒数 | 完成 | D-041/D-044 |
| 6 | 连按应急（双击取回上次结果） | 完成 | D-043 |
| 7 | 部分成功单段重试 | 完成 | D-040；`--decode-wav` + `ERYU_FAULT_FAIL_SEGMENTS` 端到端 |
| 8 | 热词候选落盘 | 完成 | D-048 |
| 8c | 资源基线进冒烟（冷启动<3s、RSS<1200MB） | 完成 | `smoke.sh` 第 4b 项，实测 394ms/569MB |
| 8b | 校验未确认时保留剪贴板（不吞字） | 完成（逻辑就绪，真机计时未验 I-033） | D-049 |
| 9 | 音源锁定实验（能量竞争 + 粘滞） | 部分完成：先做了**声纹门控**（只录我的声音） | D-051/D-052，R-036 |
| 10f | 渐进封口：长听写松手只解尾巴（D-059） | 完成 | 75s→0.6–1.1s、103s→727ms、19s→912ms（改造前 2.0s/7.7s） |
| 10e | 模拟麦克风 + --simulate 全链路（D-058） | 完成 | 冒烟第 3d 项；5.5s→385ms、8.5s→648–764ms |
| 10 | 自测模式不污染历史（D-055，ABI v1.11） | 完成 | 冒烟第 2c 项 |
| 10b | 长跑基线取第一轮之后（D-056） | 完成 | 标点模型懒加载不再被当成泄漏 |
| 10d | 自测绝不上屏（I-035） | 完成 | CLI 路径强制 dryRun + testMode |
| 10c | 历史搜索 + 退出并重新打开（D-057） | 完成 | 历史页搜索框；菜单 ⌘⇧R |
| 9d | 补标点（可选 65MB 模型，D-054） | 完成 | `core/src/punct.rs` + `config.punct` + 设置页开关/下载 + `--diagnose` 一行 |
| 9c | 长跑自测 --soak（内存/延迟漂移） | 完成 | D-053；15 次实测 RSS 567→300MB，冒烟第 3c 项 |
| 9b | 只录我的声音（声纹门控） | 完成（同人 0.92–0.93 / 异人 0.49–0.57 实测；端到端 `--decode-wav` 验证跳过） | 模型 39MB，ABI v1.9/v1.10 |
| 10 | PureVox6 强降噪 | 未开始 | 模型约数百 MB，需权衡 |
| 11 | Windows 端口 | 未开始（本机无法验证，见 docs/WINDOWS.md） | I-028 |
| 12 | Apple Speech 原生引擎 | 未开始 | DESIGN §4.2 |
| 10z33 | 「润色开关」与「润色档」分工（D-102）：去掉全局力度，力度只听当前档 | 完成 | 老配置迁移单测；设置页截图 |
| 10z34 | 浮层定稿：胶囊/宽度跟字/提示在上/档位图标可配（D-103） | 完成 | 截图对照；`cargo test` 73 项 |
| 10z35 | 浮层修正（D-104）：单一闪动省略号、提示移到下方、宽度只增+台阶、位置锁定 | 完成 | 截图三帧对照 |
| 10z36 | 胶囊宽度封顶 + 头部截断 + 底色压深（D-105） | 完成 | 47 字输入实测 bounds=420、显示尾部 |
| 10z37 | 胶囊宽度动画 + 电平条改橙 + 档位图标放大（D-106） | 完成 | 截图与代码核对 |
| 10z38 | 胶囊实色化 + 电平条浅橘黄加大 + 档位图标放大（D-107） | 完成 | 截图取色：#404048 胶囊 / #F8D088 电平条 |
| W0 | Windows 计划定稿（D-108，docs/WINDOWS.md 重写为一比一清单 + 四层判定 + W0–W4） | 完成（计划） | 需 Windows 机器才能开工，I-028 |

### Windows（W1 原生预览，main 之外的并行线）

| # | 项 | 状态 |
|---|----|------|
| 1 | core 在 Windows 原生编译（MSVC + sherpa win-x64 静态库） | 完成 |
| 2 | WASAPI 采音 + SenseVoice 识别 + 剪贴板粘贴上屏 | 完成（真机） |
| 3 | 分层窗口浮层（圆角/电平条/中文/预乘 alpha） | 完成（截图） |
| 4 | Raw Input 右 Alt 按住说话、Esc 取消 | 完成（真机） |
| 5 | 托盘图标 + 菜单、首次启动自动拉模型（断点续传） | 完成 |
| 6 | 浮层跟随输入光标（`GetGUIThreadInfo` + 翻转/夹紧） | 完成（真机，2026-11-19） |
| 7 | 胶囊宽度按实测字形宽度、超长保留尾部最新文字 | 完成 |
| 8 | 逐字上屏（WM_CHAR 逐字投递） | 完成（真机，2026-11-19） |
| 9 | 热键可配置（单键/组合键/F 键，从 core 配置读，WH_KEYBOARD_LL） | 完成（真机，2026-11-19） |
| 10 | 按应用覆盖（`--as-app` + `mode_for` 实测 mode=paste/keystroke） | 完成（真机，2026-11-19） |
| 11 | 设置窗口按 `design/settings.png` 深色重画（7 页：通用/引擎/音频/热词/历史/润色/诊断；自绘导航/按钮/开关/列表/焦点框；`WM_DPICHANGED` + 150%/200% 验证） | 完成（真机截图，2026-11-20） |
| 12 | 历史统计页 / 润色档页 / 隐私开关页 | 完成（W4） |
| 13 | 开机启动（HKCU Run 键，启动对齐 + 设置页即时生效） | 完成（真机，2026-11-20） |
| 14 | 安装包（`安装.ps1`：装到 `%LOCALAPPDATA%`、开始菜单快捷方式、可选自启） | 完成（W4 交付包） |
| 15 | 按应用覆盖端到端（真实录音） | 未验（该机无麦克风，I-074） |
| 16 | 注入后校验（UIA）/ 待复制卡片 / 选区改写 / 暂停媒体 | 未做（W5） |

| 11 | 长听写延迟（I-071）：预览窗口封顶 5s、提交块 10s→4s、预览线程单飞、模拟不再克隆整段 | ✅ 76s 语料 3.2s→1.9–2.7s；仍 >1.5s 目标，下一步见 I-071 |

### 0.7.0（进行中）

| # | 项目 | 状态 |
|---|---|---|
| 0.7a | 新用户引导四步（模型/按键/上屏/润色） | ✅ 截图 `docs/onboarding-1..4-*.png` |
| 0.7b | 模型下载源可配（`[models] mirror`） | ✅ core + macOS 设置页；Windows 待加 |
| 0.7c | Windows 视觉修正（Segoe UI / 5 根条 / 档位图标 / logo） | ✅ W5 已发 |

### Windows W7（2026-11-20）

| # | 事项 | 状态 |
|---|---|---|
| 1 | 选区改写 Ctrl+Alt+E（指令面板 + `eryu_run_command`） | 完成（VM 截图） |
| 2 | 注入后校验（读焦控件，不二次注入） | 完成（代码路径 + 设置页） |
| 3 | 录音时暂停系统播放（GSMTC） | 完成（未做真实播放验证） |
| 4 | 润色档快捷键槽 + 设置页编辑 | 完成 |
| 5 | 重识别失败的段（托盘子菜单） | 完成 |
| 6 | 首次引导四步 | 完成（W6 起） |
| 7 | 模型下载镜像 | 完成（core 默认 + VM 验证） |
| 8 | core 交叉编译（cargo-xwin + llvm-lib） | 完成 |
| 9 | 待复制卡片 | 未做 |
| 10 | 真实录音链路（VM 声卡可用） | 完成（VM 内 `--mic-test` 真实识别，I-080 关闭；I-103 另记） |

### Windows W8（2026-11-20）

| # | 事项 | 状态 |
|---|---|---|
| 1 | 绿色单文件 exe（dll 内嵌 + 运行时解出） | 完成（VM 实测单文件可跑） |
| 2 | 便携模式（exe 旁 `portable.txt` / `EryuData`） | 完成（VM 验过配置落盘位置） |
| 3 | `scripts/build-win.sh` | 完成 |
| 4 | exe 安装包（IExpress/Inno 一类） | 未做（绿色版已覆盖需求） |

### Windows 可用性（2026-11-20，进行中；用户要求：能用、不死机、与 macOS UI/能力对齐）
| 33 | 上屏假成功（I-112）：前置守卫 + 诚实状态 + 剪贴板兜底 | ✅ 2026-11-21 |

| # | 事项 | 状态 |
|---|---|---|
| 1 | 一次上屏后整个程序死锁卡死（I-104） | 完成（VM 回归：Notepad 真出字、进程 Responding=True） |
| 2 | 钩子回调里跑阻塞逻辑（selection/模态框/每键写日志，键盘卡死 + 被摘钩，I-104 审计 P0-1） | 完成（钩子只 `PostMessageW` 到消息窗口，逻辑移到 wndproc `WM_ERYU_KEY`） |
| 3 | 持 APP 锁弹模态框自死锁（model_failed / 托盘诊断） | 完成（`defer_message_box`：出锁后 150ms 再弹；托盘两项先取值再弹） |
| 4 | `inject.rs` 用无超时 `SendMessageW(WM_GETTEXT)` 打跨进程窗口 | 完成（`SendMessageTimeoutW` 300ms，读不到按 ok） |
| 5 | 设置页每帧泄漏 GDI 画刷 + keycap 状态不释放（P0-4/5） | 完成（画刷用完即删；keycap `WM_NCDESTROY` 回收 State+字体） |
| 6 | 托盘永不 NIM_MODIFY（状态/下载进度/失败提示不刷新） | 完成（`set_tip` 发 NIM_MODIFY；退出时 NIM_DELETE + Unhook） |
| 7 | 能力对齐缺口（按应用覆盖 / 声纹 / LAN / 补标点 / 电平测试 / 历史条目操作 / 诊断 / 引导试说 / 迷你窗） | 进行中（详见下「能力对齐」） |
| 9 | 设置页历史：记录开关、删除选中、重试失败的段（对齐 mac 历史页） | 完成：`--history-delete <id>` 自测 rc=0、3 条→2 条；截图 `docs/settings-win-history.png` |
| 13 | 关于框版本号；caret 探针 flip 判定与实际浮层翻转同口径 | 完成（`PRODUCT_VERSION` 常量；caret `near_bottom()`） |
| 14 | 声纹页（只认我的声音）：开关 + 已登记状态 + 录 8 秒登记 + 清空（对齐 mac 声纹） | 完成：`--voice-enroll-wav` 走真实 FFI（VM 结果 `rc=0`、`dim=512`、重启后状态仍在）；「录 8 秒登记」在后台线程跑、完成后 PostMessage 回设置页重建（不在 UI 线程卡 8 秒）；截图 `docs/settings-win-voice.png` |
| 15 | 润色页「测试连接」（对齐 mac 的连通性自测）、通用页底部布局不再重叠 | 完成：`--polish-test` 走真实 FFI，没填 Key 时返回「没有填 API Key」（不崩不卡）；测试连接在后台线程跑（同步 HTTP 不在 UI 线程），完成后回设置页刷新；截图 `docs/settings-win-polish.png` |
| 16 | 热词页「建议（从历史里挑的高频词）」：列高频词 + 一键加进热词表 + 刷新 | 完成：纯逻辑自测 `--hotwords-suggest-selftest` → `SUGSEL 设计文档 x3`（重叠碎片已去重）、`--hotwords-suggest` 走真实历史路径；截图 `docs/settings-win-hotwords.png` |
| 17 | 睡眠唤醒后热键失效（钩子被系统摘掉） | 完成：`WM_POWERBROADCAST`（PBT_APMRESUMEAUTOMATIC/SUSPEND）时重装键盘钩子 + 重注册 raw input，并打日志便于判断 |
| 18 | 自测/自动化模式会弹模态框卡死脚本；设置页逐页回归 | 完成：`QUIET` 开关（所有 `--xxx` 自测开关生效，弹框改成打日志）；新增 `--settings-pages-smoke` 逐页打开 8 页并泵消息，VM 输出 `PAGES page=0..7 ok / done`，`--self-test` 仍 OK |
| 19 | 真实热键（F13）+ 真实麦克风整链路 | 完成：`hook vk=0x7c → handle matched → hotkey_down app=Notepad.exe`，Notepad 出现「今天天气不错我们开始吧。」；证据 `docs/win-e2e-real.png`、`docs/win-parity.md`「真实热键端到端」 |
| 20 | 迷你主窗（对齐 mac R-103） | 完成：170×230 无边框置顶小窗，大圆按钮点击开始/再点停止、`按 <热键> 或点击开始`、今日已节省、✕ 关闭写 `mini_window=false`、托盘「显示主窗」恢复；录音中变红显示停止方块；证据 `docs/mini-window-win.png`、`docs/mini-window-win-rec.png` |
| 21 | 局域网听写页（对齐 mac） | 完成（I-107 已解决）：VM 里设置页点开 + 保存后监听 `127.0.0.1:8765`，`POST /transcribe` 返回 `{"ok":true,"text":"今天天气不错我们开始吧","ms":298}`；核心补齐「改配置即重启监听」 |
| 22 | 发布打包 + 全页冒烟 | `scripts/package-win.sh` 产出 `dist-win/Eryu-0.8.1-win-x64.zip`（exe + 一页说明）；`--settings-pages-smoke` 覆盖全部 9 页（含局域网）；新增 `--lan-chk` 配置诊断 |
| 23 | 局域网改配置即重启监听（I-107 收尾） | 完成：`core/src/lan.rs` 可停止 accept 循环 + `Worker::apply_config` 比较 `lan_*` 后重启；mac 回归 `lsof` 见 `127.0.0.1:8766 LISTEN`（隔离配置 `[lan] enabled=true`），Windows VM `POST /transcribe` 返回识别文本 |
| 24 | 引导「先确认麦克风」页（对齐 mac R-102） | 完成：6 步引导（模型/麦克风/按键/上屏/试说/润色）；麦克风页实时电平（core meter 事件转发 `onboarding::on_meter`）+ 当前输入设备 + dB；证据 `docs/onboarding-win-mic.png`（说话时约 -22 dB、电平条填充） |
| 25 | 音频页模型状态提示 | 完成：降噪/补标点开关文案带「模型未下载：去引擎页下载 gtcrn（1MB）/ punct（约 65MB）」，不再出现「开了没效果」；截图 `docs/settings-win-audio.png` |
| 26 | 修崩溃：中文错误事件导致进程 abort（I-108） | 完成：字节截断改字符截断 + 事件分发 `catch_unwind`；VM 真实右 Alt + TTS 回归通过（Notepad 出字、`Responding=True`），证据 `docs/win-e2e-real.png` |
| 12 | 通用页「按应用覆盖上屏方式」（每行 `进程名=粘贴/打字`，对齐 mac 的按应用覆盖） | 完成：`--settings-perapp-selftest` 走真实保存路径，写入 `{"chrome.exe":"paste","notepad.exe":"keystroke"}`，注释行忽略 |
| 11 | 音频页补「句末补标点」「音源锁定」两个开关；引擎页下载进度；诊断页「运行自检」「麦克风隐私设置」 | 完成（`--settings-mic-test N` 自测：电平条有起伏、按钮变「停止测试」；截图 `docs/settings-win-mic-level.png`） |
| 10 | 自绘开关状态不生效（I-106：所有 pill 开关永远「关」、保存写错值） | 完成（`TOGGLES` 自持状态；截图同 9） |
| 8 | 引导「试说一句」（R-101 对齐） | 完成：第 4 步真录一句、判定与 mac 完全一致（`--onboarding-probe-selftest` 8 条 OK）、没读到字时「下一步」灰且不前进、通过后写 `onboarding_probe_done`、迁移用户（done 未 probe）启动回本步；VM 证据 `docs/onboarding-win-probe.png` / `docs/onboarding-win-probe-ok.png` |
| 27 | 界面重做（按 stitch 设计稿 + Windows 规范）：无边框圆角窗、自绘顶栏、统一字体/圆角/行式布局；迷你窗同套设计；胶囊波形跟音量 | 完成：`shell-win/src/ui.rs`（字号/DPI/字体统一）、设置页行式布局与圆角控件、迷你窗 MDL2 图标与两行提示；证据 `docs/settings-win-general.png`、`settings-win-engine.png`、`settings-win-audio.png`、`mini-window-win.png`；mac 侧 `Overlay.swift` 波形公式同步并重建通过 |
| 28 | 九页界面逐页验收 + 修热词页打开即崩溃（I-109） | 完成：全部 9 页按新布局截图（`docs/settings-win-{general,engine,audio,hotwords,history,polish,diagnose,voice,lan}.png`）；I-109 修法后热词页可打开，建议词改静态展示 |
| 39 | 界面文案清理与关闭按钮 | 完成：Windows 设置页删掉 12 条解释性小字、补回右上角 ✕；mac 设置窗关闭后回菜单栏；胶囊波形柱高统一上限 20pt；mac 润色档选中行与编辑区合并成一张卡片 |
| 38 | 热词自动学习（EditObserver 的 Windows 版） | 完成：`observer.rs` 上屏后 1s 一次读焦点控件（带超时的 WM_GETTEXT），15s 内发现文字变化就按公共前后缀算出「原词→新词」；`suggestions.json` 存候选；热词页「学到的改法」可采纳/忽略；VM 实测采纳后 `hotwords.toml` 写入 `qwen = "通义"`、候选清空 |
| 37 | 开机自启对齐（Run 注册表 ↔ 配置）+ 光标回退翻转 | 完成：`--autostart-on/off` 同步写注册表与 `launch_at_login`；VM 实测 ON→注册表有 `Eryu`、配置 true；OFF→两者都清。`caret.rs` 鼠标回退也按 `near_bottom` 翻转 |
| 36 | 单修饰键热键左右互认（I-110）+ `--set-hotkey` | 完成：`same_modifier` + 补单测；VM 里 `raw.log` matched=true → 采音 → 上屏 status=ok |
| 35 | Windows 安装/卸载脚本 + 分发包 | 完成：`install-win.ps1`（UTF-8 BOM+CRLF，PS5.1 才认中文）、`uninstall-win.ps1`、`package-win.sh` → `dist-win/Eryu-0.8.1-win64.zip`（exe + 两个脚本 + README）；VM 实测 `INSTALL OK` → 快捷方式指向 `%LOCALAPPDATA%\Programs\Eryu\Eryu.exe` → `--self-test` 出 `SELFTEST OK`；`uninstall.ps1` 后程序目录与快捷方式删除、数据保留 |
| 34 | 选区改写浮层统一观感 | 完成：`selection.rs` 窗口 12px 圆角 region、字体改 `Microsoft YaHei UI`、选中行 `RoundRect` 圆角 + 柔和底色 `0x553A2B`；证据 `docs/settings-win-selection.png` |
| 33 | 引导窗统一到新界面 | 完成：`onboarding.rs` 改 `WS_POPUP|WS_CLIPCHILDREN` 640×536、圆角 16、自绘顶栏（Eryu + ✕，`WM_NCHITTEST` 拖动）、`child()` 统一 +HEADER；顺手修 `WM_DRAWITEM` 与按钮文案里 `step == 3/4` 的旧步号（导致试说页显示「开始使用」且不禁用）；证据 `docs/onboarding-win-model.png`、`docs/onboarding-win-mic.png`、`docs/onboarding-win-probe.png` |
| 32 | 降噪模型缺失时静默跳过 | 完成：`core/src/lib.rs` 两处 `start_with_opts` 的降噪路径加 `.filter(\|p\| p.exists())`；`cargo test` 全过、mac/Win 均重建通过 |
| 31 | 托盘菜单补齐（测试麦克风 / 麦克风权限设置） | 完成：`CMD_MICTEST=1010` 录 3 秒给结果、`CMD_PERM=1011` 打开 `ms-settings:privacy-microphone`；`--tray-menu-dump` 输出 13 项与 mac 对齐 |
| 53 | `--set-config` 落盘竞态 | 修复：分支里等 1200ms 让 worker `apply_config`+`save`。VM 实测 `onboarding_done=true`/`lan_enabled=true` 都被写进 `config.toml`，`netstat` 出现 8765 监听 |
| 57 | 安装体验（从 zip 冷装） | 完成：VM 里先跑 `uninstall-win.ps1`（程序目录删除、数据保留）→ 解压 zip → `install-win.ps1 -Exe …` → 安装目录哈希与包内一致、开始菜单快捷方式存在、`--self-test` 通过、启动后 `Responding=True` |
| 56 | Windows 引导全流程实测 | 完成：第 0 步点「稍后再说」→ `done=true/probe=false`；重启回到「试说一句」（下一步灰）；`ERYU_MIC_WAV`+`--hotkey-sim` 出字后下一步变蓝 → 点进润色 → 「开始使用」→ `done=true/probe=true`；再重启不再弹引导、迷你窗在 |
| 55 | 托盘「自测…」 | 补菜单项（`CMD_SELFTEST=1014`）+ `selftest_async`：另起 `--self-test` 进程，读回最后一行结果再弹框；`--tray-menu-dump` 现在 15 项 |
| 54 | 引导试说页两处（I-114） | 修复：`offer_result`/`note_partial`/`note_state` 的步号字面量 3 → `STEP_PROBE`；「下一步」owner-draw 子按钮补 `InvalidateRect`。VM 复验：日志只有 `onboarding probe ok=true len=12`、无上屏；截图 `docs/onboarding-win-probe-ok.png` |
| 53 | --set-config 落盘竞态 | 修复：分支里等 1200ms 让 worker apply_config+save。VM 实测 onboarding_done=true / lan_enabled=true 都写进了 config.toml，netstat 出现 8765 监听 |
| 52 | 开机自启路径核对 | 完成：`autostart::reconcile` 在 Run 键存在时也比较路径，不一致就重写。VM 实测：`Eryu-as.exe --autostart-on` 写入；复制成 `Eryu-as2.exe` 再跑 → `AUTOSTART 路径变了，已重写 Run 键`、注册表改为 as2；`--autostart-off` 后键被删除 |
| 51 | 0.8.1 Windows 收尾回归 | 完成：装到 `%LOCALAPPDATA%\Programs\Eryu` 的构建跑全量自查——`SELFTEST OK`、`PAGES done`、8 条 `PROBE OK`、托盘 14 项、`--perm-report` 麦克风=允许、`--polish-models` 错误路径正常；端到端（`--hotkey-sim` + `ERYU_MIC_WAV`）记事本出现「今天天气不错我们开始。」、进程 `Responding=True`。mac：`build.sh` + `smoke.sh` 全绿 |
| 50 | 引擎页附加模型下载 | 完成：core `models_list_json` 增 `punct/gtcrn/speaker`（带 `"engine": false`），Windows 列表标「附加模型 · 未安装（MB）」，`使用选中模型` 对附加模型只提示去下载。VM 实测选中标点模型点下载 → 进度 `0.4MB / 65.0MB` 起跑 |
| 55 | 自测污染历史 + 历史列表排版 | 完成：`--self-test` 先 `core.set_test_mode(true)`（core 的 `eryu_set_test_mode` 之前没绑），3 次自测后历史「自测」行数不变（17→17）；`listbox()` 固定行高 32px、历史列表 288 高、`text_at` 加 `DT_END_ELLIPSIS`（I-118 / I-119） |
| 54 | 双击安装脚本 | 完成：包内新增 `install-win.bat` / `uninstall-win.bat`（ASCII 文案、内部 Bypass 执行策略），README 重写；VM 实测解压→双击安装→`INSTALL OK`+快捷方式+`--self-test`，卸载→`GONE`，再装→`PRESENT` |
| 53 | 设置页重建句柄泄漏 | 完成：`build()` 兜底 `GetWindow(GW_CHILD)` 逐个销毁漏登记子窗口；新增 `--settings-cycle N` 自测。VM 同进程 200 次重建：修前 +23 GDI / +103 USER，修后 -1 / -9；9 页冒烟仍全 ok（I-117） |
| 52 | 热词建议质量修正 | 完成：`suggest_from` 加边界熵（左右邻字种类之和 ≥3，句首句尾不计）+ 两字功能词停用表。VM 真实历史 79 条：修前 `们开始 x56 / 我们开始 x55`，修后只剩 `测试 / 怎么样 / 的这个`；`--hotwords-suggest-selftest` 仍 `设计文档 x3`（I-116） |
| 51 | 胶囊观感修正（真机反馈） | 完成：档位徽标改 MDL2 铅笔图标（不再画档名首字「日」）；正文 14→17.5px、图标 18→22、胶囊 40→36 高；提示行改「白色绘制 + 按主题染色 + 预乘 alpha」，浅色壁纸可见。VM 100%/150% 截图 + 真实录音态 `docs/overlay-win-rec.png`（I-115） |
| 49 | 润色首次同意弹窗 | 完成：`polish_consent()`（`message_box_yesno`，QUIET 下自动通过）；拦截 `ID_POLISH_EN` 打开与「用选中的档」，确认后立刻 `save_polish()`。VM 实测：点开关 → 弹窗 → 选「否」`enabled=false`；再点 → 选「是」`enabled=true`；再点关闭回 `false` |
| 48 | 托盘「权限状态…」 | 完成：`CMD_PERM_STATUS=1013` + `perm_report_text()`（麦克风 ConsentStore 状态 / 键盘监听不需授权 / UIPI 提醒），`--perm-report` 自测输出 `麦克风：允许`；托盘菜单 14 项 |
| 47 | 润色页「检测模型」 | 完成：`ID_POLISH_MODELS=266` + `polish_models_async()`（后台线程 + `WM_POLISH_MODELS_DONE` 回刷新）；core 侧 `polish_models()` / `polish_models_raw()`。VM 验错误路径 `POLISHMODELS n=0 err=还没有 Key…`，成功路径与 `--polish-test` 共用核心接口 |
| 46 | 润色档指令编辑 | 完成：`ID_POLISH_PROMPT` 多行编辑框 + 「保存指令」→ `set_profiles`；`--polish-prompt-selftest` 实测 `PROMPTSEL profile=✓ 日常 prompt=…` / `PROMPTSEL OK`；截图 `docs/settings-win-polish.png` |
| 45 | 选区改写缺陷（P1-8/9 + #24） | 完成：粘回前 `SetForegroundWindow(prev)`，粘完 900ms 还原原剪贴板；`WM_ACTIVATE` 失焦即收起；`hide()` 改 `PostMessageW(WM_CLOSE)`，销毁回到 wndproc；两处剪贴板读取按 `GlobalSize/2` 封顶。VM 验证：面板打开后切前台 → 窗口自动消失、进程无异常 |
| 44 | 上屏后停留 | 完成：`overlay.show("idle")` 改为延迟 0.9 秒隐藏（`HIDE_GEN` 防串场），期间显示最后一句、提示行收起；`--overlay-demo --overlay-phase` 可复现各阶段；证据 `docs/overlay-win-inserting.png` |
| 43 | 回归（2026-11-21） | macOS `scripts/build.sh` + `scripts/smoke.sh` 全绿（含 ABI/错误码/文档一致性/局域网/极端状态）；Windows 安装目录 `--self-test` → `SELFTEST OK` / `EXIT=0`；`--settings-pages-smoke` → `PAGES page=0..8 ok` |
| 42 | 胶囊录音态 | 完成：`icon_glyph` 只取档名首字（CJK 才画），`HINT_H` 12→17 让提示行完整；WAV 夹具实拍 `docs/overlay-win-rec.png`、迷你窗 `docs/mini-window-win-rec.png` |
| 41 | 高分屏适配 | 完成：`ui::fit_scale(k, 900, 632)` 在 125%/150% 下按工作区收比例，`ERYU_SETTINGS_DPI=125` 实测窗口 994×698 完整落在 1470×770 内；证据 `docs/settings-win-dpi125.png` |
| 40 | 引导窗逐页复看 | 完成：上屏页改分段按钮、润色页按钮跟随状态、快捷键页显示真实键位（`hotkey_hint()`）、`advance()` 不再从已删除的下拉框读值；证据 `docs/onboarding-win-{model,hotkey,insert,polish}.png`（各 640×534） |
| 39 | 引导窗居中 + 麦克风页文案 | 完成：`CreateWindowExW` 用工作区居中坐标（原 `CW_USEDEFAULT` 丢左上角）、说明收成两行；证据 `docs/onboarding-win-mic.png`（640×534）、`docs/onboarding-win-probe.png` |
| 38 | 诊断页权限行收 ⓘ | 完成：`系统麦克风权限：允许` + ⓘ（设置路径/被拒后果）；证据 `docs/settings-win-diagnose.png`；同轮跑 `--hotkey-sim --hotkey-sim-force` 确认上屏链路仍通（无前台目标时 `status=unknown`，符合预期） |
| 37 | 音频页 ⓘ 收尾 | 完成：四行开关改短标签 + ⓘ（模型缺失只在标签留「（模型未下载）」）、电平提示改「点「开始测试」看有没有声音」；证据 `docs/settings-win-page2.png`、局域网页 `docs/settings-win-page8.png` |
| 36 | ⓘ 悬停提示可用 | 完成：`info()` 热区 + `paint()` 画气泡，气泡优先放图标右侧（下方是子控件会被盖住），四处说明收进 ⓘ；证据 `docs/settings-win-tooltip.png` |
| 35 | 上屏链路打通（I-113 已解决） | 完成：钩子放过 `LLKHF_INJECTED`（热键只认人手键）、注入改 UI 线程队列执行、UIA `ValuePattern` 优先 + `Ctrl+V`/`WM_PASTE` 兜底、迷你窗去掉 `WS_VISIBLE` 不再抢前台；VM 验证 `上屏 status=ok` + 记事本出现「今天天气不错我们开始。」 |
| 34 | 上屏可靠性（I-113 排查）：迷你窗抢前台的真 bug + 注入移出事件分发 | 部分完成：迷你主窗创建去掉 `WS_VISIBLE`（改 `SW_SHOWNA`），修掉「说了却提示焦点在 Eryu 自己窗口上」；注入改 `PostMessage(WM_ERYU_INJECTED)` 回 UI 线程执行，先 `Ctrl+V` 再 `WM_PASTE` 兜底，按下热键时记 `LAST_TARGET` 并在必要时代为恢复前台；新增 `--inject-test` / `--inject-test-hold` 自测开关；WinUI 记事本回读仍 `unknown`（待 UIA 方案） |
| 30 | 诊断页 config.toml 可编辑保存（对齐 mac「保存 TOML」） | 完成：`save_toml()` 走 `core.set_config_toml`（新加 FFI 包装），EDIT 里 CRLF↔LF 转换修好（原来 LF 被 Win32 EDIT 吞成一行）；VM 点「保存 TOML」→ 弹「已保存 config.toml。」、日志 `SAVETOML code=0`；证据 `docs/settings-win-diagnose.png` |
| 29 | 润色服务商预设 + 胶囊徽标字体/透明度对齐 mac | 完成：`ID_POLISH_PRESET=260..265` 六个预设写 `polish_endpoint/polish_model`；胶囊档名首字改雅黑、电平条透明度 0.55+0.45×音量（与 `Overlay.swift` 同口径）；证据 `docs/settings-win-polish.png`、`docs/overlay-win.png` |

### Windows 能力对齐（2026-11-20 起；用户要求「UI 和能力对齐 macOS」）

对照 `docs/win-parity.md`（审计产出，逐条带 mac 侧文件:行）分三档做：

1. 便宜且直接消除「不响应/像卡死」的感知：单实例（二次启动把人叫到已有窗口）、覆盖层翻转、退出时清理。
2. 中等：设置页「输入电平」自测 + 设备下拉（对齐 mac SettingsView.swift:563-601）、历史单条删除/重试/记录开关（:1046-1126）、下载进度接 `model_progress`（托盘 + 设置页）。
3. 大块：按应用覆盖（:171-227）、声纹（:426-467）、LAN 听写（:507-535）、引导「试说一句」已对齐（见上表 8）、迷你主窗（MiniWindow.swift:52-190）。

设置页观感：I-105 已修（去掉自绘焦点环、补一帧重画），证据 `docs/settings-win-general.png`。

已补齐（2026-11-20）：单实例（第二次启动只提示并唤出设置窗口，不再抢热键）；设置页「音频」输入电平测试（66ms 一帧、只看音量、切页自动停），证据 `docs/settings-win-mic-level.png`。

托盘菜单补齐（2026-11-20）：状态行、最近一句、取消当前录音、统计与历史、关于；菜单构造期间不持 APP 锁（沿用 P0-2 的教训）。
菜单自测：`Eryu.exe --tray-menu-dump` 会把菜单建一遍并打印各项（VM 实测输出：状态：就绪 / 取消当前录音 / 复制上次结果 / 统计与历史… / 设置… / 选区改写… / 诊断… / 关于 Eryu / 退出）。

回归方法（已在 VM 验过）：guest 里 `taskkill /F /IM Notepad.exe` → 起一个 Notepad → 点击其窗口使其前台 → `ERYU_MIC_WAV=C:\Users\lucien\probe.wav C:\eryu-green\Eryu.exe --hotkey-sim --hotkey-sim-secs 3 --no-onboarding` → 约 15s 后 Notepad 里出现「今天天气不错我们开始吧。」且 `Get-Process Eryu` 全部 `Responding=True`。注意 `runProgramInGuest` 必须带 `-interactive`，且桌面残留的 Notepad 模态框会抢焦点（先全部 kill）。

## 0.8 阶段（进行中；清单 docs/BACKLOG-0.8.md，规则 docs/GOVERNANCE.md）

| 编号 | 事项 | 状态 | 验证 |
|---|---|---|---|
| 0.8.0 | 交接与治理文档落盘（KICKOFF / GOVERNANCE / BACKLOG / DESIGN §12） | 完成 | 三份文档 + DESIGN v0.16 |
| 0.8.1 | 首次体验闭环（R-101/102/103） | 完成（D-110/111/112） | 见下「0.8.1 记录」 |
| 0.8.2 | 文本后处理三层（R-104…109） | 待开始 | — |
| 0.8.3 | 交互与统计（R-110/111/113/114） | 待开始 | — |

### 0.8.1 记录（2026-11-20）

- core 配置：`[ui]` 补 `onboarding_done` / `onboarding_probe_done` / `mini_window` 三项，TOML 往返有单测 `onboarding_state_survives_toml`（I-101）；因 `serde_json::json!` 字段数触顶加 `#![recursion_limit = "256"]`。

- R-102：引导第 2 步实时电平（复用设置页 `MeterBar`，core 66ms 一帧）+ 设备下拉（`eryu_list_devices` → `setDevice`）；连续 3 秒 `rms<0.01` 提示「未检测到明显声音，换个麦克风或靠近一点」，任一帧 ≥0.01 立即清除；进入页面起、离开即停。
- R-101：引导 5 步化（模型 / 麦克风 / 试说一句 / 上屏方式 / 润色）；第 3 步 `ProbeCriterion` 判定（≥4 字且与例句去标点 11 字的字符多重集合交集 ÷ 11 ≥ 0.5）；探针拦截 `submit_request` 只显示不注入（D-110），core 照常收 `submit_result(ok)`；未成功「下一步」禁用（去掉 defaultAction，灰态）；「稍后再说」写 `onboarding_done` 但 `onboarding_probe_done=false`，下次启动回第 3 步。新增 `--onboarding-step 0…4`、`--onboarding-selftest`。
- R-103：迷你主窗（无边框 170×230 NSPanel，置屏幕右上、可拖动）；麦克风圆钮点击开始/再点停止，录音中显示停止图标与「正在听，点击停止」；今日节省 `today.chars÷1000×40`；设置齿轮；关窗写 `[ui] mini_window=false`，菜单栏「显示主窗」写回 true。点击开停只作用于按钮（D-111，全局模式留给 R-110）。
- Windows VM 录音链路（I-080，2026-11-20 补验）：VM 的 VMware HDAudio 声卡可用（`High Definition Audio Device`，麦克风/扬声器端点 state=1）；guest `Eryu.exe --mic-test 5` + host 音量 100 播 `say` TTS，输出真实识别「天天气不错我们开始吧今天天气不错。」；`--pull-model sense` 下载的文件 SHA256 不符（I-103），从 host 拷贝正确模型后才通过。
- 验证：`cargo test --manifest-path core/Cargo.toml` 全过（65+4+6+2）；`--onboarding-selftest` 8 条判定用例全 OK；端到端（`say` 合成 WAV）`ONBOARDING SELFTEST OK 第 3 步出字：今天天气不错我们开始吧。` exit 0；截图 `docs/onboarding-1-model.png`、`onboarding-2-mic.png`、`onboarding-3-probe.png`、`onboarding-3-probe-ok.png`、`onboarding-4-output.png`、`onboarding-5-polish.png`、`docs/mini-window.png`；`--dump-config` 的 `[ui]` 可见三项。
- 2026-11-21：设置页视觉收尾（I-120 键帽组合键显示、顶栏去重、侧栏页脚、引擎页主按钮只留一个、列表整行高）；截图 `docs/settings-win-general.png`、`docs/settings-win-engine.png`、`docs/settings-win-page8.png`。
- 2026-11-21：润色页「保存指令」不再被卡片下沿裁掉；历史/诊断页复审截图更新（`docs/settings-win-polish.png`、`settings-win-history.png`、`settings-win-diagnose.png`）。
- 2026-11-21：设置窗 ✕ 挪回顶栏并实测可关闭（I-121）；`docs/settings-win-general.png` 更新。
- 2026-11-21：迷你主窗点击在组合键热键下失效的问题修好（I-122，`talk_down/talk_up` 抽出并复用）；VM 实测点击开始/停止、✕ 隐藏与 `mini_window=false` 落盘。
- 2026-11-21：托盘「显示主窗」开关写回 `mini_window`（I-123），新增 `--mini-toggle` 自测；VM 实测 true/false 双向落盘。
- 2026-11-21：引导窗 ✕ 修好（I-124，客户区坐标判定）；VM 实测关闭且 `onboarding_done=true`/`onboarding_probe_done=false`。
- 2026-11-21：设置窗导航点击修好（I-125，导航矩形补 HEADER）；电平条空槽换 `░`（I-126）；`docs/settings-win-page2.png` 更新。
- 2026-11-21：修 Windows「上屏成功后永久卡死」（I-127，热词观察在 APP 锁内二次加锁）；`ERYU_TRACE=1` 诊断开关保留在关键路径上。
- 2026-11-21：新增 `--hotkey-sim-repeat`（连跑 3 轮听写，Responding 全程 True）与 `ERYU_TRACE` 日志；win-parity 补自测入口速查。
- 2026-11-21：连跑 6 轮听写（Responding 全程 True、内存/句柄无增长），win-parity 增「稳定性实测」表。
- 2026-11-21：实测覆盖安装/卸载保数据/绿色便携/150% 缩放，win-parity 增「安装与便携模式实测」。
- 2026-11-21：修 explorer 重启后托盘图标消失（I-128）；新增 `--tray-drop-test`，输出 `TRAYDROP dropped=true back=true`。
- 2026-11-21：设置窗进任务栏/Alt+Tab（WS_EX_APPWINDOW），支持 Esc / Alt+F4 隐藏；`after-esc vis=False`、`after-altf4 vis=False` 实测。
- 2026-11-21：托盘双击打开设置（`WM_LBUTTONDBLCLK` → `settings::open()`，实测 `settings vis=True`）。
- 2026-11-21：修 I-129（stub 占位文本不再上屏）；core 单测 65/4/6/2、mac build+smoke 30 项全绿，VM 无模型路径无 `上屏` 行、有模型路径正常尝试注入。
- 2026-11-21：托盘复制改为如实反馈（无结果 / 剪贴板被占 两种提示）——`inject_copy` 返回 bool、`set_text_only` 返回 bool。
- 2026-11-21：Windows 启动兜底恢复「上次没上屏」的暂存文本（放剪贴板 + 托盘提示 + 浮层提示，读后删文件）；VM 实测 `CLIP=[崩溃前没上屏的这句话]`、`PENDING_FILE=False`。
- 2026-11-21：评估启动耗时（Windows 冷启动 4312ms，模型同步加载 3.2s）；异步加载方案因破坏「start 返回即可用」契约回退，记 I-130；`engine_fallback` 改 `Arc<AtomicBool>` 并在切换成功后复位。
- 2026-11-21：修 I-131（坏 config 备份 + 提示）；VM 实测 `BROKEN=True`、`RESP=True`，mac build+smoke 全绿。
- 2026-11-21：I-103 收尾——下载后加载校验 + 镜像 404 回退官方源 + 壳认 `ERYU_CONFIG_DIR`；VM `--pull-model gtcrn` 回退成功。
- 2026-11-21：引导模型页显示下载进度（`onboarding::on_model_progress`），引导期间隐藏迷你窗、结束再显示，模型状态文案如实；VM 截图 `docs/onboarding-win-download.png`。
- 2026-11-21：引导试说页在缺模型时给出明确原因（`engine_ready()`），VM 截图 `docs/onboarding-win-probe-nomodel.png`。
- 2026-11-21：引导麦克风页提示系统权限被拒（`mic_privacy_state`）；引导期间浮层不再被 meter 事件唤起；截图 `docs/onboarding-win-mic-denied.png`。
- 2026-11-21：引导打开时立即收起浮层（`overlay::hide_now` + `main::overlay_hide`）。
- 2026-11-21：浮层提示行显示已录秒数（`overlay::set_level_secs` + `State.secs`），与 mac 对齐。
- 2026-11-21：局域网页令牌改密码框 + 「已设置」状态，修留空保存清空令牌（I-132）；截图 `docs/settings-win-lan.png`。
- 2026-11-21：设置页键盘可达（I-133）：Tab/Shift+Tab 遍历焦点、按钮与开关画焦点描边、自测入口 `--settings-tab-demo` / `--settings-key-demo`；截图 `docs/settings-win-focus.png`。
- 2026-11-21：macOS 设置页显示 `load_warning`（config.toml 读坏时的备份提示）；截图 `docs/settings-mac-config-warning.png`。
- 2026-11-21：macOS 设置页长说明收进悬停提示，修正局域网「需重启」过时文案；截图 `docs/settings-mac-config-warning.png`、`docs/settings-mac-audio.png`。
- 2026-11-21：Windows 设置页回车激活焦点按钮；历史列表加「来源应用」列（对齐 mac）；截图 `docs/settings-win-history.png`。
- 2026-11-21：Windows 引导页键盘可用（Tab 遍历 + 焦点描边 + 回车激活 + Esc 稍后再说）；自测 `--onboarding-key-demo`；截图 `docs/onboarding-win-keyboard.png`。
- 2026-11-21：局域网鉴权与绑定地址实测（403/400/200、allow_lan 无 token 拒启、0.0.0.0 vs 127.0.0.1）。
- 2026-11-21：安装脚本补实测（-Desktop 桌面快捷方式、-PurgeData 全清后重装为全新状态）。
- 2026-11-21：Windows 全量回归（自测/九页冒烟/重建 100 次零增长/试说判定 8 条/托盘 15 项/权限报告/引导键盘）。
- 2026-11-21：Windows 引导窗按工作区收缩放（150% 模拟下整窗可见）；模型下载提示带模型名；截图 `docs/onboarding-win-dpi150.png`。
- 修 I-134：开润色未选档默认「轻度」（Windows `save_polish`、mac `setPolishEnabled`）；新增 `ERYU_POLISH_IN_SIM` 端到端测试钩子。
- 修 I-135：设置页开关「先翻转再落盘」，记录历史 / 只认我的声音现在能真正保存（VM 实测 enabled=false、voice_lock=true）。
- 升级路径实测：0.8.0 精简老配置启动正常，新键默认值生效；自测 3s、9 页冒烟通过。
- 修 I-133 补漏：文本框与列表补 WS_TABSTOP，Tab 序列包含 201/202 输入框（VM 实测）。
- 设置页键盘切页：Ctrl+Tab / Ctrl+1..9，`switch_page()` 统一鼠标与键盘路径（顺带修「离开音频页电平条不停」）；VM `--settings-ctrl-tab-demo 4` 得 page 1/2/3/4。
- 新增 Windows 真机手测清单 `docs/win-manual-test.md`（30 条，含判定与反馈格式）；安装目录构建复验 Ctrl+Tab 切页（page 1/2/3）。
- 登记 I-136（满载下连续短句偶发漏段，已定位到 VAD 判噪路径、暂不改阈值）；冒烟该项加重试并保留首跑输出。
- 修 I-137：双开时误导性启动失败框 → 先探实例、叫出设置窗、提示「已经在运行」（截图 `docs/settings-win-double-launch.png`）。
- 全新解压安装实测（`install-win.ps1 -Desktop`）：桌面快捷方式创建、目标路径正确、启动后单实例；安装脚本改为哈希校验 + 三次重试（覆盖运行中 exe 的静默失败不再发生）。
- 修 I-138：列表滚动条漏套深色主题（150% 缩放下露白槽）；150% 下设置窗整体可达、9 页冒烟与自测全绿（截图 `docs/settings-win-dpi150.png`）。
- 启动耗时实测：`ERYU_TRACE=1` 打「核心就绪 / 界面就绪」（VM 三次 1.8/1.7/1.4s），壳自身 50–90ms，其余是引擎同步加载（I-130）；数据记进 `docs/win-parity.md`。
- 修 I-139：浮层提示行按壁纸亮度选字色 + 覆盖率放大 1.5 倍，默认壁纸上清晰可读（截图 `docs/overlay-win-hint.png`）。
- 修 I-140：引导页 `WM_DRAWITEM` 的阻塞锁改 `try_lock`（构建期同步重绘的潜在死锁）；PROBE 8 条与键盘 Tab 顺序复验通过。
- 修 I-141：内置 core dll 指纹改内容哈希（FNV-1a 64），升级后不再复用旧 dll；VM 实测解出 + 新代码生效。
- 修 I-142：上屏等待 2s→6s，「超时后才到的 ok」也算成功（历史记已上屏、清待取回文本）；VM 实测 `status: inserted`、重启不再弹恢复提示（截图 `docs/settings-win-history-inserted.png`）；顺带给上屏校验加 UIA 读回兜底。
- 模拟麦克风路径补「降噪模型不存在就跳过」，联调日志不再刷 sherpa 的 Load model 报错（真实采集路径本来就是这么做的）。
- 连续听写 soak（单进程 5 轮模拟，WAV）：3 轮成功上屏到记事本（第 4/5 轮热键落在识别中按设计忽略），内存 370→375MB、句柄 375→358 无增长、无 panic；记录进 `docs/win-parity.md`。
- 加固：Windows 壳 183 处 `Mutex::lock().unwrap()` 统一改成 `crate::lock(...)`（poison 后沿用最后的值），避免一次 panic 之后每次取锁都 panic、界面看起来「卡死」；VM 复验自测/9 页/键盘 Tab/GDI-USER 泄漏全绿。

- 修 I-143：逐字打字路径（`type_text` 的 WM_CHAR 与 SendInput 两条出口）补回读校验，目标吞字时如实报 unknown 并提示改用「粘贴」，不再假报已上屏；构建通过，路径级验证待 VM 登录后补。
- Windows 首次可用（R-124/I-144）：SenseVoice 随包、VAD/降噪内置、引擎页只列两项、标点/声纹/大模型用到再下载；引导在模型未就绪时挡住「试说一句」（截图 `docs/onboarding-win-model-ready.png`、`docs/onboarding-win-probe.png`、`docs/settings-win-engine-2engines.png`）；端到端回归记事本 `inserted/17 字`；踩到并修掉 build() 里再取 WIN 锁的死锁（改原子量）。
- 升级路径加固（I-146）：安装器以主模型文件（>100MB）判定是否已装，半截模型会被补齐；VM 实测残缺→重装→`model.int8.onnx` 226MB→记事本落「今天天气不错。」。
- 诊断页重排（I-147）：第一屏改为只读状态（引擎/模型/设备/历史）+ 复制状态/打开日志/运行自检/麦克风权限，TOML 编辑器降到「高级」；截图 `docs/settings-win-diagnostics.png`。
- 逐字打字路径端到端复验（I-143/I-148）：UIA 把记事本设为前台 + `--hotkey-sim-delay 8000` → `verify got=…`、`status=ok`；前台是任务栏时改为 `undelivered`「焦点在桌面或任务栏上」；回读加 700ms 重试窗口，消除 WinUI 假 pending。
- 粘贴路径回归（同轮改动后）：记事本前台 + `--hotkey-sim-delay 8000` → `status=ok`、文中出现「今天天气不错。」、`history.json` `inserted / Notepad.exe`；VM 装机构建 `--self-test`、9 页冒烟全绿（exe `52FCBA5D0BCCA2EF…`，与 `dist-win` 一致）。
- 安装四条路径实测（win-parity 新增表）：卸载保留数据 → 重装历史 16 条仍在、模型不重拷、`--self-test` OK；`-PurgeData` 全清；全新安装模型 237115547B 就位、首屏引导即「已就绪」（`docs/onboarding-win-fresh-model.png`）。
- 引导模型页文案纠错（I-144 补）：未安装引擎「点下面下载」→「在「设置 → 引擎」里下载」，因为该页已无下载按钮。
- 选区改写端到端实测（I-149）：记事本全选 38 字 → 面板「润色」→ mock 服务返回改写文本 → 记事本正文被替换、剪贴板里用户原内容在结束后仍还原；面板截图 `docs/selection-win-palette.png`；同时修掉「粘回目标在面板抢前台后才取」的隐患。
- 连续听写 soak（10 轮 + 8 轮两次，装机构建）：句柄 290→峰值 392 后稳定 363–391、内存 353→377MB 走平、无 panic；模拟 WAV 按会话消耗导致后续轮次只剩尾巴碎片（夹具行为，已写进 win-parity）。
- 引导全流程实测：清标记 → 试说页（WAV 真实识别「今天天气不错。」、下一步变蓝）→ 润色页（`docs/onboarding-win-polish.png`）→ 开始使用 → 关窗且 `onboarding_done=true` / `onboarding_probe_done=true`；再启动不再弹引导。
- 热词闭环（I-150 已结）：`--decode-wav` 证明替换生效（`WAVLAST 今天天气真好不错我们开始吧。`）；页面「读」显示 `今天天气 -> 今天天气真好`；逐字敲 ASCII 后「保存热词」正确写入 `abc = "xyz…"`。之前的失败全是测试脚本坑（`SetWindowText` 句柄不对/传中文乱码/光标位置），非产品 bug。
- 复核「按应用覆盖上屏方式」的证据边界（win-parity 新增小节）：`mode_for` 有单测（notepad/NOTEPAD/Code/chrome/空串），粘贴与逐字打字两条注入路径各自有 VM 端到端证据；但「设了 per_app 之后热键听写实际走哪条」没在 VM 里测成（stdout 重定向拿到的内容与本次运行对不上），按未验证记录，留给真机 D 组确认。
- 通用页收口（I-151）：去掉「保存覆盖」第二个保存键（一个「保存」同时写按应用覆盖，先抓文本再重建页面，避免被清成 `{}`）；去掉页脚「配置：%APPDATA%\Eryu」。装机构建 `FB422BC6…`，截图 `docs/settings-win-general-single-save.png`。
- 历史页收口（I-152）：五个按钮压成四个（复制/删除/清空/重试失败），去掉「刷新」——说完一句自动刷（`result` 事件置标记，放开 APP 锁后再重建，避免自锁）。装机构建 `484F3412…`，截图 `docs/settings-win-history-4buttons.png`。
- 润色页收口（I-153）：六个按钮降到四个（去掉「检测模型」和「保存槽」，槽位跟「用选中的档」一起写），去掉重复的「还没填 API Key」提示；新增 `--assume-consent` 免掉同意弹窗卡自动化。装机构建 `696FB1F4…`，截图 `docs/settings-win-polish-clean.png`。
- 引擎页收口（I-154）：下载地址/代理两行共用一个「保存」，灰字说明收进 ⓘ，「使用选中模型/下载选中模型」→「使用/下载」。装机构建 `7176443E…`，截图 `docs/settings-win-engine-single-save.png`。
- 按应用覆盖上屏方式（I-155）：确认 `mode_for` 有单测、两条注入路径各自端到端、桌面/任务栏拒打生效；VM 前台被 `Progman` 夺走导致「热键→取前台程序名→覆盖」无法自动串联，记为真机手测项。新增 `--inject-test <文本> --mode paste|keystroke`。
- 补验「模型缺失时引导不会骗人」（I-144）：删/改名 SenseVoice 后重开引导，第一页回到「选模型」、下一步灰、点它弹「先下载语音模型…也可以点「稍后再说」」；截图 `docs/onboarding-win-model-missing*.png`。
- 测麦克风不再显示旧结果（I-156）：新增 `eryu_clear_last_result`，`--mic-test` 与设置页测麦前先清内存里的上次结果；VM 验证「留一份 last_result.txt 再测麦」输出「（没听到内容）」。装机构建 `35EC8743…`。
- 麦克风打不开不再「什么都没发生」（I-157）：错误提示会自己弹出来并停留 3 秒，文案只留开头一句，可用设备列表进日志；错误期间核心补发的 idle 状态不会把它顶掉。VM 实测 `device = "不存在的麦克风"`，宿主截图 `docs/overlay-win-error.png`。
- 录音中途麦克风被拔/被占用的路径实测（DESIGN §10.1 第 7 条）：VM 里用 `ERYU_FORCE_MIC_FAIL_MS=1500` 模拟中断 → 保住已录内容（历史 4 字「今天天气」）、继续解码上屏、不卡在「录音中」；前台被桌面占着时正确记 `pending` 而不是假报成功。见 `docs/win-parity.md`。
- 托盘菜单动作加了 `--tray-cmd <id>` 自测入口（菜单点击与命令行共用 `tray::run_cmd`）：VM 实测「显示主窗」两轮开关、「设置」能建窗、「退出」干净退出（无残留进程、`history.json` 合法、无 `.tmp`）。见 `docs/win-parity.md`。
- 修 I-158：启动时「上次没上屏」提示会钉住浮层 3.2 秒，把第一段录音的 `starting/listening` 吞掉（看着像热键没反应）；现在 `starting/listening` 进来先清旧提示。另实测 Esc 取消长录音：无历史残留、进程存活。
- 补齐 I-131 的 Windows 半边（I-159）：配置读坏时通用页底部显示「上次加载配置时出过问题 + 首行原因」，完整报错留给诊断页；VM 实测有提示、有 `.broken` 备份，截图 `docs/settings-win-config-warning.png`。
- 安装包 README 收口：去掉指向未随包文档的「docs/win-manual-test.md」、修正 `install.ps1` 文件名，改成 6 条「装完先自检」（托盘图标 / 说一句出字 / 设置键盘 / 测麦电平 / 历史 / 退出后热键无效）。
- 新增 `docs/win-acceptance-0.8.1.md`：一页验收矩阵（装完能用 / 说话上屏 / 界面交互 / 设置生效 / 稳定性 五组，每项给「自动化证据 + 真机还需确认」），末尾列出自动化验不到的 6 类（真实麦克风、真实按键、输入法、观感、多显示器、explorer 重启）。
- 修 I-160：I-128 里「explorer 重启后托盘图标自愈」实际上只注册了 `TaskbarCreated` 和 5 秒定时器，消息循环里没有分支调 `ensure()`。补上两个分支后 VM 实测：杀 explorer → 定时器发现图标不在并 NIM_ADD → 收到 `TaskbarCreated` 再补一次 → `re-added icon`，进程存活。
- 修 I-161：迷你窗点起来的一段自然结束后 `HOTKEY_DOWN` 没被放掉，后续点击/热键第一下被去重吞掉。拆出 `talk_start()`、加 `TALK_SOURCE`，`idle/done` 时放标记；VM 实测一个进程里连点两次能开两段会话。
- 补验 125%/150%/200% 缩放下的设置窗排版（`ERYU_SETTINGS_DPI=150`）：窗口按屏幕收成 994×698，通用/音频页无裁切无重叠，截图进 `docs/settings-win-dpi150-*.png`；win-parity 记录结论「排版没问题，重影需要真机定位」。
- 补验 125%/200%：发现 `fit_scale` 会在 VM 的 1470×770 屏幕上把任何 >~110% 的请求都夹成 `k=1.1044304`（三档窗口都是 994×698），因此高缩放的观感只能真机验证；已写入 win-parity 与验收矩阵。
- 实测设置窗 Esc / Alt+F4：都只隐藏不退出（`VISIBLE=False`、进程与热键钩子存活），托盘「设置」能再开回来。
- 修 I-162：热键写法错了以前静默退回 ALT_R（GUI 看不到 stderr），且 `CTRL+` 被当成单个 Ctrl。parse 拒绝空段、保存前校验并弹框不落盘、启动时解析失败发托盘提示；新增 `--hotkey-check`，VM 实测 `CTRL+`/`CTRL+FOO` 拒绝、`ALT_R`/`CTRL+ALT+SPACE`/`F13` 通过。
- 修 I-163：热词表整份解析失败时以前静默清空（再点保存就把规则写没了）。现在逐行兜底捞回能认的规则，并把「文件有格式错误（第 N 行），只载入 X 条」显示在两端热词页；VM 实测坏文件下 2 条规则照用。

| 2026-11-21 | Windows | 诊断包（R-127/D-114）：设置→诊断「导出/上传」、CLI `--diag-bundle/--diag-upload`、错误码落盘 `eryu-errors.log`；VM 实测导出 8.1KB、mock 收到 8613B（UA Eryu/0.8.1）、真实 `mic_unavailable` 进包（I-164） |
| 2026-11-21 | Windows | 发版收口：测试开关收进 `--dev`（`--set-config/--set-hotkey/--history-delete/--decode-wav/--inject-test/--hotkey-sim*/--onboarding-step` 等）；`--assume-consent` 不再降级全局对话框。VM 实测 GATED_RC=2、`--dev --dump-config` 正常、诊断包仍可用 |
| 2026-11-21 | mac | 版本号 0.7.0→0.8.1；`install.sh` 后 /Applications 只有一份 0.8.1、build/ 副本已清、LaunchServices 仅注册一份；`--version` 输出 `Eryu 0.8.1`、`--diagnose` 正常 |
| 2026-11-21 | Windows | `--version` → `Eryu 0.8.1`；托盘「自测…」只在 `--dev`（VM 实测非 dev 17 项、dev 18 项）；引导步数口径统一为 6 步（试说第 5 步） |
| 2026-11-21 | 审计 | 按 `docs/AUDIT-0.8.1.md` 修口径：托盘项 17/18、选区剪贴板只还原文本（R-128 排 0.8.2）、引导 6 步、测试口收进 `--dev`（I-165） |
| 2026-11-21 | 审计 | 新增 `docs/AUDIT-0.8.1.md`：A 已修 14 项 / B 文档口径 7 项 / C 未做 7 项（含理由与计划）；过度设计清理 4 屏 + 全局文案；验收条件写清 4 条真机项 |
| 2026-11-21 | mac | 修复胶囊变长时整体右移（I-166）：`resize` 把新原点写回 `lockedOrigin`；`--simulate` 实测 w 264→408 全程 center=735.0 |
| 2026-11-21 | 两端 | 菜单收口：mac 去掉 5 个排障项（重新授权移到设置→诊断）、Windows 去掉 3 个（托盘现 11 项 + 2 分隔线；`--dev` 多「自测…」）；用户 m15620 直接提问「这些测试功能堆在菜单里干什么」 |
| 2026-11-21 | mac | 修复新一段开始时胶囊挂着上一段文字（I-167）：`show(phase:)` 判据放宽到「上一刻不在录音里」并清空 partial/redacted；`ERYU_OV_TRACE=1` 打 `[ov-session]` 日志 |
| 2026-11-21 | Windows | 真机安装（FLYEASY_AI，Win11 26300 AMD64）：`--version`=0.8.1、`SELFTEST OK`、模型随包就位；全盘只有一份 exe；二次启动不再留第二份进程（`WM_ERYU_SECOND` + `exit(0)`） |
| 2026-11-21 | 收口 | 0.8.2 反馈第一刀：菜单只剩「Eryu · 就绪」+「上次：…（N 字）」；润色请求带 max_tokens 且 finish_reason=length 判失败回退原文（I-169）；引擎页改成一条「模型来源」链接 + 代理 ⓘ；局域网「生成」token + 连接地址与 curl 示例；热词改成单列「想听到的词」（R-129..R-133）。`cargo test --lib polish` 8 项通过。 | `e249447` |
| 2026-11-22 | 修复 | 长听写尾句丢失：`Capture::finish` 先宽限收音频再停流、模拟麦克风松手补齐剩余样本、`cap_len_to` 改为保留前 120s（I-172）；longcap 自建配置钉 120s。`--simulate` 22.4s 语料尾句回来，`longcap.sh` 全绿。 | pending |
| 2026-11-22 | 修复 | 焦点锁定（I-173）：热键按下记 `Injector.Target`（pid+焦点控件），提交前若前台已换则 `activate` 切回并还焦点再粘贴，切不回则 undelivered+剪贴板。`--focus-lock-test` 实测 TextEdit→访达 切换后仍落回 TextEdit（文本框含结果=true）。 | pending |
| 2026-11-22 | 修复 | 设置页 UI 三处毛病（I-174）：热词 `onChange` 改数组导致输入法打断→只标脏；润色字段改「标签在上/输入框占满/左对齐」+ `TextEditor` 自绘边框；折叠行只留名称+快捷键，说明全进 ⓘ、占位符清空。截图 `docs/settings-mac-polish.png` / `docs/settings-mac-hotwords.png`。 | pending |
- 2026-11-22：热词输入框丢焦点真根因修掉（HotwordRule 行身份由「from+to」改成 UUID），AppleScript 逐键输入 `abcde` 全落格、焦点环仍在（I-174）。
- 2026-11-22：修掉「往自家输入框里粘贴/说话都没反应」——菜单栏 app 缺主菜单导致 ⌘V 不派发；补隐藏的「编辑」菜单后，手工粘贴与语音上屏都通过（I-175）。
- 2026-11-22：Windows 对照 R-137——热词页是单 EDIT 不存在行重建；Ctrl+V 原生可用；新增 `--dev --settings-input-selftest`（`INPUT typed_ok=true typed_tail="abcde" inject=ok inject_ok=true`）。
- 2026-11-22 品牌：字在/zzai 显示层改名 + 新图标 + 官网下载页（D-117、R-139）
- 2026-11-22：图标定稿 icon-03b，落到 mac/Windows/官网；Windows 补 exe 资源段（I-176，D-118）。
- 2026-11-22：胶囊电平条改官网灯条（紫→青渐变 + 发光），两端同步；两平台包重打。
