"""Renders the freeflake app icon: a white snowflake on a winter-night squircle.

python3 design/make_icon.py  ->  docs/icon.png, packaging/macos/AppIcon.icns
"""

import math
import random
import os
import shutil
import subprocess
import tempfile

from PIL import Image, ImageDraw, ImageFilter

SIZE = 1024
SS = 4  # supersampling


def squircle_mask(n: int, inset: float) -> Image.Image:
    mask = Image.new("L", (n, n), 0)
    d = ImageDraw.Draw(mask)
    r = n * 0.225
    d.rounded_rectangle([inset, inset, n - inset, n - inset], radius=r, fill=255)
    return mask


def gradient(n: int) -> Image.Image:
    top, bottom = (66, 126, 214), (17, 34, 82)
    img = Image.new("RGB", (n, n))
    px = img.load()
    for y in range(n):
        t = y / (n - 1)
        c = tuple(int(a + (b - a) * t) for a, b in zip(top, bottom))
        for x in range(n):
            px[x, y] = c
    return img


def snowflake(n: int) -> Image.Image:
    layer = Image.new("L", (n, n), 0)
    d = ImageDraw.Draw(layer)
    c = n / 2
    arm = n * 0.33
    width = int(n * 0.045)
    for k in range(6):
        a = k * math.pi / 3 - math.pi / 2
        dx, dy = math.cos(a), math.sin(a)
        tip = (c + dx * arm, c + dy * arm)
        d.line([(c, c), tip], fill=255, width=width)
        d.ellipse([tip[0] - width / 2, tip[1] - width / 2, tip[0] + width / 2, tip[1] + width / 2], fill=255)
        for at, length in ((0.48, 0.36), (0.76, 0.24)):
            bx, by = c + dx * arm * at, c + dy * arm * at
            for side in (-1, 1):
                b = a + side * math.pi / 3
                end = (bx + math.cos(b) * arm * length, by + math.sin(b) * arm * length)
                d.line([(bx, by), end], fill=255, width=int(width * 0.8))
                d.ellipse([end[0] - width * 0.4, end[1] - width * 0.4, end[0] + width * 0.4, end[1] + width * 0.4], fill=255)
    hub = n * 0.06
    d.regular_polygon((c, c, hub), 6, fill=255)
    return layer


def render() -> Image.Image:
    n = SIZE * SS
    inset = n * 0.09
    icon = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    bg = gradient(n).convert("RGBA")
    mask = squircle_mask(n, inset)
    icon.paste(bg, (0, 0), mask)
    # Small background flakes.
    dots = ImageDraw.Draw(icon)
    rng = random.Random(7)
    for _ in range(36):
        x = inset + rng.random() * (n - 2 * inset)
        y = inset + rng.random() * (n - 2 * inset) * 0.8
        r = n * (0.004 + 0.006 * rng.random())
        dots.ellipse([x - r, y - r, x + r, y + r], fill=(255, 255, 255, 90))
    flake = snowflake(n)
    glow = flake.filter(ImageFilter.GaussianBlur(n * 0.02))
    icon.paste(Image.new("RGBA", (n, n), (170, 210, 255, 150)), (0, 0), glow)
    icon.paste(Image.new("RGBA", (n, n), (255, 255, 255, 255)), (0, 0), flake)
    # Snowdrift at the bottom.
    drift = Image.new("L", (n, n), 0)
    dd = ImageDraw.Draw(drift)
    dd.ellipse([inset - n * 0.2, n * 0.80, n * 0.62, n * 1.2], fill=255)
    dd.ellipse([n * 0.42, n * 0.84, n - inset + n * 0.25, n * 1.25], fill=255)
    drift = Image.composite(drift, Image.new("L", (n, n), 0), mask)
    icon.paste(Image.new("RGBA", (n, n), (236, 243, 255, 255)), (0, 0), drift)
    icon.putalpha(Image.composite(icon.getchannel("A"), Image.new("L", (n, n), 0), mask))
    return icon.resize((SIZE, SIZE), Image.LANCZOS)


def main() -> None:
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    icon = render()
    icon.save(os.path.join(root, "docs", "icon.png"))
    with tempfile.TemporaryDirectory() as tmp:
        iconset = os.path.join(tmp, "AppIcon.iconset")
        os.mkdir(iconset)
        for size in (16, 32, 128, 256, 512):
            for scale in (1, 2):
                px = size * scale
                name = f"icon_{size}x{size}{'@2x' if scale == 2 else ''}.png"
                icon.resize((px, px), Image.LANCZOS).save(os.path.join(iconset, name))
        out = os.path.join(root, "packaging", "macos", "AppIcon.icns")
        if shutil.which("iconutil"):
            subprocess.run(["iconutil", "-c", "icns", iconset, "-o", out], check=True)


if __name__ == "__main__":
    main()
