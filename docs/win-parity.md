# Eryu Windows 壳审计：能力差距 + 稳定性风险（2026-11-20，对照 6e03cf8）

审计范围：`shell-win/src/*.rs`（15 文件 6608 行）对照 `app/Sources/EryuApp/*.swift`（17 文件 8166 行）、`core/src/{config,ffi}.rs`。
方式：只读（grep/read），未构建、未运行、未改任何文件。行号对应当前工作树。

结论先说：Windows 壳**功能面对齐约六成，但"可见反馈"几乎为零**，且有三类结构性问题直接对应"死机/不能用"的体感：
① 全局键盘钩子回调里**同步做重活**（含锁、OpenProcess、文件 IO、sleep、模态框）；② **持锁弹模态框**（同一把 `APP` 锁在同线程二次加锁 → 永久卡死）；③ **跨进程 `SendMessageW` 无超时**（目标应用一挂，UI 线程永久阻塞）。
另外每画一次界面泄漏若干 GDI 画刷 + 每次切页泄漏一个控件 State/字体，用久了会花屏/崩。

---

## 一、稳定性风险（按后果排序）

### P0 — 一次崩溃 / 永久卡死

| # | 位置 | 触发条件 | 后果 |
|---|---|---|---|
| 1 | `shell-win/src/hotkey_hook.rs:39`（回调直接 `crate::handle_key(...)`）→ `main.rs:632`（Ctrl+Alt+E 里 `selection::open()`：sleep 140ms + SendInput + 可能 message_box）、`main.rs:672-698`（`HOTKEY_DOWN` 锁 + `foreground_app_name` 的 OpenProcess/QueryFullProcessImageNameW + `APP` 锁 + `core.*` FFI）、`main.rs:656-667`（每键 append raw.log） | 每一次按键（不是只有热键）；按住说话；Ctrl+Alt+E 时有窗口没响应 | `WH_KEYBOARD_LL` 回调超 `LowLevelHooksTimeout`（默认 1000ms）→ Windows **静默摘掉钩子**，热键永久失效（只能重启）；卡住期间**全系统键盘输入变卡**；钩子里弹模态框则整个键盘冻死 |
| 2 | `main.rs:914-916`（`model_failed` 分支，仍在 `main.rs:840` 的 `APP.lock()` 作用域内调 `message_box`）＋ `main.rs:966-981`（`MessageBoxW(None,...)` 无 owner）；`tray.rs:96-101`（`if let Some(app) = APP.lock().unwrap().as_ref() { ... diagnose(); message_box(); }`，guard 活到 if-let 结束）；`tray.rs:104-113` 同类结构里还调 `retry_segment`（走网络） | 托盘菜单点「诊断」；模型下载失败；失败段重试 | 模态消息循环期间任何 `WM_ERYU_EVENT` → `handle_event` → 同线程 `APP.lock()` → **自死锁**，界面永久卡死（只能杀进程）。这是"点一下就死机"的最直接解释 |
| 3 | `shell-win/src/inject.rs:146-172`（`SendMessageW(WM_GETTEXTLENGTH/WM_GETTEXT)` 打到**前台其它进程**）、`inject.rs:106-143`（`paste` 里 60+650ms sleep，全部在 UI 线程，由 `main.rs:882` 调）、`inject.rs:9-19`（`open_clipboard` 200ms 轮询） | 前台是 Electron / 微信 / UWP / 卡死的窗口 | 目标线程不回消息 → `SendMessageW` **无超时**、永久阻塞 UI 线程 → 卡死；正常情况每次上屏也要额外卡 0.7~1s |
| 4 | `settings.rs:1409`（`draw_toggle` 的 `CreateSolidBrush`）、`settings.rs:1442`（`draw_list_item`）、`settings.rs:1471`（`draw_button`）、`settings.rs:1351`（`paint` 的导航选中条）——都只 `CreateSolidBrush` 不 `DeleteObject`；`settings.rs:1508-1510` 每次焦点变化就整窗 `InvalidateRect` 重画 | 打开设置页，鼠标在输入框间点几次 | 每次重画泄漏 4~6 个 GDI 对象；进程 GDI 上限 10000 → 约 2000 次重画后 `CreateSolidBrush` 返回 NULL，界面变黑块/画错，继续调用会崩。体感是"设置页用一会儿就花/闪退" |
| 5 | `keycap.rs:349-356`（`Box::into_raw` 存进 `GWLP_USERDATA`，**没有 WM_NCDESTROY 回收**）+ `keycap.rs:36-41`（`State` 里有 `font`，`:352` 创建 HFONT）+ `settings.rs:1222 rebuild_page` 每次保存/切页都 DestroyWindow 重建控件；`keycap.rs:47-51`（`fn state -> Option<&'static mut State>`，在 `paint` 里被叫 3 次 `:116/:138/:151`） | 反复切页/保存 | 每次重建泄漏 1 个 State + 1 个 HFONT（几十次就数百句柄），叠加 #4 更快耗尽 GDI；`&'static mut` 三处同时存在是 UB（可能读到被覆盖的 spec → 画出乱码键帽） |

### P1 — 卡顿 / 功能静默失效 / 资源不释放

| # | 位置 | 说明与后果 |
|---|---|---|
| 6 | `tray.rs:50-53` `set_tip` 只改内存字符串；全仓只有 `tray.rs:46` 一处 `Shell_NotifyIconW(NIM_ADD)`，**无 NIM_MODIFY** | 托盘 tooltip 永远停在启动时那句，所有状态（在听/识别中/模型下载 42%/失败）用户看不到。这是"按了没反应"的主因之一 |
| 7 | `selection.rs:454-461`：后台线程**持 `APP` 锁**调 `core.run_command`（`core/src/ffi.rs:481` 注明是同步 HTTP） | 改写期间整壳冻结数秒~数十秒（所有 `APP.lock()` 使用者排队），热键也失灵 |
| 8 | `selection.rs:430-443` `hide()` 在自身 wndproc 里 `DestroyWindow`；`selection.rs:464-466` 直接 `put_clipboard` + sleep 80ms + `send_paste()` | 结果可能粘到别的窗口（palette 已销毁、焦点没还回去），且**覆盖用户剪贴板不还原**；失败只 `tray::set_tip`（:468/:475），因 #6 用户看不到任何反馈 |
| 9 | `selection.rs:245-281` 只处理 WM_PAINT/WM_KEYDOWN，**无 WM_KILLFOCUS/WM_ACTIVATE** | 点别处面板不关，常驻挡视线 |
| 10 | `caret.rs` 的 `flip` 曾三条路径全写 false | 已修：「检测模型」轮次加 `near_bottom()`（屏幕 2/3 处）与 `overlay::place` 同口径，`--caret-probe` 验过 |
| 11 | `overlay.rs:277`（30Hz 线程里 `render`），并在该线程调 `:618 UpdateLayeredWindow / :635 ShowWindow / :636 SetWindowPos / :645 SW_SHOWNOACTIVATE` | 非窗口线程操作别的线程的窗口，`ShowWindow` 会同步给 owner 线程发消息 → 卡住/死锁风险；且每帧重建字体+DIB（`:427`）+ O(w*h)×3 像素循环，4K 屏 CPU 明显占用 |
| 12 | `overlay.rs:337-349`（`set_final` spawn 线程睡 1.2s 后 `SW_HIDE`）、`:359-371`（`error` 睡 3s） | 1.2 秒内开始的新一次会话，浮层被上一轮的线程隐藏 → 用户看不到正在说的话 |
| 13 | `media.rs:23-31/34-51/53-62`（WinRT `RequestAsync().get()` 阻塞、无超时），由 `main.rs:851/:857` 在**持 `APP` 锁的 `handle_event`** 里调用 | 媒体应用/broker 卡住 → UI 线程无限期等，且锁不释放 → 整个壳假死 |
| 14 | `main.rs:960-964` `quit()` 只 `PostQuitMessage`；无 `UnhookWindowsHookEx`（`hotkey_hook.rs` 也没有卸载函数）、无托盘 `NIM_DELETE`（`tray.rs` 无卸载）、core 无 shutdown | 退出残留幽灵托盘图标、录音设备可能被占；钩子句柄靠进程退出硬回收 |
| 15 | 全仓无 `CreateMutexW`/单实例（mac 有 `AppDelegate.swift:12 SingleInstance.acquire`） | 双开 → 两个低层键盘钩子、两个托盘图标、两个 core 抢麦克风/模型锁文件，表现为"录音只有一半/忽好忽坏" |
| 16 | `hotkey_hook.rs:45-59` 失败只 `eprintln`，`main.rs:273-275` 也仅 eprintln（GUI 无控制台） | 钩子装不上=热键全废，但用户无任何提示 |
| 17 | `settings.rs:1301-1310`：`WM_PAINT` 里 `WIN.try_lock()` 失败直接 `return`，**没有 BeginPaint/EndPaint** | 该次区域未 Validate → 可能反复触发 WM_PAINT 空转（CPU 100%）+ 窗口留白；锁竞争时整页不刷新 |
| 18 | `main.rs:605-614` 解析失败退回 `hotkey::parse("ALT_R").unwrap()`；`hotkey.rs:109` `normalize` 把 0x10 一律当 `VK_LSHIFT` | 用户改错/用组合键后行为诡异却不报错；右 Shift 组合键**永远匹配不上** |
| 19 | `shell-win/src/core.rs:578-591`（`on_event` 里每次 `PostMessageW` 后 `mem::forget(utf16)`），由 meter（每秒 10+ 次）/partial_text（~100ms 一次）高频触发；`main.rs:520-527` 收到后从不释放 | 内存持续上涨（一小时几十 MB 级）；窗口销毁后 lparam 变悬垂指针，core 线程继续 post → 读已释放内存（崩溃时机随机） |
| 20 | 无 `WM_POWERBROADCAST`/`WM_ENDSESSION` 处理（mac 有 `AppDelegate.swift:1624 handleWillSleep` / `:1633 handleDidWake`） | 合盖唤醒后麦克风设备失效，热键/录音要重启进程才恢复 |
| 21 | `onboarding.rs:539-543` 点下载后无任何进度反馈；`settings.rs:1827-1867`（`pull_model`）只在结束时弹一行字；`main.rs:298-310` 首启动悄悄后台下 SenseVoice | 用户以为没反应，反复点 → 重复下载/困惑；"第一次打开像坏了" |
| 22 | `main.rs:966-981` `MessageBoxW(None,...)` 无 owner，且多处从消息回调里弹（`settings.rs:544/755/955/1016/1052/1144/1184`） | 对话框不居中、可点后面的窗口 → 重入；配合 #2 就是死锁 |
| 23 | `settings.rs:1303` 注释承认 build 持锁创建控件会同步触发 WM_PAINT；`onboarding.rs:484-517` `WM_DRAWITEM` 里两次 `WIN.lock().unwrap()`（:497/:508），而 `:56-57` 明写"WM_CTLCOLOR 里再锁就会死锁"、`:522-524` 索性丢弃所有控件通知来躲这个死锁 | 同源隐患：任何 build 期间的同步回调都可能在同一线程二次锁 `WIN` → 卡死；`WM_DRAWITEM` 是现成的未修点 |
| 24 | `inject.rs:60-63` 与 `selection.rs:150`：从剪贴板 `GlobalLock` 后 `while *p.add(len)!=0` 扫描，**没有 GlobalSize 上限** | 畸形/超长剪贴板 → 越界读 → 访问违例崩溃（一次即崩） |
| 25 | `main.rs:503` `&*(buf.as_ptr() as *const RAWINPUT)`（`Vec<u8>` 对齐只有 1，RAWINPUT 需要更大对齐） | 对齐不满足的 UB，x64 上偶发读错 VKey 或崩 |
| 26 | 全仓 90 处 `.lock().unwrap()`（main.rs 26、settings.rs 29、onboarding.rs 11、overlay.rs 10、tray.rs 4）；`window proc` 里 panic 会被**abort**（`extern "system"` 不能展开） | 一次别的线程 panic 使锁中毒后，后续每个 `.lock().unwrap()` 都 panic → 进程直接消失（无错误窗）。建议统一 `lock().unwrap_or_else(|e| e.into_inner())` |

