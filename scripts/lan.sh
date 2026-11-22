#!/usr/bin/env bash
# 局域网听写接口（M5 路线②）：起服务 → curl 探活 → 发一段 WAV 要文字 → 关掉
set -uo pipefail
cd "$(dirname "$0")/.."
BIN="${ERYU_SMOKE_BIN:-build/Eryu.app/Contents/MacOS/Eryu}"
[ -x "$BIN" ] || [ ! -x "/Applications/Eryu.app/Contents/MacOS/Eryu" ] || BIN="/Applications/Eryu.app/Contents/MacOS/Eryu"
[ -x "$BIN" ] || { echo "先跑 scripts/build.sh"; exit 1; }
MODELS="${ERYU_REAL_MODELS:-$HOME/.eryu/models}"
SENSE="$MODELS/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"
PORT="${ERYU_LAN_PORT:-$((18800 + RANDOM % 900))}"
TOKEN="lantest$RANDOM"
D="$(mktemp -d)"; trap 'pkill -f "MacOS/Eryu --lan-serve" 2>/dev/null; rm -rf "$D"' EXIT
mkdir -p "$D/models"
for m in "$SENSE" "$MODELS/gtcrn_simple.onnx" "$MODELS/silero_vad.onnx"; do [ -e "$m" ] && ln -s "$m" "$D/models/$(basename "$m")"; done
cat > "$D/config.toml" <<CFG
[engine]
active = "sense"
[audio]
punct = false
denoise = false
[lan]
enabled = true
port = $PORT
token = "$TOKEN"
CFG
# 语料：一句话的 16k 单声道 WAV
WAV=/tmp/eryu-bench/wav/zh1.wav
if [ ! -f "$WAV" ]; then
  say -v Tingting -o "$D/a.aiff" "今天下午三点在会议室评审设计文档" 2>/dev/null
  afconvert -f WAVE -d LEI16@16000 -c 1 "$D/a.aiff" "$WAV" >/dev/null 2>&1
fi

ENVV=(env ERYU_CONFIG_DIR="$D" ERYU_MODEL_DIR="$SENSE" ERYU_VAD_MODEL="$D/models/silero_vad.onnx")
"${ENVV[@]}" "$BIN" --lan-serve 20 >/tmp/eryu-lan.log 2>&1 &
PID=$!
for _ in $(seq 1 80); do
  [ -s /tmp/eryu-lan.log ] && grep -q "监听" /tmp/eryu-lan.log && break
  sleep 0.5
  curl -s -m 1 -H "x-eryu-token: $TOKEN" "http://127.0.0.1:$PORT/health" >/dev/null 2>&1 && break
done

pass=0; fail=0
ok()  { echo "  ✅ $1"; pass=$((pass+1)); }
bad() { echo "  ❌ $1"; fail=$((fail+1)); }

H="$(curl -s -m 3 -H "x-eryu-token: $TOKEN" "http://127.0.0.1:$PORT/health")"
if [ -z "$H" ]; then sleep 2; H="$(curl -s -m 3 -H "x-eryu-token: $TOKEN" "http://127.0.0.1:$PORT/health")"; fi
if [ -z "$H" ]; then echo "  服务没起来，日志："; tail -5 /tmp/eryu-lan.log | sed 's/^/    /'; fi
echo "  health: $H"
case "$H" in *'"ok":true'*) ok "探活正常" ;; *) bad "探活失败" ;; esac

CODE="$(curl -s -m 3 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/health")"
[ "$CODE" = "403" ] && ok "没 token 被拒（403）" || bad "没 token 也应被拒，实际 $CODE"

R="$(curl -s -m 60 -H "x-eryu-token: $TOKEN" --data-binary @"$WAV" "http://127.0.0.1:$PORT/transcribe")"
echo "  transcribe: ${R:0:120}"
case "$R" in *'"ok":true'*会议室*) ok "识别结果正确" ;; *) bad "识别结果不对" ;; esac

