#!/usr/bin/env python3
"""量音源锁定的命中率：A（我）与 B（插话）交替，A 应留下、B 应被跳过。

    python3 scripts/sourcelock_metric.py -p ABAB -r 2

会临时打开 ~/.eryu/config.toml 里的 source_lock，跑完恢复原值。
A 用 Tingting、B 用 Meijia 两条不同音色；A 的句子含「会议室」，B 的句子含「发票」，
所以结果里「会议室」次数 = 应该留下且留下了，「发票」出现 = 插话没被挡住（误伤）。
"""
import argparse
import os
import pathlib
import re
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
BIN = ROOT / "build" / "Eryu.app" / "Contents" / "MacOS" / "Eryu"
CONFIG = pathlib.Path.home() / ".eryu" / "config.toml"
A_SENT = "周三下午三点的会议室已经预订好了"
B_SENT = "我想问一下这次发票什么时候能报销"


def synth(voice, text, out):
    aiff = out.with_suffix(".aiff")
    subprocess.run(["say", "-v", voice, "-o", str(aiff), text], check=True)
    subprocess.run(["afconvert", "-f", "WAVE", "-d", "LEI16@16000", "-c", "1",
                    str(aiff), str(out)], check=True)


def concat(wavs, out, gap_secs=0.9):
    import wave
    frames = b""
    silence = b"\x00\x00" * int(16000 * gap_secs)
    for w in wavs:
        with wave.open(str(w)) as f:
            frames += f.readframes(f.getnframes())
        frames += silence
    with wave.open(str(out), "wb") as f:
        f.setnchannels(1)
        f.setsampwidth(2)
        f.setframerate(16000)
        f.writeframes(frames)


def set_lock(on):
    text = CONFIG.read_text()
    if "source_lock" in text:
        text = re.sub(r"source_lock = (true|false)", f"source_lock = {'true' if on else 'false'}", text)
    else:
        text = text.replace("[audio]", f"[audio]\nsource_lock = {'true' if on else 'false'}", 1)
    CONFIG.write_text(text)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("-p", "--pattern", default="ABAB")
    ap.add_argument("-r", "--rounds", type=int, default=2)
    ap.add_argument("-g", "--gap", type=float, default=0.9, help="句间静音（秒）；0 = 不给 VAD 边界（压力测试）")
    a = ap.parse_args()

    backup = CONFIG.read_text()
    try:
        with tempfile.TemporaryDirectory() as td:
            td = pathlib.Path(td)
            synth("Tingting", A_SENT, td / "a.wav")
            synth("Meijia", B_SENT, td / "b.wav")
            parts = [td / ("a.wav" if ch == "A" else "b.wav") for ch in a.pattern]
            mix = td / "mix.wav"
            concat(parts, mix, a.gap)
            want_a = a.pattern.count("A")
            want_b = a.pattern.count("B")
            print(f"素材：{a.pattern}（A 应留 {want_a}，B 应丢 {want_b}），跑 {a.rounds} 轮")
            set_lock(True)
            for r in range(1, a.rounds + 1):
                p = subprocess.run([str(BIN), "--decode-wav", str(mix)], capture_output=True,
                                   text=True, timeout=180)
                m = re.search(r"WAVRESULT (.*)", p.stdout)
                text = m.group(1) if m else ""
                kept = text.count("会议室")
                leaked = text.count("发票")
                print(f"  第 {r} 轮：留下 {kept}/{want_a}，误留插话 {leaked}/{want_b}")
                print(f"          文本：{text[:80]}")
    finally:
        CONFIG.write_text(backup)
        print("（source_lock 已恢复原值）")


if __name__ == "__main__":
    main()
