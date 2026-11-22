#!/usr/bin/env python3
"""量「接缝丢字」：同一句重复 N 遍的长音频，看关键词出现了几次。

渐进封口（D-059）每 10s 在安静缝处切一刀，接缝处偶尔会掉一个字（I-036）。
这个脚本把这件事变成数字：期望 N 次，实际出现几次。

    python3 scripts/seam.py -n 8 -r 3
"""
import argparse
import os
import pathlib
import re
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
BIN = ROOT / "build" / "Eryu.app" / "Contents" / "MacOS" / "Eryu"
SENTENCE = "第{i}条要确认的是周三下午三点的会议室和投影设备"


def synth(lines, out_wav):
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
        f.write("\n".join(lines))
        txt = f.name
    aiff = out_wav.with_suffix(".aiff")
    subprocess.run(["say", "-v", "Tingting", "-f", txt, "-o", str(aiff)], check=True)
    subprocess.run(["afconvert", "-f", "WAVE", "-d", "LEI16@16000", "-c", "1",
                    str(aiff), str(out_wav)], check=True)
    os.unlink(txt)


def run(wav, speed):
    env = dict(os.environ, ERYU_SIM_SPEED=str(speed))
    p = subprocess.run([str(BIN), "--simulate", str(wav)], capture_output=True, text=True,
                       env=env, timeout=180)
    m = re.search(r"SIMRESULT (\d+)ms (.*)", p.stdout)
    return (m.group(2) if m else ""), (int(m.group(1)) if m else -1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("-n", "--repeat", type=int, default=8)
    ap.add_argument("-r", "--rounds", type=int, default=3)
    ap.add_argument("-s", "--speed", type=float, default=8.0)
    a = ap.parse_args()

    with tempfile.TemporaryDirectory() as td:
        wav = pathlib.Path(td) / "seam.wav"
        synth([SENTENCE.format(i=i) for i in range(1, a.repeat + 1)], wav)
        print(f"素材：{a.repeat} 句 → {wav.name}（{wav.stat().st_size // 1024}KB），倍速 {a.speed}，跑 {a.rounds} 轮")
        got = []
        for r in range(1, a.rounds + 1):
            text, ms = run(wav, a.speed)
            n = text.count("会议室")
            got.append(n)
            print(f"  第 {r} 轮：关键词 {n}/{a.repeat}，松手→出字 {ms}ms")
        best, worst = max(got), min(got)
        print(f"\n接缝完整率：平均 {sum(got)/len(got)/a.repeat:.0%}（最好 {best}/{a.repeat}，最差 {worst}/{a.repeat}）")
        print("参考：D-059 记录约 0.4 字/接缝；明显低于 80% 说明切块参数或安静缝阈值退化了")


if __name__ == "__main__":
    main()
