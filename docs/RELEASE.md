# Eryu 0.2.0 发布说明

个人自用的 macOS 本地语音输入工具：按住右 Option 说话，松手把文字打进当前应用。
识别、降噪、标点全部在本机跑；只有「润色 / 选区改写」会用到你填的 API。

## 产物

```bash
./scripts/dist.sh
# dist/Eryu.app             可直接拖进 /Applications
# dist/Eryu-0.2.0.zip       同内容压缩包（约 8.6MB，签名保留）
```

安装：

```bash
cp -R dist/Eryu.app /Applications/ && open /Applications/Eryu.app
```

## 首次启动三步

1. Gatekeeper 问「无法验证开发者」→ 允许打开（自签证书 `Eryu Dev`，未公证，见 I-045）。
2. 弹窗问要不要下载模型 → 建议下载：SenseVoice 247MB + 标点 65MB + 降噪 1MB（存在 `~/.eryu/models`，之后离线可用）。
3. 三项权限：麦克风（立刻生效）、辅助功能、输入监控（勾完若没反应，菜单里「退出并重新打开」）。

## 验收一条命令

```bash
/Applications/Eryu.app/Contents/MacOS/Eryu --acceptance
```

按住右 Option 说一句，六项应全 ✅（权限 / 接住按键 / 浮层 / 流式 / 结果 / 延迟≤1500ms）。
报告写在 `/tmp/eryu-acceptance.json`。

## 日常自测

| 命令 | 用途 |
|---|---|
| `scripts/smoke.sh` | 12 项冒烟（单测、ABI、自测、模拟全链路、长听写、睡眠、验收、链接、签名…） |
| `--simulate <wav>` | 合成按键 + WAV 当麦克风，走真实浮层/流式/上屏链路 |
| `--decode-wav <wav>` | 只跑识别链路（不按键） |
| `--bench` | 冷启动/延迟基线 |
| `--soak 10 <wav>` | 连跑 10 次看内存是否泄漏 |
| `scripts/bench.py` | 25 条金标 CER + 延迟（`--engine qwen3` 对比精度档） |
| `--diagnose` | 引擎/模型/设备/权限/内存/延迟一览 |
| `scripts/seam.py` | 接缝丢字率（8 句重复，数关键词）——发版前跑一次 |
| `scripts/sourcelock_metric.py` | 音源锁定命中/误伤——发版前跑一次 |

## 已知限制（都是明确选择，不是遗漏）

- **自回声**：外放音乐/视频时录音会拾到回声，实测会让模型幻觉出乱码句；峰值 <2% 现在一律不上屏（D-072）。
  建议戴耳机；不打算做 AEC（需要 process tap/虚拟声卡，I-046）。
- **降噪有边界**：稳态噪声（风扇/空调/键盘）开着更好；旁边有人说话时开着更差（CER 0.14→0.38），那种场合关掉或用音源锁定（D-069/I-043）。
- **音源锁定分不开重叠说话**：只有 VAD 段级别判断，两人同时说话只能整段留/丢（I-040）。
- **长句接缝偶尔掉一个字**：约 0.4 字/接缝，没做音频重叠（会把真重复误删，I-036）。
- **Windows 未验证**：core 可复用、壳能力对照表已写（`docs/WINDOWS.md`），但没有 Windows 机器（I-028）。
- **未公证**：换机器/重建证书后 TCC 授权会失效（I-013/I-045）。
- **繁简转换未做**：目标用户说简体；真要做得引 OpenCC（C 库）或内置词表，收益不明。
- **终端里跑自测会看到权限假阳性**：TCC 把终端启动的进程归因给终端（I-018/I-042）。

## 卸载与清理

```bash
rm -rf /Applications/Eryu.app
rm -rf ~/.eryu            # 配置、历史、模型（约 400MB）
```
系统设置 → 隐私与安全性 里可移除「辅助功能 / 输入监控」的 Eryu 条目。

## 升级

覆盖 `/Applications/Eryu.app` 即可；`~/.eryu` 里的配置/模型/历史保留。
用 `Eryu Dev` 证书签名时 TCC 授权跨重建保持；换了证书就要重新授权。
