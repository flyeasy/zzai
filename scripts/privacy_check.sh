#!/usr/bin/env bash
# 隐私模式：历史、暂存、失败音频、统计 四处都不能留下东西（R-018 / D-023）。
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
mkdir -p "$D/models"
cat > "$D/config.toml" <<TOML
[engine]
active = "sense"

[privacy]
enabled = true
TOML
say -v Tingting -o "$D/ok.aiff" "隐私模式测试这一句不该留在磁盘上。" 2>/dev/null
afconvert -f WAVE -d LEI16@16000 -c 1 "$D/ok.aiff" "$D/ok.wav" >/dev/null 2>&1
# 1s 纯静音：走失败分支（本该落盘一条失败音频）
python3 - "$D/sil.wav" <<'PY'
import struct, sys, wave
w = wave.open(sys.argv[1], "wb"); w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000)
w.writeframes(struct.pack("<16000h", *([0]*16000))); w.close()
PY

ENV=(env ERYU_CONFIG_DIR="$D" ERYU_MODEL_DIR="$SENSE" ERYU_VAD_MODEL="$VAD")
"${ENV[@]}" "$BIN" --decode-wav "$D/ok.wav" >"$D/ok.log" 2>&1
"${ENV[@]}" "$BIN" --decode-wav "$D/sil.wav" >"$D/sil.log" 2>&1

FAIL=0
check() { if eval "$2"; then echo "  ✅ ${1}"; else echo "  ❌ ${1}"; FAIL=1; fi; }
echo "隐私模式检查（目录 ${D}）："
check "识别成功（有结果）" "grep -q 'WAVRESULT' '$D/ok.log'"
check "静音走失败分支" "grep -qE 'WAVERROR (too_quiet|no_audio)' '$D/sil.log'"
check "没有 history.json 或 0 条" "[ ! -f '$D/history.json' ] || python3 -c \"import json,sys; sys.exit(0 if len(json.load(open('$D/history.json')))==0 else 1)\""
check "没有统计记录" "[ ! -f '$D/stats.json' ] || python3 -c \"import json,sys; d=json.load(open('$D/stats.json')); sys.exit(0 if d.get('sessions',0)==0 else 1)\""
check "没有暂存结果" "[ ! -f '$D/pending/last_result.txt' ]"
check "没有失败音频（*.wav 不落盘）" "! ls '$D'/pending/*.wav >/dev/null 2>&1"
check "没有 suggestions.json" "[ ! -f '$D/suggestions.json' ]"
# 菜单「最近」也不许出现正文（这一条只有壳知道，走 --privacy-test）
PT="$("${ENV[@]}" "$BIN" --privacy-test "$D/ok.wav" 2>&1 | grep -E '^PRIVACYTEST' | tail -1)"
case "$PT" in
  *"PRIVACYTEST OK"*) echo "  ✅ 菜单最近不显示正文（${PT#PRIVACYTEST }）" ;;
  *) echo "  ❌ 菜单最近泄露正文：${PT:-无输出}"; FAIL=1 ;;
esac

if [ "$FAIL" = 0 ]; then echo "隐私模式干净 ✅"; else echo "隐私模式有残留 ❌"; exit 1; fi
