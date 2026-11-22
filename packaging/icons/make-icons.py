#!/usr/bin/env python3
"""「自在 zzai」图标生成：输出 mac iconset / Windows ico / 菜单栏模板图。
设计稿三选一：--design a|b|c；master 1024，全部 4x 超采样后缩。"""
import sys, os, math
from PIL import Image, ImageDraw, ImageFilter

SS = 4  # supersample

def lerp(c1, c2, t):
    return tuple(int(c1[i] + (c2[i] - c1[i]) * t) for i in range(3))

def gradient(size, c1, c2, diag=True):
    img = Image.new("RGB", (size, size))
    d = ImageDraw.Draw(img)
    for y in range(size):
        t = y / (size - 1)
        d.line([(0, y), (size, y)], fill=lerp(c1, c2, t))
    if diag:
        # 左上→右下再加一层，让对角线更有层次
        overlay = Image.new("L", (size, size), 0)
        od = ImageDraw.Draw(overlay)
        for i in range(size * 2):
            od.line([(i, 0), (0, i)], fill=int(30 * (1 - i / (size * 2))))
        img = Image.composite(Image.new("RGB", (size, size), (255, 255, 255)), img, overlay)
    return img

def rounded_mask(size, radius_ratio=0.225):
    m = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(m)
    r = int(size * radius_ratio)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=r, fill=255)
    return m

def draw_mic(d, size, color=(255, 255, 255)):
    cx = size * 0.5
    w = size * 0.155
    top = size * 0.255
    bot = size * 0.545
    d.rounded_rectangle([cx - w / 2, top, cx + w / 2, bot], radius=w / 2, fill=color)
    # 支架弧
    aw = size * 0.028
    ax0, ax1 = cx - size * 0.185, cx + size * 0.185
    ay = size * 0.585
    d.arc([ax0, ay - size * 0.20, ax1, ay + size * 0.16], start=0, end=180, fill=color, width=int(aw))
    # 立柱 + 底座
    d.rounded_rectangle([cx - aw / 2, ay + size * 0.10, cx + aw / 2, ay + size * 0.235], radius=aw / 2, fill=color)
    d.rounded_rectangle([cx - size * 0.115, ay + size * 0.245, cx + size * 0.115, ay + size * 0.245 + aw], radius=aw / 2, fill=color)

def design_a(size):
    base = gradient(size, (86, 104, 255), (140, 84, 255)).convert("RGBA")
    d = ImageDraw.Draw(base)
    # 左右各两道声波弧（麦克风两侧像在收音）
    for side, start, end in ((-1, 125, 235), (1, -55, 55)):
        for k, (rr, alpha) in enumerate([(0.085, 200), (0.135, 120)]):
            cx = size * 0.5 + side * size * (0.20 + rr * 0.9)
            cy = size * 0.44
            box = [cx - size * rr, cy - size * rr, cx + size * rr, cy + size * rr]
            d.arc(box, start=start, end=end, fill=(255, 255, 255, alpha), width=int(size * 0.030))
    draw_mic(d, size)
    return base

def design_b(size):
    base = gradient(size, (17, 24, 39), (45, 33, 74)).convert("RGBA")
    d = ImageDraw.Draw(base)
    # 白色气泡
    m = int(size * 0.115)
    x0, y0, x1, y1 = m, size * 0.245, size - m, size * 0.72
    d.rounded_rectangle([x0, y0, x1, y1], radius=int(size * 0.12), fill=(255, 255, 255, 255))
    # 气泡尾巴：贴着左下角的一小段圆角三角
    tw = size * 0.075
    tx = x0 + size * 0.20
    d.polygon([(tx - tw, y1 - size * 0.012), (tx + tw, y1 - size * 0.012), (tx - tw * 0.55, y1 + size * 0.085)], fill=(255, 255, 255, 255))
    d.ellipse([tx - tw * 1.05, y1 - size * 0.02, tx - tw * 0.05, y1 + size * 0.02], fill=(255, 255, 255, 255))
    # 气泡内三条波形（紫→蓝渐变）
    bars = [(0.36, 0.10, (124, 92, 255)), (0.5, 0.22, (91, 108, 255)), (0.64, 0.14, (124, 92, 255))]
    for cxr, hr, col in bars:
        cx = size * cxr
        bw = size * 0.075
        h = size * hr
        d.rounded_rectangle([cx - bw / 2, size * 0.4825 - h / 2, cx + bw / 2, size * 0.4825 + h / 2], radius=bw / 2, fill=col + (255,))
    return base

