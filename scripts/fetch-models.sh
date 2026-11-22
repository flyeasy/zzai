#!/usr/bin/env bash
# 下载 Eryu 默认语音模型到 ~/.eryu/models（模型下载需联网；听写本身全在本机）
set -euo pipefail
cd "$(dirname "$0")/.."

DEST="${ERYU_MODELS_DIR:-$HOME/.eryu/models}"
BASE="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models"
SENSE="sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"
VAD="silero_vad.onnx"
VAD_URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx"

mkdir -p "$DEST"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if [ -d "$DEST/$SENSE" ] && [ -f "$DEST/$SENSE/model.int8.onnx" ]; then
  echo "已存在：$DEST/$SENSE"
else
  echo "下载 $SENSE …"
  curl -L --fail --progress-bar -o "$tmp/sense.tar.bz2" "$BASE/$SENSE.tar.bz2"
  tar -xjf "$tmp/sense.tar.bz2" -C "$DEST"
fi

if [ -f "$DEST/$VAD" ]; then
  echo "已存在：$DEST/$VAD"
else
  echo "下载 $VAD …"
  curl -L --fail --progress-bar -o "$DEST/$VAD" "$VAD_URL"
fi

echo
echo "模型就位："
du -sh "$DEST"/* 2>/dev/null || true
echo
echo "验证：cargo run --release --example recognize_wav -- <16k mono wav>"
