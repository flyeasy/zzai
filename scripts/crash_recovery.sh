#!/usr/bin/env bash
# 崩溃恢复端到端：结果已落盘、还没确认上屏的瞬间硬杀进程（SIGKILL），重启后必须能原样取回。
# 对应 D-014（结果先落盘再上屏）与 R-059。
set -uo pipefail
cd "$(dirname "$0")/.."
BIN="${ERYU_SMOKE_BIN:-build/Eryu.app/Contents/MacOS/Eryu}"
[ -x "$BIN" ] || [ ! -x "/Applications/Eryu.app/Contents/MacOS/Eryu" ] || BIN="/Applications/Eryu.app/Contents/MacOS/Eryu"
[ -x "$BIN" ] || { echo "先跑 scripts/build.sh"; exit 1; }

MODELS="${ERYU_REAL_MODELS:-$HOME/.eryu/models}"
SENSE="$MODELS/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"
VAD="$MODELS/silero_vad.onnx"
[ -f "$SENSE/model.int8.onnx" ] || { echo "没找到 SenseVoice 模型：${SENSE}"; exit 1; }

D="$(mktemp -d)"
trap 'rm -rf "$D"' EXIT
say -v Tingting -o "$D/in.aiff" "崩溃恢复测试：这句话必须能被取回来。" 2>/dev/null
afconvert -f WAVE -d LEI16@16000 -c 1 "$D/in.aiff" "$D/in.wav" >/dev/null 2>&1

run() { ERYU_CONFIG_DIR="$D" ERYU_MODEL_DIR="$SENSE" ERYU_VAD_MODEL="$VAD" "$@"; }

echo "== 1. 解码 → 在提交确认前 SIGKILL =="
set +e
OUT="$(run env ERYU_KEEP_PENDING=1 ERYU_KILL_ON_SUBMIT=1 "$BIN" --decode-wav "$D/in.wav" 2>&1)"
CODE=$?
set -e
echo "  $(printf '%s' "$OUT" | grep -E 'WAVRESULT|WAVTEST' | head -3 | tr '\n' ' ')"
PEND="$D/pending/last_result.txt"
if [ "$CODE" -eq 137 ]; then echo "  ✅ 进程被 SIGKILL（exit 137）"; else echo "  ⚠️ 退出码 $CODE（期望 137）"; fi
if [ -s "$PEND" ]; then
  echo "  ✅ 暂存已落盘：$(wc -c <"${PEND}" | tr -d ' ') 字节"
else
  echo "  ❌ 没落盘（${PEND}）"; exit 1
fi
SAVED="$(cat "$PEND")"

echo "== 2. 重启后取回 =="
GOT="$(run "$BIN" --print-last 2>&1 | tail -1)"
echo "  $GOT"
if [ "$GOT" = "$SAVED" ]; then
  echo "  ✅ 取回内容与暂存一致：$SAVED"
else
  echo "  ❌ 取回不一致：保存=「${SAVED}」取回=「${GOT}」"; exit 1
fi

echo "== 3. 取回后文件保留（等用户确认走后由确认路径清理）=="
if [ -s "$PEND" ]; then echo "  ✅ 文件仍在"; else echo "  ⚠️ 文件被提前删除（检查 --print-last 是否误清）"; fi

echo "崩溃恢复端到端通过 ✅"
