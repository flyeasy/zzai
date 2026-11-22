#!/usr/bin/env bash
# 权限不足时的降级：单键热键失败要说明原因、应用不崩、组合键仍可用。
set -uo pipefail
cd "$(dirname "$0")/.."
BIN="${ERYU_SMOKE_BIN:-build/Eryu.app/Contents/MacOS/Eryu}"
[ -x "$BIN" ] || [ ! -x "/Applications/Eryu.app/Contents/MacOS/Eryu" ] || BIN="/Applications/Eryu.app/Contents/MacOS/Eryu"
[ -x "$BIN" ] || { echo "先跑 scripts/build.sh"; exit 1; }
OUT="$("$BIN" --degrade-test 2>&1)"; CODE=$?
echo "$OUT" | grep -E "^DEGRADETEST" | tail -1
case "$OUT" in
  *"DEGRADETEST OK"*) echo "权限降级通过 ✅" ;;
  *) echo "权限降级失败 ❌（exit=$CODE）"; exit 1 ;;
esac