---

## 二、能力差距（macOS 有 → shell-win 缺/弱）

| 能力 | macOS 位置 | Windows 现状 |
|---|---|---|
| 设置页 7 个分页骨架 | `SettingsView.swift:44-63`（general/engine/audio/hotwords/polish/history/diagnose） | ✅ 有：`settings.rs:55 PAGES`，同名 7 页，自绘 UI |
| 快捷键录制 + 预设 | `SettingsView.swift:128-155` | ✅ `keycap.rs` 键帽控件 + 三个预设（右 Alt / Ctrl+Alt+空格 / F13）；组合键左右修饰键互认（I-110） |
| 录音判定 min/max | `:156-170` | ✅ 有（`settings.rs:463-526` 里的 e_min/e_max） |
| 上屏方式（粘贴/逐字） | `:171-190` | ✅ 有（c_mode） |
| **按应用覆盖**（`insert_per_app`） | `SettingsView.swift:171-227`（sheet 增删改）+ `core/src/config.rs:845 mode_for` | ✅ 有（`651bace`）：通用页多行「进程名=粘贴/打字」+ 保存覆盖；`--settings-perapp-selftest` 回读验证 |
| 背景降噪（含下载 1MB 模型） | `:229-243` | ✅ 开关 + 模型状态；引擎页列出 `gtcrn` / `punct` / `speaker` 可直接下载（`e2d2880`，core 清单已补附加模型） |
| 补标点（含下载 65MB 模型） | `:250-267` | ✅ 开关 + 引擎页可下载 punct 模型（`e2d2880`）；未装时开关文案提示去下载 |
| 隐私模式 | `:269` | ✅ 有（`settings.rs:514`） |
| 开机自启 | `:278-300` | ✅ 有（`autostart.rs`）；exe 挪位置后启动会重写 Run 键（`_`） |
| 音源锁定（实验） | `:318` | ✅ 有（`b28d7e9`，音频页开关 + 写 `source_lock`） |
| 引擎：模型列表/下载/镜像/代理 | `EngineSection :332-420` | ✅ 基本对齐（`settings.rs:549-610`） |
| **声纹**（只识别我的声音 + 录 8 秒 + 删除） | `AudioSection :426-467` | ✅ 有（`0bdfde8`）：声纹页开关/登记状态/录 8 秒/清空；证据 `docs/settings-win-voice.png` |
| **局域网听写接口** | `:507-535` | ✅ 有（`263b40d`）：开关/端口/令牌/allow_lan/日志；VM 实测 `POST /transcribe` → `{"text": …}`；证据 `docs/settings-win-lan.png` |
| 麦克风选择 | `:549` | ✅ 有（`settings.rs:613-669`） |
| **输入电平测试** | `:563-601` MeterBar | ✅ 有（`d885f1f`）：音频页电平条 + dB，66ms 一帧、不录音；证据 `docs/settings-win-mic-level.png` |
| 热词建议 采纳/忽略 | `HotwordsSection :1171-1216` | ✅ 有（`e09857e`）：历史高频词一键加进热词（采纳）；暂无「忽略」记忆 |
| 润色：档位编辑/服务商预设/测试连接/检测模型/同意弹窗 | `PolishSection :806-840, :887, :937, :940, :986` | ✅ endpoint/model/key/档位列表/快捷键槽/服务商预设/测试连接/档位指令编辑/检测模型/首次同意弹窗（`d1466eb`/`e01d9be`/`07f4caf`） |
| 历史：统计 + 列表 | `HistorySection :1014-1126` | ✅ 有统计+最近 50 条+复制/清空（`settings.rs:765-825`） |
| 历史：单条删除 / 重试上次失败 / 记录开关 | `:1046-1126` | ✅ 有（`4cf73aa`）：记录开关、删除选中、重试失败段；证据 `docs/settings-win-history.png` |
| 诊断：自检/权限跳转/编辑 config.toml | `DiagnoseSection :1239-1272` | ✅ 「运行自检」「麦克风隐私设置」跳转；config.toml 可编辑并「保存 TOML」（`559cd4e`），CRLF 转换 + 保存后重载；底部显示系统麦克风权限状态 |
| 引导步骤 | `Onboarding.swift:221-245` 5 步（model/mic/probe/output/polish）+ `:512-586 probeStep` 必须"试说一句"成功 | 6 步（model/mic/hotkey/output/probe/polish；试说在第 5 步，不是文档旧说的第 3 步）：`01a7fdb` 加了试说闭环与成功判定，本轮补「先确认麦克风」页（实时电平条 + 当前输入设备 + dB，进页开 meter、离页停）；证据 `docs/onboarding-win-mic.png` |
| **迷你主窗** | `MiniWindow.swift:52-190`（今日已节省 X 分、说话按钮、设置入口） | ✅ 有（`d6e02cf`）：170×230、圆钮点击开始/停止、今日节省、✕ 隐藏、托盘恢复；证据 `docs/mini-window-win.png` |
| 托盘菜单 | `StatusController.swift:150-163` | ✅ 对齐且收口：状态行、最近一句、取消、复制、统计与历史、设置、选区改写、诊断、重试失败段、关于、显示主窗、退出（`--tray-menu-dump`：发版 11 项 + 2 分隔线 / `--dev` 多一项「自测…」）。测麦、麦克风权限、权限状态、自测这些排障口**不进菜单**——设置 → 音频/诊断里都有，`--dev` 下才在菜单里出现。**没有「暂停听写」这一项**——mac 的暂停是长时间不用的开关，Windows 用「取消当前录音（Esc）」 + 设置里「暂停媒体」替代，见 §13「刻意不做」 |
| 单击 / 双击模式 | `AppDelegate.swift:887/:912` + `KeyTap.swift` | 与 mac 同状态：两端都只有「按住说话」，两种模式是 0.8.3 的 R-110，未开工 |
| 权限体系与自检 | `Permissions.swift:84-192` | ✅ 诊断页「运行自检」+「麦克风权限」跳转；权限详情（ConsentStore 状态、键盘监听说明、UIPI 提醒）改由设置 → 诊断的「复制状态」带出（`--dev` 下托盘仍有「权限状态…」）；采集失败仍报 `no_audio`（与 mac 同） |
| 睡眠/唤醒 | `AppDelegate.swift:1624/1633` | ✅ 有（`d2a7a75`）：`WM_POWERBROADCAST` 唤醒后重装钩子 + 重注册 raw input |
| 单实例 | `AppDelegate.swift:12` | ✅ 有（`d885f1f`）：`FindWindowW("EryuMessageWindow")` 命中即提示并唤出设置 |
| 编辑观察 | `EditObserver.swift`（108 行） | ✅ `observer.rs`：上屏后 15 秒内 1s 轮询焦点控件文本，公共前后缀 diff 出「原词→新词」，热词页采纳（证据 `docs/settings-win-hotwords-learned.png`）。限制：UWP/WinUI 控件不响应 `WM_GETTEXT` 时不观察（安静跳过），经典 Win32 编辑框可用 |
| 关于/版本 | `AppDelegate.swift:272-273 --version` | ✅ 有：托盘「关于 Eryu」带版本号（`PRODUCT_VERSION`，`1606081`） |
| 选区改写 | `SelectionLane.swift`（327 行）+ ⌘E | ✅ 五条指令一致（`selection.rs`）；粘回前归还焦点、失焦自动收起；**剪贴板只还原文本**（图片/文件类剪贴板会被覆盖，R-128 排 0.8.2）；端到端实测：记事本 38 字 → 润色 → 正文被替换、文本剪贴板还原（I-149，截图 `docs/selection-win-palette.png`） |
| 浮层 | `Overlay.swift`（翻转、多屏） | ✅ 翻转：caret/窗口/鼠标三条路径都走 `near_bottom`；多屏：`work_area_at` 用 `MonitorFromPoint(锚点)` 取该屏 `rcWork`，无锚点时按鼠标所在屏 |

