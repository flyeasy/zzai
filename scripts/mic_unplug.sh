#!/usr/bin/env bash
# 录音中途麦克风断开：已录到的内容必须保住并继续识别，不许整段丢掉。
set -uo pipefail
cd "$(dirname "$0")/.."
BIN="${ERYU_SMOKE_BIN:-build/Eryu.app/Contents/MacOS/Eryu}"
[ -x "$BIN" ] || [ ! -x "/Applications/Eryu.app/Contents/MacOS/Eryu" ] || BIN="/Applications/Eryu.app/Contents/MacOS/Eryu"
[ -x "$BIN" ] || { echo "先跑 scripts/build.sh"; exit 1; }
MODELS="${ERYU_REAL_MODELS:-$HOME/.eryu/models}"
SENSE="$MODELS/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"
VAD="$MODELS/silero_vad.onnx"
[ -f "$SENSE/model.int8.onnx" ] || { echo "没找到模型：$SENSE"; exit 1; }

D="$(mktemp -d)"; trap 'rm -rf "$D"' EXIT
say -v Tingting -o "$D/a.aiff" "这段话说一半麦克风被拔掉前面录到的内容应该保住。" 2>/dev/null
afconvert -f WAVE -d LEI16@16000 -c 1 "$D/a.aiff" "$D/a.wav" >/dev/null 2>&1

# 1.2s 时模拟拔麦；这条错误是预期的，收尾要继续
OUT="$(ERYU_SIM_TOLERATE_ERRORS=1 ERYU_FORCE_MIC_FAIL_MS=1200 ERYU_MIC_WAV="$D/a.wav" \
       ERYU_SIM_SPEED=2 ERYU_CONFIG_DIR="$D" ERYU_MODEL_DIR="$SENSE" ERYU_VAD_MODEL="$VAD" \
       "$BIN" --simulate "$D/a.wav" 2>&1)"
echo "$OUT" | grep -E "mic_unplugged|SIMRESULT|SIMULATE" | head -3
FAIL=0
case "$OUT" in *"mic_unplugged"*) ;; *) echo "  ❌ 没触发中断"; FAIL=1;; esac
case "$OUT" in *"SIMULATE OK"*) ;; *) echo "  ❌ 中断后没能收尾"; FAIL=1;; esac
TXT="$(printf '%s' "$OUT" | sed -nE 's/.*SIMRESULT [0-9]+ms //p' | tail -1)"
case "$TXT" in
  "") echo "  ❌ 已录内容没有识别出来"; FAIL=1;;
  *)  echo "  ✅ 保住了中断前的内容：${TXT:0:24}…";;
esac
[ $FAIL -eq 0 ] && echo "拔麦保住内容通过 ✅" || { echo "拔麦保住内容失败 ❌"; exit 1; }