def design_c(size):
    base = gradient(size, (64, 88, 255), (168, 85, 247)).convert("RGBA")
    d = ImageDraw.Draw(base)
    # 圆头粗线画「z」：一条折线 + 圆头，避免拐角缺口
    col = (255, 255, 255, 255)
    lw = int(size * 0.115)
    zx0, zx1 = size * 0.285, size * 0.715
    zy0, zy1 = size * 0.30, size * 0.70
    d.line([(zx0, zy0), (zx1, zy0), (zx0, zy1), (zx1, zy1)], fill=col, width=lw, joint="curve")
    for (x, y) in [(zx0, zy0), (zx1, zy1)]:
        d.ellipse([x - lw / 2, y - lw / 2, x + lw / 2, y + lw / 2], fill=col)
    return base

DESIGNS = {"a": design_a, "b": design_b, "c": design_c}

def render_master(design, size=1024):
    big = DESIGNS[design](size * SS)
    mask = rounded_mask(size * SS)
    # 轻微内阴影/高光：顶部一层白色渐变
    hi = Image.new("L", (size * SS, size * SS), 0)
    hd = ImageDraw.Draw(hi)
    for y in range(int(size * SS * 0.5)):
        hd.line([(0, y), (size * SS, y)], fill=int(26 * (1 - y / (size * SS * 0.5))))
    big = Image.composite(Image.new("RGBA", big.size, (255, 255, 255, 255)), big, hi)
    out = Image.new("RGBA", big.size, (0, 0, 0, 0))
    out.paste(big, (0, 0), mask)
    return out.resize((size, size), Image.LANCZOS)

def save_png(img, path):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    img.save(path)

def bundle(design, root):
    """生成 mac .icns 与 Windows .ico（多尺寸）。"""
    master = render_master(design, 1024)
    icns_dir = os.path.join(root, "build", "zzai.iconset")
    os.makedirs(icns_dir, exist_ok=True)
    for size, name in [(16,"icon_16x16.png"),(32,"icon_16x16@2x.png"),(32,"icon_32x32.png"),(64,"icon_32x32@2x.png"),
                       (128,"icon_128x128.png"),(256,"icon_128x128@2x.png"),(256,"icon_256x256.png"),
                       (512,"icon_256x256@2x.png"),(512,"icon_512x512.png"),(1024,"icon_512x512@2x.png")]:
        master.resize((size,size), Image.LANCZOS).save(os.path.join(icns_dir, name))
    print("iconset", icns_dir)
    ico_path = os.path.join(root, "shell-win", "assets", "eryu.ico")
    master.save(ico_path, sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(96,96),(128,128),(256,256)])
    print("ico", ico_path)
    save_png(render_master(design, 1024), os.path.join(root, "packaging", "icons", f"zzai-{design}-1024.png"))

if __name__ == "__main__":
    design = "a"
    if "--design" in sys.argv:
        design = sys.argv[sys.argv.index("--design") + 1]
    outdir = os.path.join(os.path.dirname(__file__), "preview")
    for d in ("a", "b", "c"):
        m = render_master(d, 512)
        save_png(m, os.path.join(outdir, f"preview-{d}.png"))
        print("wrote", os.path.join(outdir, f"preview-{d}.png"))
    m = render_master(design, 1024)
    save_png(m, os.path.join(os.path.dirname(__file__), f"zzai-{design}-1024.png"))
    print("master", design)
    if "--bundle" in sys.argv:
        root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
        bundle(design, root)
