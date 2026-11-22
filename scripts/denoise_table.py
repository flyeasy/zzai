#!/usr/bin/env python3
"""量「降噪到底帮不帮」：白噪声 vs 背景人声，开/关 GTCRN 各跑一遍 CER。

    python3 scripts/denoise_table.py --noise white --gain 0.5
    python3 scripts/denoise_table.py --noise speech --gain 0.5

依赖：/tmp/eryu-bench 的 25 条金标（scripts/make-corpus.py 生成）、~/.eryu/models 里的 GTCRN。
结论写进 D-069 的表；任何"降噪有用/有害"的说法都用这套数字。
"""
import argparse
import json
import pathlib
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import wave

ROOT = pathlib.Path(__file__).resolve().parent.parent
BIN = ROOT / "build" / "Eryu.app" / "Contents" / "MacOS" / "Eryu"
BENCH = pathlib.Path("/tmp/eryu-bench")
REFS = pathlib.Path("bench/refs.json")
GTCRN = pathlib.Path.home() / ".eryu" / "models" / "gtcrn_simple.onnx"
DENOISER = ROOT / "core" / "target" / "release" / "examples" / "denoise_file"
CONFIG = pathlib.Path.home() / ".eryu" / "config.toml"


def read_wav(p):
    w = wave.open(str(p))
    data = w.readframes(w.getnframes())
    return list(struct.unpack("<%dh" % (len(data) // 2), data))


def write_wav(p, s):
    w = wave.open(str(p), "wb")
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000)
    w.writeframes(struct.pack("<%dh" % len(s), *[max(-32768, min(32767, int(x))) for x in s]))
    w.close()


def rng(seed):
    state = seed
    while True:
        state ^= (state << 13) & 0xFFFFFFFF
        state ^= state >> 17
        state ^= (state << 5) & 0xFFFFFFFF
        yield (state % 2001) - 1000        # ±1000 ≈ 白噪声


def rms(xs):
    return (sum(x * x for x in xs) / max(1, len(xs))) ** 0.5


def mix(src, bg, snr_db, noise, out):
    """按**信噪比**混，不是按固定倍数——否则「0dB」只是个标签（D-079 的教训）。"""
    a = read_wav(src)
    speech_rms = max(rms(a), 1.0)
    if noise == "white":
        g = rng(12345)
        b = [next(g) for _ in range(len(a))]
    else:
        bb = read_wav(bg)
        b = []
        while len(b) < len(a):
            b.extend(bb)
        b = b[: len(a)]
    scale = speech_rms * (10 ** (snr_db / 20.0)) / max(rms(b), 1e-9)
    write_wav(out, [int(x + y * scale) for x, y in zip(a, b)])


def cer(ref, hyp):
    ref = re.sub(r"[\s，。！？、,.!?;:；：]", "", ref).lower()
    hyp = re.sub(r"[\s，。！？、,.!?;:；：]", "", hyp).lower()
    if not ref:
        return 0.0
    d = list(range(len(hyp) + 1))
    for i, rc in enumerate(ref, 1):
        prev, d[0] = d[0], i
        for j, hc in enumerate(hyp, 1):
            cur = min(d[j] + 1, d[j - 1] + 1, prev + (rc != hc))
            prev, d[j] = d[j], cur
    return d[len(hyp)] / len(ref)


def set_denoise(on):
    text = CONFIG.read_text()
    text = re.sub(r"denoise = (true|false)", f"denoise = {'true' if on else 'false'}", text)
    CONFIG.write_text(text)


def run(wav):
    p = subprocess.run([str(BIN), "--decode-wav", str(wav)], capture_output=True, text=True, timeout=120)
    m = re.search(r"WAVRESULT (.*)", p.stdout)
    return m.group(1) if m else ""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--noise", choices=["white", "speech"], default="white")
    ap.add_argument("--snr-db", type=float, default=0.0, help="信噪比：0 = 噪声与语音等能量；−6 = 噪声小一半能量")
    ap.add_argument("--keys", nargs="*", default=["zh1", "zh2", "zh3", "zh4", "zh5"])
    a = ap.parse_args()
    if not GTCRN.exists():
        sys.exit(f"没有 GTCRN 模型：{GTCRN}（设置 → 音频 里点下载，或 --pull-model gtcrn）")
    refs = json.loads(REFS.read_text())
    bg = BENCH / "wav" / "zh10.wav"
    backup = CONFIG.read_text()
    try:
        rows = []
        with tempfile.TemporaryDirectory() as td:
            td = pathlib.Path(td)
            for key in a.keys:
                src = BENCH / "wav" / f"{key}.wav"
                noisy = td / f"{key}.wav"
                mix(src, bg, a.snr_db, a.noise, noisy)
                r = refs.get(key, "")
                # 产品链路的降噪在采集侧，离线对比必须先把 wav 过一遍 GTCRN 再解码
                subprocess.run([str(DENOISER), str(GTCRN), str(noisy)], check=True,
                               capture_output=True)
                cleaned = td / f"{key}-denoised.wav"
                set_denoise(False)
                off = cer(r, run(noisy))
                on = cer(r, run(cleaned))
                rows.append((key, off, on))
                print(f"  {key}: 不降噪 {off:.3f} → 降噪 {on:.3f}  {'✅ 变好' if on < off else ('= 持平' if abs(on-off) < 1e-9 else '❌ 更差')}")
        off_avg = sum(r[1] for r in rows) / len(rows)
        on_avg = sum(r[2] for r in rows) / len(rows)
        print(f"\n噪声={a.noise} 信噪比={a.snr_db:+.0f}dB")
        print(f"平均 CER：不降噪 {off_avg:.3f} → 降噪 {on_avg:.3f}")
    finally:
        CONFIG.write_text(backup)


if __name__ == "__main__":
    main()
