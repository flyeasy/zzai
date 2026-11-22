#!/usr/bin/env bash
# 打一个能直接拖进「应用程序」的包：build → 拷到 dist/ → zip（保留签名与扩展属性）
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' packaging/Info.plist)"
./scripts/build.sh

# 发版闸门：产物必须在检查全绿之后才打（ERYU_SKIP_GATE=1 可跳过，仅限调试）
if [ "${ERYU_SKIP_GATE:-0}" != "1" ]; then
  echo
  echo "==> 发版闸门（ERYU_GATE_FAST=1 可只跑快的）"
  ./scripts/release_gate.sh || { echo "闸门没过，不打包"; exit 1; }
fi

rm -rf dist
mkdir -p dist
ditto build/Eryu.app dist/Eryu.app
ZIP="dist/Eryu-${VERSION}.zip"
ditto -c -k --sequesterRsrc --keepParent dist/Eryu.app "$ZIP"

echo
echo "==> 产物"
echo "  dist/Eryu.app"
echo "  $ZIP  （$(du -h "$ZIP" | cut -f1)）"
echo "  sha256：$(shasum -a 256 "$ZIP" | cut -d' ' -f1)"
echo
echo "安装："
echo "  cp -R dist/Eryu.app /Applications/ && open /Applications/Eryu.app"
echo "首次启动 macOS 会问「是否允许来自开发者 … 的应用」，因为它是自签名（Eryu Dev），点「打开」。"
