#!/usr/bin/env bash
# 安装到 /Applications，并让系统里只留这一个 Eryu。
#
# 为什么需要它：Launchpad / Spotlight 是按 bundle 注册的。项目里 build/Eryu.app、
# dist/Eryu.app、以及解包验证用的 /tmp 副本只要被打开过，就会各自登记一次，
# 于是出现「两个 Eryu」，点哪个取决于注册顺序——用户无法分辨。
set -uo pipefail
cd "$(dirname "$0")/.."

# 默认优先用打包产物；没有就用 build/ 里的（同一份二进制，签名/版本一致）
if [ $# -ge 1 ]; then
  SRC="$1"
elif [ -d dist/Eryu.app ]; then
  SRC="dist/Eryu.app"
else
  SRC="build/Eryu.app"
fi
DST="/Applications/Eryu.app"
LSR="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"

if [ ! -d "$SRC" ]; then
  echo "找不到 $SRC"
  echo "先构建：./scripts/build.sh（或打包：./scripts/dist.sh）"
  exit 1
fi
VER="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$SRC/Contents/Info.plist" 2>/dev/null || echo 未知)"

# 正在运行的旧实例会锁住 bundle（先退出，避免装出「半个新版本」）
pkill -f "Eryu.app/Contents/MacOS/Eryu" 2>/dev/null && { echo "已退出正在运行的 Eryu"; sleep 0.6; }

echo "==> 安装 ${SRC}（v${VER}）到 $DST"
rm -rf "$DST"
ditto "$SRC" "$DST" || { echo "复制失败"; exit 1; }
codesign --verify --strict --deep "$DST" 2>/dev/null || { echo "⚠️  签名校验没通过，先别用"; }

# 把项目里的其它副本从 LaunchServices 注销，并删掉——系统里只留 /Applications 这一份。
# 开发/冒烟要用时再 `./scripts/build.sh`，它会重新生成 build/Eryu.app。
echo "==> 清掉项目内副本，避免再出现「两个 Eryu」"
for p in "$PWD/build/Eryu.app" "$PWD/dist/Eryu.app"; do
  if [ -d "$p" ]; then
    "$LSR" -u "$p" 2>/dev/null
    rm -rf "$p"
    echo "  已删除 ${p#"$PWD/"}"
  fi
done
# dist/Eryu.app 只是打包中间产物，装完就删（zip 里还有）
if [ "$SRC" != "dist/Eryu.app" ] && [ -d dist/Eryu.app ]; then
  rm -rf dist/Eryu.app && echo "  已删除 dist/Eryu.app（zip 保留）"
fi

echo "==> 现在系统里注册的 Eryu："
"$LSR" -dump 2>/dev/null | grep -o '/[^"]*Eryu\.app' | sort -u | sed 's/^/  /'

echo
echo "装好了：open /Applications/Eryu.app"
echo "权限只需点一次（麦克风 / 辅助功能 / 输入监控），之后重装不会再问——签名身份固定是 Eryu Dev。"
