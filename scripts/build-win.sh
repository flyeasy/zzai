#!/usr/bin/env bash
# 构建 Windows 绿色单文件 exe（core 的 dll 会被编进 exe，运行时自动解出）
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET=x86_64-pc-windows-msvc
WINLIB="${SHERPA_ONNX_LIB_DIR:-/tmp/winlibs/sherpa-onnx-v2.0.1-win-x64-static-MD-Release-lib/lib}"
export SHERPA_ONNX_LIB_DIR="$WINLIB"
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"

echo "==> 1/3 构建 core.dll"
(cd core && cargo xwin build --release --target "$TARGET")

echo "==> 2/3 把 dll 打进 exe 资源"
cp core/target/$TARGET/release/eryu_core.dll shell-win/assets/eryu_core.dll

echo "==> 3/3 构建绿色 exe"
(cd shell-win && cargo xwin build --release --target "$TARGET")
OUT=dist-win
mkdir -p "$OUT"
cp shell-win/target/$TARGET/release/eryu-shell-win.exe "$OUT/Eryu.exe"
ls -lh "$OUT/Eryu.exe"
echo "单文件：$OUT/Eryu.exe（数据默认在 %APPDATA%\\Eryu；exe 旁边放 portable.txt 或 EryuData 目录就变成绿色便携）"
