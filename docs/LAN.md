# 用手机听写：局域网接口怎么用

Eryu 可以在本机开一个局域网 HTTP 接口：**手机录一段音，Eryu 在 Mac 上识别，把文字回给你**。
识别用的是 Mac 上已下载的本地模型（不上传云端），接口本身不上屏、不写历史、不碰剪贴板。

## 一、在 Mac 上打开

1. 设置 → 音频 → 「局域网听写接口（实验）」→ 打开开关。
2. 设一个 **token**（随便一串，手机端要用）。
3. 想给别的设备用就开「允许局域网访问」。**开着却没设 token 时接口会拒绝启动**（这是故意的）。
4. 重启 Eryu 生效。

查地址：

```bash
/Applications/Eryu.app/Contents/MacOS/Eryu --lan-info
```

输出里优先用**「同一 Wi-Fi（推荐）」**那一行，例如：

```
http://luciendemacbook-air-2.local:8765/
```

`.local` 是 macOS 自带的 Bonjour 名字，同一个 Wi-Fi 下你手机能直接打开，**不用记 IP**（路由器换地址也不受影响）。

## 二、三条路，按需要选

### A. 手机浏览器（最快，但录音受限）

Safari 打开上面那个地址 → 点「选文件」挑一段录音 → 转文字。

> 浏览器只在 **https 或 localhost** 时才给网页录音权限。用 `http://…local` 打开时**不能**在网页里直接录音（这是浏览器的规矩，不是 Eryu 的问题），但**选文件**一直可用。

### B. iOS 快捷指令（能直接录音，推荐日常用）

手机「快捷指令」App → 新建：

1. **录音** → 音频质量随意（Eryu 会自己重采样到 16k）
2. **获取 URL 内容**
   - URL：`http://你的Mac名.local:8765/transcribe`
   - 方法：**POST**
   - 请求头：`x-eryu-token` = 你设的 token
   - 请求体：**文件** → 选上一步的录音
3. **获取词典值** → 键 `text`（返回的是 JSON）
4. **显示结果** / **拷贝到剪贴板**

把这串存成快捷指令，放桌面或加到「辅助功能 → 轻点背面两下」，按一下就说完。

### C. 电脑上的脚本

```bash
curl -s -H "x-eryu-token: 你的token" \
     --data-binary @录音.wav \
     http://你的Mac名.local:8765/transcribe
# {"ok":true,"text":"今天下午三点在会议室评审设计文档","ms":364,"segments":1,"chars":16}
```

`录音.wav` 必须是 **16kHz 单声道 16-bit PCM**（用 `afconvert -f WAVE -d LEI16@16000 -c 1 in.m4a out.wav` 转）。

## 三、接口

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/health` | 探活：`{"ok":true,"engine":…,"vad_ready":true,"protocol":1}` |
| POST | `/transcribe` | body = 16k 单声道 WAV；回 `{"ok":true,"text":…,"ms":…,"segments":…,"chars":…}` |
| GET | `/` | 网页版（**不需要 token**，否则手机打开就 403 没地方填） |

错误码：`bad_request` / `forbidden` / `empty_body` / `too_large`（>25MB）/ `bad_wav` / `too_short` / `decode_failed` / `not_found`。

## 四、出问题怎么查

设置 → 诊断 → 「最近的局域网请求」：只有时间 / 来源 / 路径 / 状态码 / 耗时，**没有内容**。

| 状态码 | 意思 | 怎么办 |
|---|---|---|
| 200 | 成功 | — |
| 403 | token 不对或没带 | 请求头加 `x-eryu-token` |
| 503 `busy` | 同时已经 2 个在跑 | 等一下再发；批量要一条一条来 |
| 400 `bad_wav` | 不是 16k 单声道 WAV | 先 `afconvert` 转一下 |
| 408 `timeout` | 传得太慢（180 秒） | 换近一点的 Wi-Fi / 别传超大文件 |

命令行：`/Applications/Eryu.app/Contents/MacOS/Eryu --lan-log`（看当前进程），或 `--lan-serve 60` 跑一分钟，
退出时会打印 `LANLOG [...]`。

## 五、别做的三件事

- **别把 8765 端口映射到公网。** 这是给「同一个 Wi-Fi」用的，没有 TLS、没有限流。
- **别关 token 又开「允许局域网」**——接口会拒绝启动（`lan_no_token`），这是保护你。
- 识别期间 Mac 上的语音输入会**排队**（共用同一个引擎），不会并行多占内存；要并发就等这一条返回。