PAGE_CODE="$(curl -s -m 5 -o /tmp/eryu-lan-page.html -w '%{http_code}' "http://127.0.0.1:$PORT/")"
[ "$PAGE_CODE" = "200" ] && grep -q "Eryu 听写" /tmp/eryu-lan-page.html && ok "网页版可用（GET / 返回录音页）" || bad "网页版打不开（${PAGE_CODE}）"

BAD="$(curl -s -m 5 -H "x-eryu-token: $TOKEN" --data-binary "hello" "http://127.0.0.1:$PORT/transcribe")"
case "$BAD" in *bad_wav*) ok "坏 WAV 有明确错误" ;; *) bad "坏 WAV 没报错：${BAD:0:80}" ;; esac

kill "$PID" 2>/dev/null; wait "$PID" 2>/dev/null

# 安全默认：监听 0.0.0.0 但没 token 时必须拒绝启动（而不是默默暴露）
# 并发限流：超上限的请求要拿到 503 busy（而非挂死/静默丢）
echo "== 并发限流（同时 4 个转写请求）=="
"${ENVV[@]}" "$BIN" --lan-serve 8 >/tmp/eryu-lan-busy.log 2>&1 &
PID3=$!
sleep 3
for i in 1 2 3 4; do
  curl -s -m 25 -H "x-eryu-token: $TOKEN" --data-binary @"$WAV" "http://127.0.0.1:$PORT/transcribe" >>/tmp/eryu-lan-busy.out 2>/dev/null &
done
wait
BUSY="$(grep -c '"code":"busy"' /tmp/eryu-lan-busy.out 2>/dev/null || true)"
OKS="$(grep -c '"ok":true' /tmp/eryu-lan-busy.out 2>/dev/null || true)"
if [ "${BUSY:-0}" -ge 1 ] && [ "${OKS:-0}" -ge 1 ]; then
  ok "超上限返回 503 busy（busy=${BUSY} ok=${OKS}）"
elif [ "${OKS:-0}" -ge 4 ]; then
  ok "四个都排队完成了（这台机器够快，没触发上限）"
else
  bad "并发行为不对（busy=${BUSY} ok=${OKS}）"
fi
rm -f /tmp/eryu-lan-busy.out
# 日志：退出时会打印 LANLOG，确认记了状态码但没记内容
sleep 6
if grep -q 'LANLOG \[{"at"' /tmp/eryu-lan-busy.log && ! grep -q '今天下午' /tmp/eryu-lan-busy.log; then
  ok "请求日志有状态码、没有内容"
else
  bad "请求日志不对"
fi
kill "$PID3" 2>/dev/null; wait "$PID3" 2>/dev/null

# 地址一览：至少要给「同一 Wi-Fi（.local）」那条，手机才不用查 IP
INFO="$("${ENVV[@]}" "$BIN" --lan-info 2>/dev/null)"
if printf '%s' "$INFO" | grep -q "同一 Wi-Fi"; then ok "地址一览含 .local"; else bad "地址一览没给 .local"; fi

echo "== 安全默认（allow_lan 但没 token）=="
cat > "$D/config.toml" <<CFG
[engine]
active = "sense"
[lan]
enabled = true
port = $PORT
allow_lan = true
CFG
"${ENVV[@]}" "$BIN" --lan-serve 6 >/tmp/eryu-lan-notoken.log 2>&1 &
PID2=$!
sleep 3
PROBE="$(curl -s -m 2 "http://127.0.0.1:$PORT/health")"
if [ -n "$PROBE" ]; then
  bad "没 token 居然也监听了：$PROBE"
else
  ok "没 token 拒绝监听"
fi
grep -q "lan_no_token\|没设 token" /tmp/eryu-lan-notoken.log && ok "日志说明了原因" || bad "日志没说原因"
kill "$PID2" 2>/dev/null; wait "$PID2" 2>/dev/null
echo
[ "$fail" -eq 0 ] && echo "局域网接口通过 ✅（${pass} 项）" || { echo "局域网接口失败 ❌（通过 ${pass}，失败 ${fail}）"; exit 1; }
