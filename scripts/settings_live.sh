#!/usr/bin/env bash
# 改完设置是否立刻生效：热键（换键后旧的失效、新的生效）+ 引擎（切完立刻可用）。
# 对应 R-064 / D-086。
set -uo pipefail
cd "$(dirname "$0")/.."
BIN="${ERYU_SMOKE_BIN:-build/Eryu.app/Contents/MacOS/Eryu}"
[ -x "$BIN" ] || [ ! -x "/Applications/Eryu.app/Contents/MacOS/Eryu" ] || BIN="/Applications/Eryu.app/Contents/MacOS/Eryu"
[ -x "$BIN" ] || { echo "先跑 scripts/build.sh"; exit 1; }

OUT="$("$BIN" --settings-live-test 2>&1)"
echo "$OUT" | grep -E "SETLIVE" | tail -1
case "$OUT" in
  *"SETLIVE OK"*) echo "设置即时生效通过 ✅" ;;
  *) echo "设置即时生效失败 ❌"; exit 1 ;;
esac
