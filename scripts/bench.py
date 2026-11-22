#!/usr/bin/env python3
"""金标语料基准：逐条走产品链路（`Eryu --decode-wav`），算 CER 与延迟。

用法：
  python3 scripts/bench.py                      # 默认 /tmp/eryu-bench/wav
  python3 scripts/bench.py --dir /tmp/x --engine qwen3 --json /tmp/bench.json

CER = 字符级编辑距离 / 参考文本长度（去掉标点与空白后比较，英文不分大小写）。
延迟取 `WAVRESULT` 里的「松手→出字」毫秒数（含预览让出、VAD、识别、热词、标点、润色）。
"""
import argparse
import json
import pathlib
import re
import statistics
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BIN = ROOT / "build" / "Eryu.app" / "Contents" / "MacOS" / "Eryu"
PUNCT = re.compile(r"[\s，。、！？；：“”‘’（）《》…—·,.!?;:\"'()\[\]{}<>~`@#$%^&*_+=|\\/-]")


def norm(s: str) -> str:
    return PUNCT.sub("", s).lower()


def edit_distance(a: str, b: str) -> int:
    if a == b:
        return 0
    if not a:
        return len(b)
    if not b:
        return len(a)
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]


def run_one(wav: pathlib.Path, engine: str) -> tuple[str, int]:
    env = dict(**__import__("os").environ)
    env["ERYU_ENGINE"] = engine
    p = subprocess.run(
        [str(BIN), "--decode-wav", str(wav)], capture_output=True, text=True, env=env, timeout=300
    )
    out = p.stdout
    m = re.search(r"WAVRESULT (.*)", out)
    if not m:
        raise RuntimeError(f"{wav.name}: 没有 WAVRESULT\n{out[-400:]}")
    # --decode-wav 不打 ms（那是 --simulate 的输出）：用引擎日志里的解码 + 标点耗时相加
    ms = 0
    for pat in (r"\[eryu\]\[timing\] 解码 (\d+)ms", r"\[eryu\]\[timing\] 热词\+标点 (\d+)ms"):
        mm = re.search(pat, p.stderr)
        if mm:
            ms += int(mm.group(1))
    return m.group(1).strip(), ms


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", default="/tmp/eryu-bench/wav")
    ap.add_argument("--engine", default="sense", choices=["sense", "qwen3"])
    ap.add_argument("--json", default="")
    ap.add_argument("--verbose", action="store_true")
    args = ap.parse_args()

    refs = json.loads((ROOT / "bench" / "refs.json").read_text(encoding="utf-8"))
    wav_dir = pathlib.Path(args.dir)
    rows = []
    for key, ref in refs.items():
        wav = wav_dir / f"{key}.wav"
        if not wav.exists():
            print(f"  跳过 {key}（缺 {wav}）")
            continue
        got, ms = run_one(wav, args.engine)
        n_ref, n_got = norm(ref), norm(got)
        cer = edit_distance(n_ref, n_got) / max(1, len(n_ref))
        rows.append({"id": key, "lang": key[:2], "cer": cer, "ms": ms, "ref": ref, "got": got})
        if args.verbose:
            print(f"  {key}: CER {cer:.3f} {ms}ms\n    参考 {ref}\n    得到 {got}")

    if not rows:
        print("没有可用样本")
        return 1

    print(f"\n引擎 {args.engine}（{len(rows)} 条）")
    print(f"{'类别':<6}{'条数':>4}{'CER':>9}{'p50ms':>8}{'p95ms':>8}")
    for lang in ("zh", "en", "mx", "all"):
        sub = rows if lang == "all" else [r for r in rows if r["lang"] == lang]
        if not sub:
            continue
        cer = sum(r["cer"] for r in sub) / len(sub)
        lats = sorted(r["ms"] for r in sub)
        p50 = statistics.median(lats)
        p95 = lats[min(len(lats) - 1, int(round(0.95 * (len(lats) - 1))))]
        print(f"{lang:<6}{len(sub):>4}{cer:>9.3f}{p50:>8.0f}{p95:>8.0f}")

    if args.json:
        pathlib.Path(args.json).write_text(json.dumps(rows, ensure_ascii=False, indent=1), encoding="utf-8")
        print(f"明细 → {args.json}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
