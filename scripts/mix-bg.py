#!/usr/bin/env python3
"""把另一段语音当背景混进语料，复现「旁边有人说话」的场景。

    python3 scripts/mix-bg.py --src /tmp/eryu-bench/wav --bg /tmp/eryu-bench/wav/zh10.wav \
        --dst /tmp/eryu-bgmix --gain 0.5 --keys zh1 zh2 en1

--gain 是背景相对主语音的线性倍数（0.5 ≈ −6dB）。
"""
import argparse
import pathlib
import struct
import wave


def read(p):
    w = wave.open(str(p))
    frames = w.readframes(w.getnframes())
    w.close()
    return list(struct.unpack("<%dh" % (len(frames) // 2), frames))


def write(p, s):
    w = wave.open(str(p), "wb")
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(16000)
    w.writeframes(struct.pack("<%dh" % len(s), *[max(-32768, min(32767, int(x))) for x in s]))
    w.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", required=True)
    ap.add_argument("--bg", required=True, help="当背景的 wav（会循环铺满）")
    ap.add_argument("--dst", required=True)
    ap.add_argument("--gain", type=float, default=0.5)
    ap.add_argument("--keys", nargs="*", default=None, help="只处理这些 id（默认全部）")
    a = ap.parse_args()

    src, dst = pathlib.Path(a.src), pathlib.Path(a.dst)
    dst.mkdir(parents=True, exist_ok=True)
    bg = read(a.bg)
    keys = a.keys or [p.stem for p in sorted(src.glob("*.wav"))]
    for k in keys:
        main = read(src / f"{k}.wav")
        write(dst / f"{k}.wav", [x + bg[i % len(bg)] * a.gain for i, x in enumerate(main)])
    print(f"混好 {len(keys)} 条 → {dst}")


if __name__ == "__main__":
    main()
