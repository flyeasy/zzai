#!/usr/bin/env bash
# 文档一致性检查：让 SSOT 与派生文档不互相打脸
# 1) ABI 版本：C 头注释 / lib.rs 常量 / smoke 期望值
# 2) 版本号：Info.plist / CHANGELOG 首条
# 3) 错误码：core 里出现的每个码都在 docs/ERRORS.md 里
# 4) 默认热键：config.rs 默认值 == 文档里写的一致
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0
note() { echo "  ✅ $1"; }
bad()  { echo "  ❌ $1"; fail=$((fail+1)); }

echo "== 1. ABI 版本 =="
MINOR_LIB="$(sed -nE 's/^pub const ABI_MINOR: u32 = ([0-9]+);.*/\1/p' core/src/lib.rs | head -1)"
MAJOR_LIB="$(sed -nE 's/^pub const ABI_MAJOR: u32 = ([0-9]+);.*/\1/p' core/src/lib.rs | head -1)"
if grep -qE "v${MAJOR_LIB}\.${MINOR_LIB}( |\*/|$)" core/include/eryu_core.h; then
  note "C 头有 v${MAJOR_LIB}.${MINOR_LIB} 段，与 lib.rs 一致"
else
  bad "C 头缺少 v${MAJOR_LIB}.${MINOR_LIB} 段（lib.rs 是 ${MAJOR_LIB}.${MINOR_LIB}）"
fi
SMOKE="$(grep -oE 'eryu_abi_version' scripts/smoke.sh | head -1)"
[ -n "$SMOKE" ] && note "冒烟检查了 ABI 符号" || bad "冒烟没检查 ABI 符号"

echo "== 2. 版本号 =="
PLIST="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' packaging/Info.plist 2>/dev/null)"
# 未发布的版本头写成 `## 未发布（0.8.1）`，同样算「当前版本」
CHANGELOG="$(grep -m1 -oE '^## (未发布（[0-9]+\.[0-9]+\.[0-9]+）|[0-9]+\.[0-9]+\.[0-9]+)' CHANGELOG.md | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')"
if [ "$PLIST" = "$CHANGELOG" ]; then note "Info.plist 与 CHANGELOG 都是 $PLIST"; else bad "Info.plist=$PLIST 但 CHANGELOG=$CHANGELOG"; fi
grep -q "CFBundleShortVersionString" packaging/Info.plist && note "Info.plist 有版本键" || bad "Info.plist 缺版本键"

echo "== 3. 错误码是否都在 docs/ERRORS.md =="
MISSING=""
for code in $(grep -rhoE 'emit_error\("[a-z_]+"' core/src/lib.rs | sed 's/.*("//; s/"//' | sort -u); do
  grep -q "\`$code\`" docs/ERRORS.md || MISSING="$MISSING $code"
done
[ -z "$MISSING" ] && note "core 的每个错误码都在文案表里" || bad "文案表缺:$MISSING"

echo "== 4. 默认值口径 =="
grep -q '"ALT_R"' core/src/config.rs && note "默认热键 ALT_R（右 Option）" || bad "默认热键不是 ALT_R"
grep -q "右 Option" docs/USAGE.md && note "使用手册提到右 Option" || bad "使用手册没写默认热键"
grep -q "min_record_ms" docs/DECISIONS.md && note "最短录音参数有记录" || bad "最短录音参数没有记录出处"

echo "== 5. 里程碑状态与 PROGRESS 对齐 =="
grep -q "M3 🟡" DESIGN.md && note "DESIGN 标记 M3 未完成（Windows 待机器）" || bad "DESIGN 的 M3 状态可疑"
grep -q "I-028" docs/WINDOWS.md docs/ISSUES.md && note "Windows 未验证有 ISSUES 条目" || bad "Windows 未验证没有 ISSUES 条目"

echo
if [ "$fail" -eq 0 ]; then echo "文档一致性检查全部通过 ✅"; else echo "有 $fail 项不一致"; exit 1; fi
