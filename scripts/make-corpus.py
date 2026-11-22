#!/usr/bin/env python3
"""用 macOS 的 say + afconvert 生成 25 条金标语料（16k mono），再配合 scripts/bench.py 算 CER。

用法：python3 scripts/make-corpus.py [--out /tmp/eryu-bench/wav] [--only zh1,en2]
语料文本在 bench/refs.json（zh1-10 中文 / en1-10 英文 / mx1-5 中英混）。
"""
import argparse
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
VOICE = {"zh": "Tingting", "en": "Samantha", "mx": "Tingting"}


def synth(text: str, voice: str, out: pathlib.Path) -> None:
    aiff = out.with_suffix(".aiff")
    subprocess.run(["say", "-v", voice, "-o", str(aiff), text], check=True)
    subprocess.run(
        ["afconvert", "-f", "WAVE", "-d", "LEI16@16000", "-c", "1", str(aiff), str(out)],
        check=True,
    )
    aiff.unlink(missing_ok=True)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="/tmp/eryu-bench/wav")
    ap.add_argument("--only", default="")
    args = ap.parse_args()

    refs = json.loads((ROOT / "bench" / "refs.json").read_text(encoding="utf-8"))
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    only = {s.strip() for s in args.only.split(",") if s.strip()}

    made = 0
    for key, text in refs.items():
        if only and key not in only:
            continue
        lang = key[:2]
        synth(text, VOICE[lang], out / f"{key}.wav")
        made += 1
        print(f"  {key}.wav  {text[:24]}…")
    print(f"生成 {made} 条 → {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
