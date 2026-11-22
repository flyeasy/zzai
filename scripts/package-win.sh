#!/bin/bash
# 把 Windows 版打成可分发 zip：exe + 安装/卸载脚本 + 说明
set -euo pipefail
cd "$(dirname "$0")/.."
VER=$(awk -F'"' '/PRODUCT_VERSION/{print $2}' shell-win/src/main.rs | grep -o '^[0-9.]*')
OUT="dist-win/Eryu-${VER}-win64"
rm -rf "$OUT" && mkdir -p "$OUT"
cp dist-win/Eryu.exe "$OUT/Eryu.exe"

# 把主力语音模型一起打进 zip：装完就能直接说话，不用再等 233MB 下载。
# （VAD/降噪两个小模型已经编进 exe 里了，这里只带 SenseVoice。）
MODEL_SRC="${ERYU_MODELS_DIR:-$HOME/.eryu/models}/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"
if [ -d "$MODEL_SRC" ]; then
  mkdir -p "$OUT/models"
  # 只带推理要用的文件：样例音频（test_wavs/）对用户没用，不塞进安装包
  rsync -a --exclude 'test_wavs' "$MODEL_SRC" "$OUT/models/"
  echo "已附带语音模型：$(du -sh "$OUT/models" | awk '{print $1}')"
else
  echo "提示：本机没有 SenseVoice 模型（$MODEL_SRC），这次打的包不含模型，用户需在设置里下载。"
fi
cp scripts/install-win.ps1 scripts/uninstall-win.ps1 scripts/install-win.bat scripts/uninstall-win.bat "$OUT/"
# 双击友好：把 ps1 也改名成 README 里的叫法（bat 里引用的是带 -win 的名字，两个都留）
cat > "$OUT/README.txt" <<TXT
Eryu ${VER}（Windows 64 位，预览版）
====================================

安装
  1) 整个文件夹解压到任意位置（别直接在 zip 里点）
  2) 双击 install-win.bat（推荐；PowerShell 执行策略不拦它）
     或右键 install-win.ps1 →「使用 PowerShell 运行」
  3) 开始菜单搜「Eryu」启动；托盘图标右键可以设置/看历史

只想先试一下
  也可以直接双击 Eryu.exe（不建快捷方式，数据仍写在 %APPDATA%\Eryu）

便携模式
  在 Eryu.exe 旁边放一个空的 portable.txt（或建一个 EryuData 目录），
  数据就写在 exe 旁边，不碰 %APPDATA%。

卸载
  双击 uninstall-win.bat
  或：powershell -ExecutionPolicy Bypass -File uninstall-win.ps1
  加 -PurgeData 连配置、模型、历史一起删。

键盘操作
  设置页：Tab / Shift+Tab 移动焦点，回车或空格按下当前按钮，Esc 关闭窗口
          Ctrl+Tab / Ctrl+Shift+Tab 切换左侧页面，Ctrl+1..9 直接跳到第 N 页
  历史页：↑/↓ 选行，点「复制选中」或「删除选中」对该行操作

装完先自检这几条（有异常就按最后一节把状态发出来）
  1) 开始菜单启动，右下角出现托盘图标；第一次会走五步引导
  2) 在记事本里按住热键说一句，松开后文字出现在光标处
  3) 托盘右键 →「设置」：Tab 能走遍控件，Ctrl+Tab 能翻页，Esc 关闭
  4) 设置 →「音频」→「测试 3 秒」：说话时电平条在动
  5) 设置 →「历史」：能看到刚说的那句，可复制/删除
  6) 托盘右键 →「退出」后按热键没反应；再启动一切照旧

首次使用
  安装包已带语音模型（models\ 目录，约 226MB），装完直接能用，不需要另外下载。
  引导：确认模型已就绪 → 确认麦克风 → 设定快捷键 → 选上屏方式 → 试说一句。
  「试说一句」必须真的识别出文字才能点下一步（也可以先跳过，下次启动回到这一步）。

为什么不用下模型
  语音识别（SenseVoice）随这个压缩包一起给；降噪和静音检测已编进 exe。
  只有「标点」「声纹」「Qwen3 大模型」在打开对应开关/选中时会自动下载。

出问题看哪里
  托盘右键 →「设置」→ 左侧「诊断」：第一屏就是状态（引擎/模型/麦克风/历史），
  有问题点「复制状态」把这段发出来，或点「打开日志」看最后几行。
TXT
cd dist-win && rm -f "Eryu-${VER}-win64.zip" && zip -q -r "Eryu-${VER}-win64.zip" "Eryu-${VER}-win64"
echo "打包：dist-win/Eryu-${VER}-win64.zip"
ls -la "Eryu-${VER}-win64.zip"