一句话概括差距：**core 能力（配置字段）几乎全部已经存在**，Windows 缺的是"把它们摆到 UI 上"以及"把结果反馈给用户"。6608 行里 1956 行花在自绘设置页，真正的语音链路（热键→录音→上屏）不到 600 行。

---

## 三、先修什么（按顺序，一条一句可执行）

**P0（先止血，1~2 天）**
1. `hotkey_hook.rs:39` 改成：钩子回调只做 `if 命中 hotkey { PostMessageW(hwnd, WM_ERYU_KEY, vk, is_break) }`，把 `handle_key` 整体搬到 UI 线程的消息处理里（同时 `:27` 的 raw.log 也搬走，或仅在 ERYU_RAW_DEBUG 时异步写）。
2. 所有 `message_box` 挪出锁作用域：`main.rs:914-916` 先把文案拼好再 `drop(app)` 弹；`tray.rs:96-101` 改成 `let d = { let g = APP.lock().unwrap(); g.as_ref().map(|a| a.core.diagnose()) }; if let Some(d) = d { crate::message_box(&d) }`；`:104-113` 同理。
3. `inject.rs:146/106-143` 的跨进程文本读取改 `SendMessageTimeoutW(SMTO_ABORTIFHUNG, 100)`，拿不到就退回"不读目标文本、只发 Ctrl+V"；把 `sleep` 从 UI 线程挪到 worker。
4. 修 GDI 泄漏：`settings.rs` 里 `CreateSolidBrush` 全部改成窗口字段/`OnceLock` 缓存（照抄已有 `cached_brush :113-123`），或每次绘制后 `DeleteObject`。
5. `keycap.rs` 加 `WM_NCDESTROY` → `Box::from_raw(GWLP_USERDATA)` + `DeleteObject(font)`；`state()` 别造 `&'static mut`，改成把 spec 复制出来。

**P1（核心可用，一周）**
6. `tray.rs:50-53` 里补 `Shell_NotifyIconW(NIM_MODIFY)`（`NIF_TIP`），失败态用 `NIF_INFO` 气泡——这一步单独就能解决大量"没反应"的抱怨。
7. `selection.rs:454-461` 去掉持锁跨网络：先在锁内取出 core 指针/必要参数，调用时无锁。
8. `selection.rs:430-443` 的 `hide` 改成 `PostMessageW(WM_CLOSE)` 延后销毁；`:464-466` 粘贴前 `SetForegroundWindow(prev)`，结束后还原剪贴板原文。
9. `selection.rs:245-281` 加 `WM_ACTIVATE(WA_INACTIVE)` → hide；顺带给 palette 加 Esc 之外的"点外部关闭"。
10. `caret.rs` 把 `flip` 算出来（caret.y 距屏幕底不足浮层高+24 就 true），`overlay.rs` render 里按 flip 上下偏移。
11. `overlay.rs` 的窗口调用（`:618/:635/:636/:645`）全部 `PostMessageW` 回 UI 线程执行，30Hz 线程只算像素。
12. `overlay.rs:337-371` 用 epoch 计数器：线程醒来若 `epoch != my_epoch` 就不隐藏。
13. `media.rs` 的 pause/resume 移到独立线程（或加超时），并从 `handle_event` 里把 `media::pause()` 移到 `drop(app)` 之后。
14. 加单实例：`main.rs` 启动时 `CreateMutexW("Local\\Eryu.SingleInstance")`，`ERROR_ALREADY_EXISTS` → 通知已有实例退出。
15. `quit()`/`WM_DESTROY` 里补 `UnhookWindowsHookEx(HOOK)`（`hotkey_hook.rs:19` 的 static mut 换成 `AtomicIsize` 并在退出时读出来）、`Shell_NotifyIconW(NIM_DELETE)`（`tray.rs` 加 `uninstall`）。
16. `hotkey_hook.rs:45-59` 与 `main.rs:273-275` 失败改成托盘气泡 + 设置页诊断红字。
17. `settings.rs:1301-1310` 拿不到锁也要 `BeginPaint/EndPaint`（画底色即可），避免 WM_PAINT 空转。
18. `main.rs:605-614` 解析失败保留旧值并提示；`hotkey.rs:109` 区分 `VK_LSHIFT/VK_RSHIFT`。
19. `core.rs:577-590` 改成"事件入 `VecDeque` + PostMessage 只带 id"，UI 线程取走释放，或 `Box::from_raw` 回收。
20. 加 `WM_POWERBROADCAST`（`PBT_APMRESUMEAUTOMATIC` → core 重开设备）；`WM_ENDSESSION` 里做 15 的清理。

**P2（UI 对齐，按用户抱怨强度排序）**
21. 设置页补三块最便宜的：按应用覆盖（`insert_per_app`）、补标点开关、历史开关/单条删除——core 字段全在，纯 UI 工作量。
22. 下载进度接事件：`model_progress` 目前无人消费，接到托盘 tooltip（配合 6）+ 引导页进度条 + 设置页引擎行。
23. 声纹录入页（下载 39MB 模型 + 录 8 秒 + 删除）与局域网听写页（开关、0.0.0.0:端口、token）。
24. 引导补第 2 步麦克风与"试说一句必须成功"（对齐 `Onboarding.swift:512-586`），否则新用户永远不知道麦克风坏了。
25. 诊断页补：自检按钮（调已存在的 `settings::self_test_all`）、跳转"设置→隐私→麦克风"（`ms-settings:privacy-microphone`）、编辑并保存 config.toml。
26. 迷你主窗（`mini_window`）、托盘菜单补齐（自检/麦克风测试/取消/打开历史/重试失败段）、关于/版本页。
27. 润色页补 服务商预设、测试连接、检测可用模型、档位 prompt 编辑（对齐 `SettingsView.swift:887-940`）。
28. 控件绘制性能：`overlay.rs:427` 每帧 `CreateDIBSection` + 字体重建改成缓存（按 DPI/尺寸做 key），并把 O(w*h) 三遍循环降到一遍。

---

## 四、审计备注

- 所有结论均来自静态阅读，未运行程序；`wrap`/`expect` 未逐条验证可达性，标记的是"最可能被用户触发的路径"。
- 有两个猜测被源码否证，已从 P0 降级：`main.rs:303-308` 持锁 `pull_model`（`core/src/ffi.rs:514` 说明派发即返回，异步）、`main.rs:840` 持锁调 overlay（overlay 用独立 `st` 锁，无 ABBA）。
- `settings.rs` 的 `WM_COMMAND` 各分支都是先 `with_win(...)` 取完值再动作，没有持 `WIN` 弹框；真正"持锁 + 模态"的只有 `main.rs:914-916` 与 `tray.rs:96-101`。

## 自测/CLL 入口（2026-11-20 补）

