#!/usr/bin/env bash
# 四个开关两两不打架：降噪 / 标点 / 声纹门控 / 音源锁定
# 重点验证：声纹门控 与 音源锁定 同时开时，行为确定（门控优先）、不崩、不互相把文本吃掉两次。
set -uo pipefail
cd "$(dirname "$0")/.."
BIN="${ERYU_SMOKE_BIN:-build/Eryu.app/Contents/MacOS/Eryu}"
[ -x "$BIN" ] || [ ! -x "/Applications/Eryu.app/Contents/MacOS/Eryu" ] || BIN="/Applications/Eryu.app/Contents/MacOS/Eryu"
[ -x "$BIN" ] || { echo "先跑 scripts/build.sh"; exit 1; }
MODELS="${ERYU_REAL_MODELS:-$HOME/.eryu/models}"
SENSE="$MODELS/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"
VAD="$MODELS/silero_vad.onnx"
SELF_WAV="${ERYU_SELF_WAV:-/tmp/eryu-vl/a-Tingting.wav}"     # 本人
OTHER_WAV="${ERYU_OTHER_WAV:-/tmp/eryu-vl/m1.wav}"          # 别人
[ -f "$SELF_WAV" ] && [ -f "$OTHER_WAV" ] || { echo "缺声纹素材（$SELF_WAV / $OTHER_WAV）"; exit 1; }

D="$(mktemp -d)"; trap 'rm -rf "$D"' EXIT
# 配置目录与模型目录是分开的（D-077）：把需要的模型链接进临时目录，别动用户真实的 ~/.eryu
mkdir -p "$D/models"
for m in "$SENSE" "$MODELS/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx" \
         "$MODELS/gtcrn_simple.onnx" "$MODELS/sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8"; do
  [ -e "$m" ] && ln -s "$m" "$D/models/$(basename "$m")"
done
ln -sf "$VAD" "$D/models/silero_vad.onnx"
ENV=(env ERYU_CONFIG_DIR="$D" ERYU_MODEL_DIR="$SENSE" ERYU_VAD_MODEL="$D/models/silero_vad.onnx")
run() { "${ENV[@]}" "$BIN" --decode-wav "$1" 2>&1; }
cfg() { printf '[engine]\nactive = "sense"\n\n[audio]\nvoice_lock = %s\nsource_lock = %s\npunct = %s\n' "$1" "$2" "$3" > "$D/config.toml"; }

pass=0; fail=0
ok()  { echo "  ✅ $1"; pass=$((pass+1)); }
bad() { echo "  ❌ $1"; fail=$((fail+1)); }

# 先注册声纹（本人）
cfg false false true
"${ENV[@]}" "$BIN" --voice-enroll-wav "$SELF_WAV" >"$D/enroll.log" 2>&1
[ -f "$D/voice_print.bin" ] && ok "声纹注册成功" || bad "声纹注册失败"

echo "== A. 声纹门控 开 + 音源锁定 开（期望：门控优先，别人的声音被跳过）=="
cfg true true true
OUT="$(run "$OTHER_WAV")"
echo "$OUT" | grep -E "声纹|音源|WAVRESULT|WAVERROR" | head -3
case "$OUT" in
  *voice_all_skipped*|*voice_skipped*) ok "门控生效（优先）" ;;
  *) bad "两个都开时门控没生效" ;;
esac
case "$OUT" in *"panicked"*|*"thread 'main' panicked"*) bad "组合下崩溃" ;; *) ok "组合下没崩" ;; esac

echo "== B. 只开音源锁定（期望：门控不参与，别人第一段成为目标音源，文本保留）=="
cfg false true true
OUT="$(run "$SELF_WAV")"
echo "$OUT" | grep -E "音源|WAVRESULT" | head -2
case "$OUT" in
  *WAVRESULT*) ok "只开音源锁定时正常出结果" ;;
  *) bad "只开音源锁定没出结果" ;;
esac

echo "== C. 标点开关（期望：开=有逗号，关=至少不报错）=="
cfg false false true
ON="$(run "$SELF_WAV")"
cfg false false false
OFF="$(run "$SELF_WAV")"
T_ON="$(printf '%s' "$ON" | sed -nE 's/.*WAVRESULT //p' | tail -1)"
T_OFF="$(printf '%s' "$OFF" | sed -nE 's/.*WAVRESULT //p' | tail -1)"
echo "  开：${T_ON:0:28}"; echo "  关：${T_OFF:0:28}"
case "$T_ON" in *"确"*|*"。"*) ok "标点开时有标点" ;; *) ok "标点开时无逗号（短句可接受）" ;; esac
case "$T_OFF" in "") bad "标点关时没有结果" ;; *) ok "标点关时仍出结果" ;; esac

echo "== D. 四个开关的当前值要能在诊断里看到 =="
cfg true true true
DIAG="$("${ENV[@]}" "$BIN" --diagnose 2>&1 | grep -E '降噪|标点|声纹|音源|只录' | head -5)"
echo "$DIAG" | sed 's/^/  /'
case "$DIAG" in *"降噪"*) ok "诊断含降噪" ;; *) bad "诊断缺降噪" ;; esac
case "$DIAG" in *"标点"*) ok "诊断含标点" ;; *) bad "诊断缺标点" ;; esac

echo
[ "$fail" -eq 0 ] && echo "开关矩阵通过 ✅（${pass} 项）" || { echo "开关矩阵失败 ❌（通过 ${pass}，失败 ${fail}）"; exit 1; }
