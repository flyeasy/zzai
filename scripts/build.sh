#!/usr/bin/env bash
# 构建 Eryu.app：Rust core 静态库 → Swift 壳 → 组装 bundle → ad-hoc 签名
# 用法：scripts/build.sh [--no-sign]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SIGN=1
[[ "${1:-}" == "--no-sign" ]] && SIGN=0

echo "==> 1/5 Rust core（release，静态库）"
cd core
# 注意：不要用 `cargo build | tail`（pipefail 下 tail 提前退出会让 cargo 吃 SIGPIPE 假失败）
if ! cargo build --release > /tmp/eryu-cargo.log 2>&1; then
  tail -40 /tmp/eryu-cargo.log
  echo "cargo build 失败（完整日志 /tmp/eryu-cargo.log）"
  exit 1
fi
tail -1 /tmp/eryu-cargo.log
# 清掉历史 cdylib，防止链接器优先选 .dylib（会引入本机绝对路径依赖）
rm -f target/release/liberyu_core.dylib
cd "$ROOT"

echo "==> 2/5 同步 C 头（单一来源：core/include/eryu_core.h）"
cp core/include/eryu_core.h app/Sources/EryuC/include/eryu_core.h

echo "==> 3/5 Swift 壳（release）"
cd app
if ! swift build -c release > /tmp/eryu-swift.log 2>&1; then
  grep -E "error:" /tmp/eryu-swift.log | head -30
  echo "swift build 失败（完整日志 /tmp/eryu-swift.log）"
  exit 1
fi
tail -1 /tmp/eryu-swift.log
cd "$ROOT"

echo "==> 4/5 组装 build/Eryu.app"
APP="build/Eryu.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp app/.build/release/Eryu "$APP/Contents/MacOS/Eryu"
cp packaging/Info.plist "$APP/Contents/Info.plist"
cp packaging/Eryu.icns "$APP/Contents/Resources/Eryu.icns"
# 保留 arm64 切片、去掉调试符号
strip -x "$APP/Contents/MacOS/Eryu" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $(date +%Y%m%d%H%M)" "$APP/Contents/Info.plist" >/dev/null 2>&1 || true

if [[ $SIGN -eq 1 ]]; then
  # 优先用稳定的自签名身份（TCC 跨重建保持，见 I-013 / D-003b）；
  # 没建过就退回 ad-hoc，并提示怎么建。
  WANT_ID="${ERYU_SIGN_IDENTITY:-Eryu Dev}"
  if security find-identity -v -p codesigning 2>/dev/null | grep -qF "$WANT_ID"; then
    echo "==> 5/5 签名（身份：${WANT_ID}，TCC 可跨重建保持）"
    codesign --force --sign "$WANT_ID" --entitlements packaging/eryu.entitlements \
             --identifier com.eryu.app "$APP" >/dev/null
  else
    echo "==> 5/5 ad-hoc 签名（TCC 每次重建都会重新授权，见 docs/ISSUES.md I-013）"
    codesign --force --sign - --entitlements packaging/eryu.entitlements \
             --identifier com.eryu.app "$APP" >/dev/null
    echo "    提示：想避免反复授权，建一张代码签名证书（钥匙串访问 → 证书助理 → 创建证书 → 代码签名，名字「${WANT_ID}」），"
    echo "    然后重跑本脚本即可自动使用（也可 ERYU_SIGN_IDENTITY 指定别的名字）。"
  fi
  codesign -dv "$APP" 2>&1 | grep -E "Identifier=|Signature=|Authority=" | sed 's/^/    /'
else
  echo "==> 5/5 跳过签名"
fi

echo
echo "产物：$ROOT/$APP"
echo "运行：open $ROOT/$APP    （首次会请求辅助功能权限）"