- `--onboarding-step N`：强制把引导开在第 N 步（0 模型 / 1 按键 / 2 上屏 / 3 试说 / 4 润色），截图自测用。
- `--onboarding-probe-selftest`：只验「试说一句」的判定函数（8 条用例，打印重合率），退出码 0/1。
- `--hotkey-sim [--hotkey-sim-secs N] [--hotkey-sim-vk 0x7C]`：模拟按住热键 N 秒。**注意**：不传 `--hotkey-sim-vk` 时默认模拟右 Alt（0xA5）；如果当前配置的快捷键不是右 Alt（例如被改成 `F13`），`handle_key` 会被匹配过滤掉，日志里只有 `SIM hotkey_down` / `SIM hotkey_up`、没有任何识别结果，看起来很像「录音链路坏了」。先 `--dump-config` 看 `[hotkey] keys`，再用对应 vk（F13=0x7C）重试。
- 假麦克风：环境变量 `ERYU_MIC_WAV` 指向 16k 单声道 WAV，日志里出现 `[eryu][sim] 用 WAV 代替麦克风（N 采样）` 才说明真的走了模拟采集；没有这行就是会话压根没开始。

## 设置页开关的状态存在哪（2026-11-20）

自绘 pill 开关的状态由 `shell-win/src/settings.rs` 的 `TOGGLES`（`Mutex<Vec<(id, bool)>>`）持有，**不要**改回 `BM_SETCHECK`/`BM_GETCHECK`：owner-draw 按钮上这两个消息不生效，会导致所有开关永远显示「关」、保存写错值（I-106）。新增开关时：用 `checkbox()` 建控件（自动登记初值）、把 id 加进 `is_toggle()`、点击由 WM_COMMAND 末尾统一 `flip_toggle` 翻转、保存时用 `checked(hwnd)` 读状态。

## 按应用覆盖（Windows 版形态）

mac 是表格（进程 + 下拉），Windows 版用多行文本：每行 `进程名=粘贴` 或 `进程名=打字`，`#` 开头忽略。保存走 `set_config({"insert_per_app": {...}})`，与 mac 写同一份配置；`--settings-perapp-selftest` 在进程内直接调 `save_per_app()`（等价于点「保存覆盖」，绕开 UIPI）。

### 覆盖方式的生效链路（2026-11-21 复核）

`core/src/config.rs::mode_for`（按进程名包含匹配、大小写不敏感，未命中退回全局 `insert.mode`）已有单测覆盖：`cargo test --lib` 里 `mode_for(&cfg, "notepad.exe")`、`"NOTEPAD.EXE"`、`"Code.exe"`、`"chrome.exe"`、`""` 全部断言正确。

端到端这条链路拆成两段各自有证据：

- **粘贴**：VM 实测 `mode=paste` → 记事本落字、历史 `inserted / Notepad.exe`；
- **逐字打字**：VM 实测 `mode=keystroke` → `type fg_class=RichEditD2DPT`、`verify #0 got="…"`（`176078d`）。

**没有单独做的**：在 VM 里把 `per_app` 设成 `notepad.exe=keystroke` 再热键听写、从日志确认走的是打字而不是粘贴。试了两轮，`runProgramInGuest` 下把 Eryu 的 stdout 重定向到文件时拿到的内容与本次运行对不上（混进了别的自测输出），没法据此下结论——所以这一格的结论是「逻辑与两条注入路径都有证据，覆盖选择这一步只有单测」，不是「已验证端到端」。真机上按 `docs/win-manual-test.md` 的 D 组「按应用覆盖」顺手确认一次即可。

## 声纹（2026-11-20）

- 对应 mac：`SettingsView.swift` 的「声纹」段（`eryu_voice_enroll_mic` / `eryu_voice_state` / `eryu_voice_clear`）。
- Windows：设置页第 8 个导航项「声纹」（`page_voice`），开关写 `voice_lock`；状态行直接渲染 `eryu_voice_state` 的 JSON（`enrolled`/`dim`/`created`/`model_ready`）。
- 录 8 秒登记必须放后台线程：`eryu_voice_enroll_mic` 是阻塞调用，放 UI 线程会把窗口冻住 8 秒。线程拿裸句柄（`Core::raw_handle()`，转 `usize` 过线程边界——`*mut c_void` 不是 `Send`），完成后 `PostMessageW(WM_VOICE_DONE)` 回设置页重建。
- 自测入口：`--voice-state`、`--voice-enroll-wav <wav>`（走 `eryu_voice_enroll_wav`，不需要麦克风）。VM 实测：`VOICE wav rc=0`、`state={"dim":512,"enrolled":true,...}`，且第二次启动仍读到同一状态。
- 声纹模型不随安装包附带：缺 `3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx` 时登记返回 `-3` 并提示「声纹模型还没下载」；设置页会显示「声纹模型未就绪」，不误导用户。

## 升级路径实测（2026-11-21）

- 模拟 0.8.0 老配置（只有 `[hotkey] keys`、`[engine] model`、`[ui] onboarding_done` 三段）启动：进程存活、只有一个主进程、`--self-test` 通过（3s，含引擎加载）、9 页冒烟通过。
- 老配置里没有的新键（`polish_level`、`lan_*`、`voice_lock`、`history.*`、`[privacy]`）由代码默认值补齐，不需要用户手动迁移文件；文件本身保持原样，只有用户在设置里改过某项才回写。

## 历史页操作实测（2026-11-21）

- 修复 I-135 后（并逐项复验全部自绘开关）：「记录历史」开关点击 → `--dump-config` 变 `enabled = false`；「只认我的声音」点击 → `voice_lock = true`（之前两处都只翻转界面）。

- `--open-settings-page 4` 打开后列表 `LB_GETCOUNT=2`；`LB_SETCURSEL 0` + `WM_COMMAND id=121`（复制选中）→ 剪贴板变成该条文本。
- 同一条再发 `WM_COMMAND id=125`（删除选中）→ 列表变 1 条，`history.json` 同步少一条（跨进程 `--dump` 可见）。删除走 `core.delete_history(id)`，与 mac 的单条删除同一入口。

## 稳定性加固（2026-11-21）

- 壳里所有全局状态都是 `Mutex`。原来一律 `lock().unwrap()`：只要某个线程 panic 一次，锁就被标记 poisoned，之后**每一次**取锁都 panic——用户看到的是「上一个错误没提示，后面连设置都点不动」。现在统一走 `crate::lock(&M)`（`unwrap_or_else(|e| e.into_inner())`），panic 只影响当次调用。
- 同类教训：I-127 是「持锁时调自己的 pub 函数 → 再取同一把锁自锁」，I-140 是「构建期同步重绘 → 重入 `WIN`」，都是同一类「锁的作用域没算清」。

## 连续听写 soak 实测（2026-11-21）

- 单进程连跑 5 轮模拟听写（`--hotkey-sim-repeat 5 --hotkey-sim-secs 3`，WAV 输入，不润色），装机目录构建：`history.json` 得到 3 条 `status: inserted` / `app: Notepad.exe` / `chars: 12` / `ms: 2917~3096`，记事本里三段文字依次拼接。
- 第 4、5 轮的热键落在上一轮的「识别中」，按设计被忽略（`talk_down` 只做去重，不排队）——两端同一套核心状态机，行为一致。要连测多轮就把间隔拉大到超过一轮识别时间。
- 内存/句柄：期间 `mem_mb=370.3 / handles=375` → `375.1 / 358`，没有增长；进程一直存活，日志无 panic。
- 注意：VM 里测监听要「先杀干净所有 `Eryu*`（上一轮留着设置窗会让新进程撞单实例守卫、静默退出）」；`&&` 启动的子进程用 `Start-Process -RedirectStandardError` 才拿得到日志（GUI 子系统的 `& $E` 不落盘）。

## 上屏状态与升级路径实测（2026-11-21）

- **上屏三态**：`ok` → 历史「已上屏」并清掉待取回文本；`undelivered`（目标窗口是管理员权限、焦点在 Eryu 自己窗口等）→ 文字留剪贴板；`unknown`（读不到目标输入框）→ 历史「待复制」，下次启动提示取回。
- **VM 实测（装机目录构建）**：`ERYU_MIC_WAV=probe.wav` + `ERYU_POLISH_IN_SIM=1` + `--hotkey-sim --hotkey-sim-force`，前台是记事本 → 日志 `上屏 status=ok`、`history.json` 为 `"status": "inserted"` / `"app": "Notepad.exe"`，历史页显示「17 字 已上屏」（截图 `docs/settings-win-history-inserted.png`）；紧接着再启动一次**没有**弹「上次没上屏」。
- **升级路径**：内置 `eryu_core.dll` 的指纹改为内容哈希（I-141），把旧 dll 留在 `%APPDATA%\Eryu\bin` 再装新 exe，日志出现「CORE 内置 dll 已解出」，新逻辑立即生效——之前用「长度 + 首末字节」当指纹，core 改了几百行也不会重写。
- **VM 测试注意**：脚本跑在 Windows Terminal 里会一直占着前台，`SetForegroundWindow`/`AppActivate` 都抢不过来（调用进程不是前台进程）。要测「真正的前台程序」得让控制台先退出、由隐藏进程延时启动 Eryu，否则注入会打进终端、历史里的 app 名也会是 `WindowsTerminal.exe`。

## 润色端到端实测（2026-11-21）

