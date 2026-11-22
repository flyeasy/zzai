#!/usr/bin/env bash
# 换输入设备要即时生效：配置里改成第二个设备后，下一次录音必须真的用第二个设备。
set -uo pipefail
cd "$(dirname "$0")/.."
BIN="${ERYU_SMOKE_BIN:-build/Eryu.app/Contents/MacOS/Eryu}"
[ -x "$BIN" ] || [ ! -x "/Applications/Eryu.app/Contents/MacOS/Eryu" ] || BIN="/Applications/Eryu.app/Contents/MacOS/Eryu"
[ -x "$BIN" ] || { echo "先跑 scripts/build.sh"; exit 1; }
MODELS="${ERYU_REAL_MODELS:-$HOME/.eryu/models}"
SENSE="$MODELS/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"
VAD="$MODELS/silero_vad.onnx"

D="$(mktemp -d)"; trap 'rm -rf "$D"' EXIT
# 去掉「（默认：…）」后缀，否则会把说明文字当成设备名
DEVS="$(ERYU_CONFIG_DIR="$D" ERYU_MODEL_DIR="$SENSE" ERYU_VAD_MODEL="$VAD" "$BIN" --diagnose 2>/dev/null \
        | sed -nE 's/^输入设备：//p' | sed 's/（默认：.*）//')"
[ -n "$DEVS" ] || { echo "拿不到设备列表"; exit 1; }
A="$(python3 -c 'import sys;print(sys.argv[1].split(" / ")[0])' "$DEVS")"
B="$(python3 -c 'import sys;parts=sys.argv[1].split(" / ");print(parts[1] if len(parts)>1 else parts[0])' "$DEVS")"
echo "设备 A=${A}  B=${B}"
run() {
  printf '[audio]\ndevice = "%s"\n' "$1" > "$D/config.toml"
  ERYU_CONFIG_DIR="$D" ERYU_MODEL_DIR="$SENSE" ERYU_VAD_MODEL="$VAD" "$BIN" --mic-test 1 2>&1 | grep -E "采音开始|MICTEST" | head -2
}
O1="$(run "$A")"; O2="$(run "$B")"
echo "$O1"; echo "$O2"
FAIL=0
case "$O1" in *"采音开始 $A"*) ;; *) echo "  ❌ 第一个设备没生效"; FAIL=1;; esac
# mic-test 1 秒可能太短（环境安静时会报 too_quiet），只要「采音开始」证明设备生效即可
case "$O2" in *"采音开始 $B"*) ;; *) echo "  ❌ 换设备没即时生效"; FAIL=1;; esac
[ $FAIL -eq 0 ] && echo "换设备即时生效通过 ✅" || { echo "换设备即时生效失败 ❌"; exit 1; }
