# 发布前必跑清单

原则：**能机器判定的，绝不靠人眼**。下面每一项都有对应脚本与可预期的输出。

## 一、一条命令

```bash
./scripts/release_gate.sh          # 全量（6–12 分钟）
ERYU_GATE_FAST=1 ./scripts/release_gate.sh   # 只跑快的（约 2–4 分钟）
```

`scripts/dist.sh` 默认先跑闸门，闸门不过不打包（`ERYU_SKIP_GATE=1` 只在调试时用）。

## 二、闸门跑什么

| 检查 | 脚本 | 失败意味着 |
|---|---|---|
| 单元 + FFI + 容错测试 | `cd core && cargo test` | 核心逻辑或 ABI 契约被破坏 |
| 文档一致性 | `scripts/doccheck.sh` | ABI 头/版本号/错误码表/默认值不同步 |
| 极端状态矩阵 | `scripts/matrix.sh` | 缺模型、坏设备、坏配置、只读目录下行为退化 |
| 合成验收（六项） | `--acceptance-synthetic` | 权限/热键/浮层/流式/结果/延迟任一环断 |
| 上屏失败兜底 | `scripts/fallback.sh` | 失败后结果丢失（用户打字没了） |
| 隐私模式 | `scripts/privacy_check.sh` | 历史/统计/暂存/失败音频/菜单任一留着正文 |
| 权限降级 | `scripts/degrade.sh` | 权限不足时静默或崩溃 |
| 两分钟上限 | `scripts/longcap.sh` | 长听写截断/延迟失控 |
| 连续说话 | `scripts/nopause.py` | 无停顿语料丢整句 |
| 录音中拔麦 | `scripts/mic_unplug.sh` | 断开后已录内容丢失 |
| 换输入设备 | `scripts/device_switch.sh` | 换设备没生效 |

## 三、机器替代不了的部分（每次发版请人手动确认一次）

1. `open build/Eryu.app` → 系统弹「想访问麦克风」→ 允许；菜单 → 设置 → 诊断，看辅助功能/输入监控是否需要勾选。
2. 按住右 Option 说一句完整的话，确认：
   - 浮层在光标所在屏出现，有电平条和逐字文本；
   - 松手后文字落到当前输入框，且**没有**重复或吞字；
   - 菜单「上次：松手→出字 Nms」≤1500ms。
3. 在密码框里按住热键，确认不会把密码内容读成文本（系统安全输入下应静默不触发）。
4. 播放音乐，按住热键，确认（如开启）音乐暂停、松手恢复。

## 四、打包与分发

```bash
./scripts/dist.sh
```

产物 `dist/Eryu-<版本>.zip`，含：

- 自签名（`Eryu Dev`）——首次启动 macOS 会拦一次，需在「隐私与安全性」点「仍要打开」；
- 未公证：换证书或重装都会让 TCC 授权失效（`docs/ISSUES.md` I-045）；
- 首次启动若缺模型，会弹窗问是否现在下载（`docs/ISSUES.md` 相关记录）。

## 五、版本号与变更记录

发版时必须同时改三处，`doccheck.sh` 会校验前两处一致：

1. `packaging/Info.plist` 的 `CFBundleShortVersionString` / `CFBundleVersion`
2. `CHANGELOG.md` 顶部新增一节
3. `DESIGN.md` 版本表追加一行（做了什么 / 证据）