- 本地 mock（`/v1/chat/completions` 回固定文案）→ `--polish-test` 通过，请求带 `Authorization: Bearer <key>`、system prompt 为内置 7 条规则。
- 真链路：`ERYU_MIC_WAV` + `ERYU_POLISH_IN_SIM=1`（默认 WAV 联调会跳过润色，避免把网络耗时混进本地区间）+ `--hotkey-sim --hotkey-sim-force` → 日志 `上屏 status=ok`，记事本里落的是**润色后**的文本（`润色后：今天天气不错，我们开始吧。`），mock 收到第二次请求。
- 顺带修 I-134：开关打开但档位仍是 `off` 时，保存会默认写 `light`（两端一致），否则开关亮着文字却不变。

## 润色「测试连接」（2026-11-20）

- 对应 mac：设置 → 润色里的连通性测试（`eryu_polish_test`）。
- Windows：润色页「保存润色设置」旁的「测试连接」；`eryu_polish_test` 是同步 HTTP（最长到 `polish_timeout_ms`），必须放后台线程，完成后 `PostMessageW(WM_POLISH_DONE)` 回设置页重绘（同声纹那套）。
- 自测入口：`--polish-test`。VM 实测（没填 Key）：`POLISH text= err=没有填 API Key` —— 说明 FFI 与错误串回传都正常，且不阻塞、不崩。

## 热词建议（2026-11-20）

- 对应 mac：设置 → 热词里的「建议」（从历史高频词里挑）。
- Windows：`page_hotwords` 下半部分「建议（从历史里挑的高频词）」；算法在 `suggest_from(texts, existing)`：2–5 个汉字/字母数字的片段、出现 ≥2 次、排除已在热词表里的词，按次数→长度排序后去掉被更长建议词包含的碎片（否则会出现「设计文档/计文档/文档」这种重叠）。
- 一键加入：`add_suggested_hotword()` 读列表选中项，往 `hotwords_json` 追加 `{"from": 词, "to": 词}` 再 `set_hotwords`。
- 自测：`--hotwords-suggest-selftest`（纯逻辑，不读历史文件）→ `SUGSEL 设计文档 x3 / count=1`；`--hotwords-suggest`（读真实历史）。注意 `--debug-submit-text` 在 VM 里会卡住（会走去上屏路径），别用它造历史。

## 真实热键端到端（2026-11-20 22:59 VM）

之前所有「上屏」证据都是 `--hotkey-sim`（直接调 `handle_key`）或模拟 WAV。这一段把最后两环补上：**真实键盘按键**（走 LowLevel 钩子）+ **真实麦克风**。

步骤（VM 内，全部在 Console 会话 1）：
1. 杀掉所有 Eryu 进程，`cmd /c hook.bat` 启动 `Eryu-new.exe --no-onboarding`（带 `ERYU_RAW_DEBUG=1`）。
2. 宿主 `osascript -e 'set volume output volume 100'`，`say -v Tingting` 念「今天天气不错，我们开始吧」「我们下周把设计文档发给你」。
3. 权限足够的 PowerShell 里 `keybd_event(0x7C)`（F13，按下）→ 6 秒 → `keybd_event(0x7C, 0, 2, 0)`（松开）；前台是 Notepad。
4. 结果：`raw.log` 里 `hook vk=0x7c break=false target=…` → `handle vk=0x7c spec=F13 matched=true` → `hotkey_down app=Notepad.exe`；Notepad 里出现「今天天气不错我们开始吧。」，见 `docs/win-e2e-real.png`。

结论：
- 合成按键**能**被低级钩子收到（之前有一次测不到，是因为当时跑的是上一版进程：`target=` 为 0，钩子只投递不处理，看起来像"按键没反应"）。`ERYU_RAW_DEBUG=1` 时钩子会往 `raw.log` 写 `hook vk=… target=…`，排查热键问题先看这一行。
- 真实麦克风 + 真实按键 + 上屏 + 程序不卡，四件事在 VM 里同时成立。
- 测试时别留第二个实例：单实例提示框会抢前台，Ctrl+A/C 读到的是提示框文字而不是 Notepad 内容（踩过一次，误以为没有上屏）。

## 迷你主窗（2026-11-20 23:0x VM）

- 对应 mac：`MiniWindow.swift`（170×230，大的麦克风按钮 + 按 F2 或点击开始 + 今日节省 + 设置）。
- Windows：`shell-win/src/mini.rs`。`WS_POPUP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`，放在工作区右下角、比任务栏高 18px、再往上让 60px 躲开「未激活 Windows」水印窗口（那是个置顶窗口，会盖住迷你窗）。
- 交互：点圆钮 → `mini_toggle_talk()`，按当前热键的 vk 走 `handle_key`（跟真实按键同一条路）；录音中红底白方块 + 「正在听，点击停止」，`state=idle` 回待机；空白处可拖动（`WM_NCHITTEST` 返回 `HTCAPTION`）；点 ✕ 隐藏并写 `mini_window=false`，托盘「显示主窗」重新打开并写回 true。
- 字体坑：GDI 里 emoji（`🎤`）是代理对，会画成「图形 + 方框」，符号字体也常有缺字——最后改成用 GDI 图元画话筒（圆头 + 长条 + 底座），完全可控。
- 证据：`docs/mini-window-win.png`（待机）、`docs/mini-window-win-rec.png`（录音中，红底方块）。

## 局域网听写（2026-11-20 深夜，已验通）

- mac：设置 → 局域网（开关 / 端口 / 令牌 / allow_lan / 日志）。Windows：`shell-win/src/settings.rs` 的 `page_lan`（第 9 页「局域网」），字段 `lan_enabled/lan_port/lan_token/lan_allow_lan` 经 `core.set_config` 写入，日志来自 `eryu_lan_log`。
- 接口：`POST /transcribe`（body = 16k 单声道 16-bit WAV）→ `{"text": …}`；`allow_lan=true` 时必须配 token。
- 状态：已验通（I-107 已解决）。VM 里设置页点「开启」→「保存并重启接口」后 `netstat` 出现 `TCP 127.0.0.1:8765 LISTENING`，`Invoke-WebRequest -Uri http://127.0.0.1:8765/transcribe -Method Post -InFile probe.wav` 返回 `{"ok":true,"text":"今天天气不错我们开始吧","ms":298,"segments":1,"chars":11}`，日志行 `127.0.0.1 /transcribe → 200`；证据 `docs/settings-win-lan.png`。
- 关键修复（mac/Windows 共用）：以前监听只在 worker 启动时起，改配置不重启 → 保存按钮等于没生效。现在 `core/src/lan.rs` 用可停止的非阻塞 accept 循环（`STOP` 标志 + `pub fn stop()`），`Worker::apply_config` 比较 `(lan_enabled, lan_port, lan_token, lan_allow_lan)`，变了就 `lan::stop()` 再 `start_lan_if_enabled()`。
- 界面坑：LAN 控件 ID 曾用 200–205，与 `ID_EDIT_BASE = 200` 的输入框区间相撞（`is_toggle` 的 `+4/+5` 把「保存/刷新」画成开关）→ 已改 230–235。测试时别用 `--set-config` 传 JSON（cmd/PowerShell 会吞内层双引号），用设置页 UI。

## 打包与自测入口（2026-11-20 收尾）

- `scripts/package-win.sh [版本]` → `dist-win/Eryu-<版本>-win-x64.zip`：只含 `Eryu.exe` 和 `先看这里.txt`（数据默认 `%APPDATA%\Eryu`；exe 旁放 `portable.txt` 即绿色便携）。
- 全量自测：`Eryu.exe --self-test`、`--onboarding-probe-selftest`、`--settings-pages-smoke`（9 页逐页开一遍）、`--tray-menu-dump`、`--voice-state`、`--lan-chk`。

## 自动化坑：`--set-config` 要等 worker 落盘（2026-11-21）

`set_config` 是投给 worker 的异步命令，`--set-config` 分支以前 report 完就 `return`，进程退出时 worker 可能还没 `apply_config`+`save` → 实测 `SETCONFIG code=0` 但 `--dump-config`/`config.toml` 没变（早期"denoise 写不进去"也是这个原因）。现在分支里 `sleep 1200ms` 再退出，与 `--set-hotkey` 一致。

配套：VM 走 HTTP 访问本机 8765 时如果 curl 报 `HTTP=000`，先看系统代理——`curl --noproxy "*"` 才是直连（`Invoke-WebRequest` 同样受系统代理影响）。

## 自动化坑：--set-config 要等 worker 落盘（2026-11-21）

set_config 是投给 worker 的异步命令，--set-config 分支以前 report 完就 return，进程退出时 worker 可能还没 apply_config+save，实测 SETCONFIG code=0 但 --dump-config/config.toml 没变（早期“denoise 写不进去”也是这个原因）。现在分支里 sleep 1200ms 再退出，与 --set-hotkey 一致。

配套：VM 里用 curl 访问本机 8765 如果报 HTTP=000，先看系统代理，要加 --noproxy "*" 才是直连（Invoke-WebRequest 同样受系统代理影响）。

## ⓘ 悬停提示（2026-11-21）

