#!/usr/bin/env bash
# 发版闸门：产物必须在这些检查全绿之后才能打出来。
#   ./scripts/release_gate.sh            # 全跑（约 6–12 分钟）
#   ERYU_GATE_FAST=1 ./scripts/release_gate.sh   # 只跑快的（不跑长语料/设备）
set -uo pipefail
cd "$(dirname "$0")/.."
FAST="${ERYU_GATE_FAST:-0}"
pass=0; fail=0
ok()  { echo "  ✅ $1"; pass=$((pass+1)); }
bad() { echo "  ❌ $1"; fail=$((fail+1)); }
step() { echo; echo "== $1 =="; }

step "1. 单元/契约测试（cargo test）"
if (cd core && cargo test --quiet >/tmp/eryu-gate-cargo.log 2>&1); then
  ok "cargo test 全绿（$(grep -c '^test .* ok$' /tmp/eryu-gate-cargo.log || true) 项）"
else bad "cargo test 失败，见 /tmp/eryu-gate-cargo.log"; fi

step "2. 文档一致性（ABI/版本/错误码）"
if ./scripts/doccheck.sh >/tmp/eryu-gate-doc.log 2>&1; then ok "文档一致"; else bad "文档不一致，见 /tmp/eryu-gate-doc.log"; fi

step "3. 极端状态矩阵（缺模型/坏设备/坏配置/只读目录）"
OUT="$(./scripts/matrix.sh 2>&1)"; case "$OUT" in
  *"全部通过"*) ok "矩阵 7 项通过" ;;
  *) bad "矩阵有失败：$(echo "$OUT" | grep '❌' | head -2 | tr '\n' ' ')" ;;
esac

step "4. 合成验收（六项：权限/接住/浮层/流式/结果/延迟）"
D="$(mktemp -d)"; trap 'rm -rf "$D"' EXIT
MODELS="${ERYU_REAL_MODELS:-$HOME/.eryu/models}"
SENSE="$MODELS/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"
VAD="$MODELS/silero_vad.onnx"
say -v Tingting -o "$D/a.aiff" "验收测试这一句话用来确认整条链路是通的。" 2>/dev/null
afconvert -f WAVE -d LEI16@16000 -c 1 "$D/a.aiff" "$D/a.wav" >/dev/null 2>&1
ACC="$(ERYU_CONFIG_DIR="$D" ERYU_MODEL_DIR="$SENSE" ERYU_VAD_MODEL="$VAD" \
       ERYU_ACCEPT_THRESHOLD_MS="${ERYU_ACCEPT_THRESHOLD_MS:-2500}" \
       ./build/Eryu.app/Contents/MacOS/Eryu --acceptance-synthetic "$D/a.wav" 2>&1)"
case "$ACC" in
  *"ACCEPTANCE OK"*) ok "合成验收六项通过" ;;
  *) bad "合成验收失败：$(echo "$ACC" | grep -E 'ACCEPTANCE|❌' | head -2 | tr '\n' ' ')" ;;
esac

step "5. 失败兜底（上屏失败/隐私/权限降级）"
for s in fallback privacy_check degrade switch_matrix lan; do
  if OUT="$("./scripts/$s.sh" 2>&1)"; then ok "$s 通过"; else bad "$s 失败：$(echo "$OUT" | tail -1)"; fi
done

if [ "$FAST" != "1" ]; then
  step "6. 长语料（两分钟上限 / 连续说话 / 拔麦 / 换设备）"
  for s in longcap.sh nopause.py mic_unplug.sh device_switch.sh; do
    cmd="./scripts/$s"
    case "$s" in *.sh) cmd="./scripts/$s" ;; *) cmd="python3 ./scripts/$s" ;; esac
    if OUT="$($cmd 2>&1)"; then ok "$s 通过"; else bad "$s 失败：$(echo "$OUT" | tail -1)"; fi
  done
fi

echo
if [ "$fail" -eq 0 ]; then
  echo "发版闸门通过 ✅（${pass} 项）"
else
  echo "发版闸门失败 ❌（通过 ${pass}，失败 ${fail}）"
  exit 1
fi
