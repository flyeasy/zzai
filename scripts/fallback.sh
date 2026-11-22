#!/usr/bin/env bash
# 上屏失败兜底：强制 undelivered → 结果必须还在（剪贴板 + 菜单可取回），不许静默丢字。
# 对应 R-063 / D-085。
set -uo pipefail
cd "$(dirname "$0")/.."
BIN="${ERYU_SMOKE_BIN:-build/Eryu.app/Contents/MacOS/Eryu}"
[ -x "$BIN" ] || [ ! -x "/Applications/Eryu.app/Contents/MacOS/Eryu" ] || BIN="/Applications/Eryu.app/Contents/MacOS/Eryu"
[ -x "$BIN" ] || { echo "先跑 scripts/build.sh"; exit 1; }
MODELS="${ERYU_REAL_MODELS:-$HOME/.eryu/models}"
SENSE="$MODELS/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"
VAD="$MODELS/silero_vad.onnx"
[ -f "$SENSE/model.int8.onnx" ] || { echo "没找到 SenseVoice 模型：${SENSE}"; exit 1; }

D="$(mktemp -d)"; trap 'rm -rf "$D"' EXIT
say -v Tingting -o "$D/a.aiff" "上屏失败兜底测试这一句必须还能取回。" 2>/dev/null
afconvert -f WAVE -d LEI16@16000 -c 1 "$D/a.aiff" "$D/a.wav" >/dev/null 2>&1

# 剪贴板是全局资源：先备份，测完还原，别影响用户
OLD_CLIP="$(pbpaste 2>/dev/null || true)"
OUT="$(ERYU_CONFIG_DIR="$D" ERYU_MODEL_DIR="$SENSE" ERYU_VAD_MODEL="$VAD" \
      "$BIN" --fallback-test "$D/a.wav" 2>&1)"
CODE=$?
printf '%s' "$OLD_CLIP" | pbcopy 2>/dev/null || true

echo "$OUT" | grep -E "FALLBACK" | head -3
case "$OUT" in
  *"FALLBACK OK"*) echo "上屏失败兜底通过 ✅" ;;
  *) echo "上屏失败兜底失败 ❌（exit=$CODE）"; exit 1 ;;
esac