- `settings.rs` 的 `info(w, x, y, text)` 只往 `TIPS` 登记热区（坐标是「窗口坐标」，含 36px 自绘顶栏），图标与气泡都在 `paint()` 里画——不放子控件，父窗口才收得到 `WM_MOUSEMOVE`。
- 两个坑：① **热区不能被 STATIC/EDIT 子控件盖住**，否则鼠标事件被子控件截走、父窗口收不到移动消息（这是前几版「图标画得出来、气泡不出现」的原因）；② 气泡**不能画在热区下方**，下面的 EDIT/LISTBOX 是子窗口，会盖住父窗口画的内容——所以气泡优先画在图标右侧，右侧放不下才挪到上方。
- 验证：`/tmp/v50.png` 里悬停「按应用覆盖」旁 ⓘ 出现气泡「一行一个程序：粘贴=模拟 Ctrl+V…」，见 `docs/settings-win-tooltip.png`。

## 界面重做（2026-11-21 凌晨，进行中）

- 设计依据：`design/settings.html` / `design/settings.png`（stitch 导出，2560×2048）。token：正文 13px、小字 11px、小节 14px、标题 20px；卡片圆角 12、窗口圆角 16、控件圆角 6~8；主色 `#3b82f6`；背景 `#1b1c1f`、卡片 `#24262c`、输入框 `#2e3038`。
- 代码：新增 `shell-win/src/ui.rs`（`BODY/CAPTION/SECTION/TITLE`、DPI `scale()`、`px()`、`font()`）；设置页改成无边框 `WS_POPUP` 900×632，`CreateRoundRectRgn` + `SetWindowRgn` 切圆角，`CS_DROPSHADOW` 投影；自绘 36px 顶栏（标题 + ✕，`WM_NCHITTEST` 返回 `HTCAPTION` 可拖动）；`child()` 给控件统一加 `HEADER` 偏移与圆角 region。
- 页面结构：行式布局（左边标签、右边控件），通用/引擎/音频/热词/历史/润色/诊断/声纹/局域网 9 页共用同一套 `label/edit/button/checkbox/section`。
- 迷你窗：同套设计（DPI 缩放、14px 圆角、MDL2 话筒字形、两行提示、今日节省），`docs/mini-window-win.png`。
- 踩坑：GDI 里 emoji 是代理对会画成方框 → 用 `Segoe MDL2 Assets`（E720）或 GDI 图元；owner-draw 按钮别用 `BM_SETCHECK`（I-106）；`SetWindowRgn` 在 `windows-0.58` 属 `Graphics::Gdi` 且参数是 `HRGN` 不是 `Option<HRGN>`。

## 自测入口速查（Windows）

- `--self-test`：跑一句固定文本，验证引擎与上屏链路（不写历史）
- `--hotkey-sim --hotkey-sim-force --hotkey-sim-secs N`：模拟按住热键 N 秒（VM 里合成键会被钩子忽略，所以用这个内部入口）
- `--hotkey-sim-repeat K`：同一次运行里连跑 K 轮「按下 → 采音 → 松开」，用来验证连续听写不卡死、状态不残留
- `--settings-pages-smoke` / `--settings-cycle N`：九页逐页构建、N 次重建后的 GDI/USER 句柄对比
- `--tray-menu-dump`：托盘菜单项回读；`--inject-test [文本]`：直接验证注入实现
- `ERYU_TRACE=1`：关键路径（事件进出、注入线程、APP 锁）打印 `[eryu][trace]`，定位卡死用

## 稳定性实测（Windows，2026-11-21）

在 VM（Win11 ARM，VMware 虚拟声卡）里用 `ERYU_MIC_WAV` 固定 WAV 当麦克风、`--hotkey-sim*` 模拟按住热键，跑出来的可复现数据：

| 项目 | 命令 | 结果 |
| --- | --- | --- |
| 连续听写 6 轮 | `--hotkey-sim-repeat 6 --hotkey-sim-secs 2` | 6 次 `上屏 status=ok`；`Responding` 全程 True；工作集 348~349MB 无增长；句柄 398→385、线程 17→14（收尾后回落） |
| 九页逐页点击 | 手工点击导航（脚本模拟鼠标） | 每页点击后 `Responding=True`；✕ 关闭后窗口隐藏、进程仍活 |
| 设置页重建 200 次 | `--settings-cycle 200` | GDI `-1`、USER `-9`（修复 I-117 前是 +23/+103） |
| 注入自测 | `--inject-test` / `--inject-test-hold 5` | 两种路径都能把文字写进目标输入框 |

注意：VM 里合成按键会被低级键盘钩子按 `LLKHF_INJECTED` 主动忽略（避免自家合成键触发自家热键），所以自动化一律走 `--hotkey-sim`；真实人手按键不受影响，但观感仍需要真机确认。

### 安装与便携模式实测（2026-11-21）

| 项目 | 做法 | 结果 |
| --- | --- | --- |
| 覆盖安装 | 同一 zip 连装两次 | 两次 exe SHA256 一致（`4275F483…`），开始菜单快捷方式在 |
| 卸载保数据 | `uninstall-win.ps1` | 程序目录与快捷方式清掉，`%APPDATA%\Eryu\config.toml` 保留 |
| 卸载清自启 | 先 `--autostart-on` 再卸载 | 卸载前 `RUN_BEFORE="…\Programs\Eryu\Eryu.exe"`，卸载后 `RUN_AFTER=[]`；重装后 `REINSTALLED=True`、`SELFTEST OK` |
| 绿色便携 | `Eryu.exe` 旁放 `portable.txt` 后 `--set-config` | 数据写到 `EryuData\config.toml`，`%APPDATA%\Eryu` 未被触碰 |
| 150% 缩放 | `ERYU_SETTINGS_DPI=150` | 设置窗 994×698 完整落在 1470×770 工作区（`docs/settings-win-dpi150.png`） |

- 托盘自愈：explorer 重启后 5 秒内自动补回图标（`--tray-drop-test` 验证 `TRAYDROP dropped=true back=true`，I-128）。
- 模型缺失：不再上屏 stub 占位文本，改为浮层/托盘提示「语音模型还没下载」（I-129，`core/asr.rs` + `core/lib.rs`）。
- 崩溃恢复：上次没上屏的文字在下次启动时进剪贴板并提示（与 mac `pending_result` 同语义；Windows 侧用启动兜底读盘，见 `main.rs::recover_pending_once`）。
## 启动耗时（实测，2026-11-21）

- Windows 启动到迷你窗出现（VM，100% 缩放，SenseVoice 已就绪）：冷启动 **4312 / 6057ms**（复制新 exe 后首次，文件缓存冷），热启动 **1628–2029ms**；其中模型加载约 3.2s，发生在 `EryuCore::start` 里，是同步的。
- 尝试过把引擎加载挪到后台线程、先显示窗口：冷启动能快，但会让「开机后立刻按住热键」拿到「还在加载」错误，冒烟里的 SOAK / 隐私 / 崩溃恢复等项也全部依赖「start 返回即引擎可用」，**已回退**（见 `docs/ISSUES.md` I-130 的记录）。真要做，得先加一个明确的「引擎就绪」握手，壳与冒烟都等它。
- macOS 侧同口径基线（`scripts/smoke.sh` 的 BASELINE）：冷启动 466ms。
- 配置容错：`config.toml` 解析失败会备份为 `config.toml.broken`，Windows 托盘/浮层提示（I-131）。
- 模型下载：下完先加载校验（坏文件删除并提示重试）；内置镜像缺文件（gtcrn/punct/speaker 实测 404）时自动退回官方源（I-103）。
- 引导模型页：下载时显示「正在下载：N%」（VM 实拍 `docs/onboarding-win-download.png`）；未安装的模型不再标「使用中」。
- 引导试说：缺模型时提示「还没有可用的语音模型」（实拍 `docs/onboarding-win-probe-nomodel.png`）。
- 引导麦克风页：系统权限被拒时显示「系统麦克风权限被关了」（实拍 `docs/onboarding-win-mic-denied.png`）；引导期间浮层与迷你窗均不显示。
- 引导期间浮层/迷你窗都不出现（打开引导时立即 `hide_now`）。
- 浮层提示行：录音中显示「在听… Ns · 松开上屏 · Esc 取消」（读 meter 事件的 `secs`）。
- 局域网页令牌：密码框 + 「令牌（已设置）」；留空保存＝不改（`lan_has_token`，对齐 mac 的 SecureField）。
- `--settings-tab-demo N` / `--settings-key-demo <id> [saveId]`：设置页键盘可达自测（焦点序列 / 激活控件）。
- `--onboarding-key-demo N`：引导页键盘可达自测（Tab 焦点序列）。

