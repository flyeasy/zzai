#!/usr/bin/env bash
# 极端状态矩阵：缺模型 / 设备不存在 / 配置损坏 / 配置目录不可写
# 目的：失败路径不崩、错误码与 docs/ERRORS.md 对得上、绝不上屏也不写用户数据。
set -uo pipefail
cd "$(dirname "$0")/.."
BIN="${ERYU_SMOKE_BIN:-build/Eryu.app/Contents/MacOS/Eryu}"
[ -x "$BIN" ] || [ ! -x "/Applications/Eryu.app/Contents/MacOS/Eryu" ] || BIN="/Applications/Eryu.app/Contents/MacOS/Eryu"
[ -x "$BIN" ] || { echo "先跑 ./scripts/build.sh"; exit 1; }
pass=0; fail=0
ok()  { echo "  ✅ $1"; pass=$((pass+1)); }
bad() { echo "  ❌ $1"; fail=$((fail+1)); }

echo "== 1. 空配置目录（没有任何模型）=="
D1="$(mktemp -d)"
DIAG="$(ERYU_CONFIG_DIR="$D1" "$BIN" --diagnose 2>/dev/null)"
case "$DIAG" in
  *"SenseVoice 就绪=false"*) ok "诊断如实报告模型缺失" ;;
  *) bad "诊断没报告模型缺失：$(echo "$DIAG" | grep '^引擎：' | head -1)" ;;
esac
SELF="$(ERYU_CONFIG_DIR="$D1" "$BIN" --self-test 2>/dev/null)"
case "$SELF" in
  *SELFTEST\ OK*) ok "缺模型仍能跑通 stub 自测（链路不崩）" ;;
  *) bad "缺模型时自测失败" ;;
esac
CON="$(ERYU_CONFIG_DIR="$D1" "$BIN" --contract 2>/dev/null | grep '^CONTRACT' | tail -1)"
case "$CON" in
  *"CONTRACT OK"*) ok "缺模型仍满足壳契约" ;;
  *) bad "缺模型时契约不满足：${CON:-无输出}" ;;
esac
[ -f "$D1/history.json" ] && bad "自测写了 history.json（不该写）" || ok "自测没写历史"

echo "== 2. 设备名不存在（必须指名 + 列出可用设备）=="
D2="$(mktemp -d)"
printf '[audio]\ndevice = "根本不存在的麦克风"\n' > "$D2/config.toml"
# 这一项只验设备解析，不验识别质量：允许 stub（否则会被引擎守卫拦在设备检查之前）
OUT="$(ERYU_CONFIG_DIR="$D2" ERYU_ALLOW_STUB=1 "$BIN" --mic-test 1 2>&1)"
case "$OUT" in
  *mic_unavailable*"找不到输入设备"*"可用设备"*) ok "报 mic_unavailable 并列出可用设备" ;;
  *) bad "设备不存在时没给出可操作提示：$(echo "$OUT" | tail -1)" ;;
esac

echo "== 3. config.toml 损坏（必须带警告继续跑）=="
D3="$(mktemp -d)"
printf 'this is not toml [[[\n' > "$D3/config.toml"
DIAG3="$(ERYU_CONFIG_DIR="$D3" "$BIN" --diagnose 2>&1)"
case "$DIAG3" in
  *"解析失败"*|*"load_warning"*) ok "损坏配置有警告，不崩" ;;
  *) bad "损坏配置没有警告：$(echo "$DIAG3" | head -2 | tr '\n' ' ')" ;;
esac

echo "== 4. 配置目录不可写（保存失败要报出来，不能静默）=="
D4="$(mktemp -d)"; chmod 500 "$D4"
OUT4="$(ERYU_CONFIG_DIR="$D4" "$BIN" --diagnose 2>&1)"
chmod 700 "$D4"
case "$OUT4" in
  *"无法写入"*|*"Permission denied"*) ok "不可写目录在诊断里明确报出来" ;;
  *) bad "不可写目录没有报错：$(echo "$OUT4" | head -2 | tr '\n' ' ')" ;;
esac

rm -rf "$D1" "$D2" "$D3" "$D4"
echo
if [ "$fail" -eq 0 ]; then echo "极端状态矩阵全部通过 ✅（$pass 项）"; else echo "有 $fail 项失败"; exit 1; fi