### 局域网鉴权实测（2026-11-21）
VM 安装目录构建上逐项验证：
- 无 token 请求 → `403 {"code":"forbidden"}`；带 `x-eryu-token` → `200 {"ok":true,"text":…}`；
- 非 WAV body → `400 {"code":"bad_wav"}`；带 token 的 `GET /health` → `200 {"ok":true,"engine":"sense-voice-int8","vad_ready":true}`；
- `allow_lan = true` 但没设 token 时**拒绝启动**并打印原因（安全设计）；补上 token 后监听 `0.0.0.0:8793`，仅本机时监听 `127.0.0.1`。
- `install-win.ps1 -Desktop` 会额外在桌面建快捷方式（实测 `LNK_START=True LNK_DESKTOP=True`）。
- `uninstall-win.ps1 -PurgeData` 会连配置/模型/历史一起删（实测 `exe=False data=False desk=False`），重装后是全新状态。
- 2026-11-21 全量回归：`SELFTEST OK`、`PAGES done`、`--settings-cycle 100` → `GDI delta=0 / USER delta=0`、PROBE 8/8、托盘 11 项 + 2 分隔线（`--dev` 多「自测…」、权限报告、引导 Tab 焦点序列 204/202/200。
- 引导窗按工作区收缩放（与设置窗同一 `fit_scale`），150% 模拟下整窗落在屏幕内。

## 启动耗时实测（2026-11-21，VM 冷/热启动各三次）

`ERYU_TRACE=1` 时壳会往 `%APPDATA%\Eryu\eryu-cli.log` 写两行时间点：「核心就绪」= 引擎加载完、可以开始听；「界面就绪」= 托盘/热键钩子装好、进入消息循环。

| 次数 | 核心就绪 | 界面就绪 |
| --- | --- | --- |
| 1 | 1821ms | 1910ms |
| 2 | 1730ms | 1816ms |
| 3 | 1365ms | 1419ms |

结论：VM（共享磁盘、单核性能受限）里 1.4–1.9s；其中壳自身只有约 50–90ms，绝大部分是 SenseVoice 加载（I-130 的同步加载，异步化需要显式「引擎就绪」握手，留到 0.8.2）。真机 SSD 上会更短。这段时间里进程没有窗口，托盘图标出来即可按热键。

## 浮层提示行可读性（2026-11-21）

- 提示行没有底色，颜色按**窗口下方壁纸**的实际亮度选：`<128` 用近白、否则近黑（采不到桌面像素时退回系统主题初判）。
- 笔画 alpha = 白字覆盖率 × 1.5（截顶），避免 12.5pt 小字抗锯齿后发虚。
- 证据：`docs/overlay-win-hint.png`（Win11 默认蓝壁纸，100% 缩放）。

## 首次可用：模型怎么给（2026-11-21，R-124）

- 安装包（`dist-win/Eryu-0.8.1-win64.zip`）内含 `models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09`（233MB），`install-win.ps1` 拷到 `%APPDATA%\Eryu\models`（已存在跳过，不覆盖用户自己的模型）。
- VAD（`silero_vad.onnx` 643854B）与降噪（`gtcrn_simple.onnx` 535638B）用 `include_bytes!` 编进 exe，`Core::load` 前 `ensure_bundled_models()` 解到数据目录；用户不需要知道它们存在。
- 引擎页只列 SenseVoice / Qwen3 两项；标点、声纹在打开对应开关时自动下载（托盘提示进度），下载事件 `model_done` 会解除引导的灰显。
- 引导判定：`asr_ready(core)`（`models_json` 中非 stub、`engine=true`、`installed=true`）为假时一律停在 STEP_MODEL，「下一步」灰显并弹框说明；截图 `docs/onboarding-win-model-ready.png`、`docs/onboarding-win-probe.png`、`docs/settings-win-engine-2engines.png`。
- 回归（装机目录构建 `B2D4BCB8…`）：`ERYU_MIC_WAV=probe.wav` + `ERYU_POLISH_IN_SIM=1` + 隐藏启动 → 记事本出现「润色后：今天天气不错，我们开始吧。」，`history.json` `inserted / Notepad.exe / 17 字`。

## 字号与清晰度（2026-11-21，R-125 / I-145）

- `--dpi-report` 自测：VM 上 `DPI awareness=2`（PER_MONITOR_AWARE_V2）、`system=96` → 不是 DWM 缩放导致发虚。
- 字号：设置正文 13→14、说明 11→12、小节 14→15；引导正文/小字同步；选区浮层标题 11→12。
- 胶囊（分层窗口）：字体由 ClearType 改灰度抗锯齿（`ANTIALIASED_QUALITY`），提示行 alpha 曲线改 `x^1.7`、门限 24，消除灰雾/重影；截图 `docs/overlay-win-capsule.png`。
- 全新安装（`uninstall -PurgeData` 后解压新 zip → `install-win.ps1 -Desktop` → 直接双击）：首屏就是「选一个识别模型」且显示「语音模型已就绪（随安装包一起装好）」，无需下载任何东西即可下一步；截图 `docs/onboarding-win-firstrun.png`（打包 exe SHA256 `31EE50E5…`，zip 169MB）。

## 逐字打字（上屏方式=keystroke）实测（2026-11-21）

- 成功路径：UIA `Document.SetFocus()` 把 Win11 记事本设成前台 → `--hotkey-sim --hotkey-sim-force --hotkey-sim-delay 8000` → 日志 `type fg_class=RichEditD2DPT`、`verify #0 got="润色后：今天天气不错，我们开始吧。今天天气不错。"`、`上屏 status=ok`，记事本正文确实多出这段文字。
- 拒打路径：前台只剩任务栏（`Shell_TrayWnd`）时 → `上屏 status=undelivered detail=焦点在桌面或任务栏上：先把光标放进要写字的输入框，再说一次`（I-148）。
- 回读：`wait_contains()` 每 70ms 读一次、最多 ~700ms，WinUI 的异步合成不会再被误判成 `unknown`/`pending`。
- 测试要点：隐藏启动的 PowerShell 会抢前台，所以要先 `SetFocus` 再延后触发热键；`--hotkey-sim-delay` 就是为此加的。

## 安装/卸载/重装/全新安装实测（2026-11-21，装机目录）

用 `dist-win/Eryu-0.8.1-win64.zip` 解压出来的脚本，在 VM 上按用户会遇到的四条路径走了一遍：

| 路径 | 结果 |
| --- | --- |
| 卸载（不带 `-PurgeData`） | `UNINSTALL OK`；`Programs\Eryu\Eryu.exe` 与开始菜单快捷方式消失；`%APPDATA%\Eryu`（config.toml / history.json / models）**全部保留** |
| 重装 | `INSTALL OK`；exe + 开始菜单 + 桌面快捷方式回来；历史仍是 16 条；模型输出「语音模型已存在」（不重拷 226MB）；`--self-test` OK |
| 卸载 `-PurgeData` | 提示「目录里的文件已全部删掉」；exe、`%APPDATA%\Eryu`、桌面快捷方式全部消失 |
| 全新安装（干净机器） | `INSTALL OK`；模型 237115547 字节就位；首屏引导直接是「语音模型已就绪（随安装包装好）」（截图 `docs/onboarding-win-fresh-model.png`），只有「下一步/稍后再说」 |

## 连续听写稳定性（soak，2026-11-21 装机构建）

单进程连续 8 轮听写（`--hotkey-sim --hotkey-sim-force --hotkey-sim-secs 3 --hotkey-sim-repeat 8`，`ERYU_MIC_WAV=probe.wav`，记事本前台），每 3 秒采样句柄/内存：

- 句柄：290 → 峰值 392（引擎加载那一秒）→ 稳定在 363–391，20 个采样点无单向增长
- 内存：353MB → 377MB 后走平
- `eryu-cli.log` 无 panic；进程在该模式（`--hotkey-sim` 不自动退出）下一直存活
- 记事本正文最终为「今天天气不错我们开始吧。」，历史新增条目均为 `inserted`
- 注意：模拟 WAV 是**按会话顺序消耗**的，第一轮拿到整句，后续几轮拿到的是尾巴（历史里 `chars=3~4` 就是碎片），这是测试夹具行为，不是识别退化；真机是每轮重新录音，不存在这个问题

## 引导全流程实测（2026-11-21，装机构建）

从「模型未就绪」到「开始使用」整条路径，用真实事件驱动走了一遍（不是逐个 --onboarding-step 摆拍）：

1. 清掉 `onboarding_done` / `onboarding_probe_done`，`--onboarding-step 4` + `ERYU_MIC_WAV=probe.wav` + 模拟热键 → 试说页显示识别结果「今天天气不错。」、提示「读到了，点「下一步」继续。」、下一步由灰变蓝（`docs/onboarding-win-probe-passed.png`）。
2. 点「下一步」→ 润色页（`docs/onboarding-win-polish.png`，六颗进度点最后一颗点亮，按钮「开始使用」）。
3. 点「开始使用」→ 引导窗关闭，`config.toml` 写成 `onboarding_done = true` / `onboarding_probe_done = true`。
4. 再启动一次：不再弹引导（`open_if_needed` 见两个标记齐了就跳过）。实测枚举可见顶层窗口只有 `EryuMini`，`ONBOARDING_VISIBLE=False`。

顺带一个 VM 测试坑：用 `FindWindow("EryuOnboarding", null)` 在 PowerShell 里返回 0，但 `EnumWindows` + `GetClassName` 能拿到同一个窗口（句柄 `0x170430`）——以后按类名找窗口一律用枚举，不要用 `FindWindow`。

### 按应用覆盖上屏方式（2026-11-21）

- **规则**：`[insert] mode` 是全局默认，`[insert] per_app` 按前台程序名覆盖（包含匹配、不区分大小写）。判定在 `core/src/config.rs::mode_for`，结果随 `submit_request` 的 `mode` 字段交给壳，壳只按 `mode` 走粘贴或逐字打字。
- **已验证**：`mode_for` 有单测覆盖 `notepad.exe` / `NOTEPAD.EXE` / `Code.exe` / `chrome.exe` / 空串与回退（`core/src/config.rs` 的 `per_app_mode_matches_and_falls_back`）；壳侧两条路径各自端到端通过——逐字打字把「润色后：今天天气不错，我们开始吧。」真的打进记事本（`fg_class=RichEditD2DPT`、`status=ok`），粘贴路径同样落字；焦点在桌面/任务栏时两条路径都报 `undelivered`（I-148）。
- **没能在 VM 里自动验的一段**：真实热键 → 引擎取前台程序名 → 覆盖生效 → 对应的那条路上屏。原因是 VM 里 `runProgramInGuest` 起的进程会让前台掉到 `Progman`（桌面），`SetForegroundWindow` 拿不回记事本；测出来的 `app` 永远是 `explorer.exe`，覆盖规则没机会命中。这是虚拟机前台控制的限制，不是产品行为。
- **给测试用的入口**：`--inject-test <文本> --mode paste|keystroke` 可直接验任一条注入路径（默认 paste）。
- **真机确认**：见 `docs/win-manual-test.md` D 组——先在设置里给记事本配 `keystroke`、全局留 `paste`，然后在记事本里说一句、在终端里说一句，两句都应落字，且设置页历史里的「方式」随程序不同。

## 录音中途麦克风被拔掉/被占用（2026-11-21）

- **设计行为**（DESIGN §10.1 第 7 条）：录音中设备消失不丢已录内容——立刻收尾，把中断前录到的部分送去识别并上屏。
- **代码位置**：`core/src/lib.rs` 的 `tick()` 里检测 `Capture::has_failed()`（真实设备消失）与 `ERYU_FORCE_MIC_FAIL_MS`（测试钩子），命中后 `emit_error("mic_unplugged", …)` 并 `m.up()` 走 `Finish` 分支解码；错误文案「麦克风中断了（可能被拔掉或被其他应用占用）：已保留中断前录到的内容」。
- **VM 实测**（装机构建，`ERYU_MIC_WAV=probe.wav` + `--hotkey-sim --hotkey-sim-force --hotkey-sim-secs 3` + `ERYU_FORCE_MIC_FAIL_MS=1500`）：日志出现「麦克风中断了（模拟拔掉）：已保留中断前录到的内容」，随后 `processing` → `inserting`；`history.json` 新增一条（`text="今天天气"`、4 字，正是中断前那 1.5 秒的内容），进程存活、回到 idle，没有卡在「录音中」。
- **状态判定**：这次 VM 里前台是桌面（记事本没抢到前台），所以那条历史记为 `app=explorer.exe` / `status=pending`——上屏没落地就报 pending，是设计里的正确行为（不会假报成功，见 I-112/I-148）。
- **真机确认**：见 `docs/win-manual-test.md` F 组「录到一半拔掉麦克风」。

## 托盘菜单动作（2026-11-21）

- 托盘菜单项（`--tray-menu-dump` 可打印）：复制上次结果 / 统计与历史 / 设置 / 选区改写 / 诊断 / 重识别失败的段 / 关于 / 显示主窗 / 自测 / 测试麦克风 / 麦克风权限 / 退出。
- 这些动作原先只能靠人工点菜单触发，自动化验不到。新增 `--tray-cmd <id>`（id 见 `shell-win/src/tray.rs` 的 `CMD_*`），把菜单点击和命令行都走同一个 `tray::run_cmd(hwnd, cmd)`，命令行能验到「显示主窗 / 设置 / 退出」。
- **VM 实测**（装机构建 `30E100B2…`）：`--tray-cmd 1009` → `TRAYCMD id=1009 mini_before=true mini_after=false`，再跑一次 `false → true`；`--tray-cmd 1004` → 日志 `SETTINGS create 成功`；`--tray-cmd 1003`（退出）→ 进程退出（`LEFTOVER=0`）、`history.json` 仍是合法 JSON（31 条）、目录里没有残留 `*.tmp`。
- **写这个入口时踩的坑（教训）**：`if let Some(hwnd) = crate::lock(&APP).as_ref().map(|a| a.hwnd) { … }` 会把 `MutexGuard` 的临时值留到整个 if-let 语句结束——`run_cmd` 里的动作再取 `APP` 锁就自锁挂死（现象：打印 `TRAYCMD start` 后没有下文、进程不退）。改成先把 hwnd 取到局部变量、守卫在那一行就释放，再调用。

## 设置窗 150% 缩放实测（2026-11-21，`ERYU_SETTINGS_DPI=150` 模拟）

- 窗口 `rect=238,36,1232,734`（994×698），`fit_scale` 把 900×632 逻辑尺寸按「屏幕放得下」缩到 1.10，而不是硬乘 1.5 撑出屏幕。
- 通用页：八个控件、两个自绘开关、「保存」、按应用覆盖编辑框与脚注全部在窗口内，无裁切、无重叠（截图 `docs/settings-win-dpi150-general.png`）。
- 音频页：设备列表、四个开关（含 ⓘ）、保存、电平条与「开始测试」同一屏，无溢出（截图 `docs/settings-win-dpi150-audio.png`）。
- 结论：这一档缩放下的**排版**没问题；用户报的「重影」若存在，更可能来自具体显示器/缩放组合，需要真机截图定位（仍待 ④ 的具体窗口与比例）。

## 缩放模拟的上限（2026-11-21，重要）

- `ERYU_SETTINGS_DPI=125/150/200` 三次测下来，窗口都是 `994×698`（`k=1.1044304`）：`fit_scale` 会按「屏幕放得下」收，而 VM 屏幕只有 1470×770，所以任何大于 ~110% 的请求都被同一档夹住。
- 结论：**VM 里只能验证到约 110% 这一档**，125%/200% 的观感必须在真机上（4K/2K 显示器）看；这条已写进验收矩阵「自动化验不到的六类」。
- 已验到的部分：994×698 下通用/音频/历史三页都没有裁切与重叠（历史页 8 行 + 四个按钮 + 记录历史开关同屏）。
- 另记一笔：历史页里 `status=pending` 显示成「待复制」，但其中有几条（`app=SearchHost.exe`）文字其实已经进了搜索框，只是回读确认不到——这个措辞和 mac 一致，等 0.8.2 再考虑改叫「未确认上屏」。

## 设置窗关闭方式实测（2026-11-21）

- `--open-settings-page 0` → 窗口可见；投 `WM_KEYDOWN VK_ESCAPE(0x1B)` → `VISIBLE=False`、进程存活（只是隐藏，不退出，热键继续可用）。
- 投 `WM_SYSCOMMAND SC_CLOSE(0xF060)`（等价 Alt+F4）→ 同样隐藏、进程存活。
- 两种都只隐藏不销毁：再点托盘「设置」能立刻回来（同一窗口句柄）。

## 自家窗口输入（R-137，2026-11-22）

mac 侧本轮修了两个「只有 mac 会中」的坑：热词行身份由内容拼成导致每敲一字重建行（I-174）、菜单栏 app 没有主菜单导致自家窗口 ⌘V 全失效（I-175）。Windows 侧逐条对照并实测：

| 关注点 | Windows 结构 | 结论 |
| --- | --- | --- |
| 行编辑时重建 | 热词页是**一个多行 EDIT**（`ID_HW_EDIT`），没有「每行一个控件」，`WM_COMMAND` 里也没有 `EN_CHANGE → rebuild` | 结构上不可能出现「打字丢焦点」 |
| 自家窗口粘贴 | `EDIT` 原生处理 `Ctrl+V`，不依赖菜单命令派发（`WM_COMMAND` 无 `ID_EDIT_PASTE` 依赖） | 不需要补菜单 |
| 往自己窗口上屏 | `inject::paste` 取前台焦点控件 → UIA `SetValue`/`Ctrl+V` → 用 `WM_GETTEXT`（同进程）读回 | 能读回，状态给 `ok` |

**实测**（装机目录 exe `EDBDD2D8064FFA2D`，自测入口 `--dev --settings-input-selftest`）：

```text
INPUT typed_ok=true typed_tail="abcde" inject=ok inject_ok=true after_len=12 detail=
```

自测流程：打开「热词」页 → 对 `e_hotwords` 句柄 `SetFocus` → 逐字 `SendMessageW(WM_CHAR, 'a'..'e')`（每次 60ms）→ 读回尾部为 `abcde`（页面没重建、焦点没丢）→ 把设置窗设为前台 → `inject::paste("注入自测ABC", "paste")` → 读回含该串（`inject_ok=true`）→ 还原原内容。同一入口留在仓库里，以后每次改设置页都能一条命令回归。
