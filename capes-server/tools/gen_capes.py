#!/usr/bin/env python3
"""Generates the Lumos Client cape textures (original pixel art).

Every cape is a standard 64x32 Minecraft cape texture:
  - cape front (the side people see behind you) at (1,1), 10x16
  - cape back (against the player) at (12,1), 10x16
  - edges: top (1,0) 10x1, bottom (11,0) 10x1, left (0,1) 1x16, right (11,1) 1x16
  - elytra (22,0): wing outer face at (24,2) 10x20, inner (36,2) 10x20, edges around them

Run:  python3 capes-server/tools/gen_capes.py
Writes capes-server/public/capes/<id>.png and capes-server/src/catalog.js.
Deterministic: the same script always draws the same pixels.
"""

import json
import math
import random
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "public" / "capes"
CATALOG = ROOT / "src" / "catalog.js"


def lerp(a, b, t):
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(3))


def hexc(s):
    s = s.lstrip("#")
    return tuple(int(s[i : i + 2], 16) for i in (0, 2, 4))


def shade(c, f):
    return tuple(max(0, min(255, int(v * f))) for v in c)


def grad(stops, t):
    t = max(0.0, min(1.0, t))
    n = len(stops) - 1
    i = min(int(t * n), n - 1)
    return lerp(stops[i], stops[i + 1], t * n - i)


# Each design draws a w x h face: f(x, y, w, h, rnd) -> (r, g, b).
# rnd is a per-face seeded Random so designs are stable.


def lumos_flame(x, y, w, h, rnd):
    # A lantern flame rising from the hem on a deep night-blue cloth.
    bg = grad([hexc("#120c1e"), hexc("#1f1030")], y / h)
    cx = (w - 1) / 2
    base = h - 2
    t = (base - y) / (h * 0.8)  # 0 at hem, 1 near the top
    if 0 <= t <= 1:
        half = (1 - t) ** 0.8 * (w * 0.42) * (0.75 + 0.25 * math.sin(y * 1.3))
        d = abs(x - cx)
        if d <= half:
            core = 1 - d / max(half, 0.01)
            col = grad([hexc("#ff3d1f"), hexc("#ff7a2c"), hexc("#ffc24a"), hexc("#fff4c2")], core * (1 - t * 0.5))
            return col
        if d <= half + 1:
            return lerp(bg, hexc("#ff6b2c"), 0.35)
    if y >= h - 1:
        return hexc("#ff6b2c")
    if rnd.random() < 0.04:
        return hexc("#ffb36b")  # drifting sparks
    return bg


def aurora(x, y, w, h, rnd):
    sky = grad([hexc("#04131f"), hexc("#0a2233")], y / h)
    band = math.sin(x * 0.7 + y * 0.25) * 2.2 + h * 0.35
    band2 = math.sin(x * 0.5 - 1.3) * 1.8 + h * 0.6
    for b, cols in ((band, (hexc("#39ff9f"), hexc("#21c7b8"))), (band2, (hexc("#a86bff"), hexc("#3fd0ff")))):
        d = abs(y - b)
        if d < 2.2:
            return lerp(cols[0], cols[1], d / 2.2)
    if (x * 7 + y * 13) % 23 == 0:
        return hexc("#d9f6ff")
    return sky


def galaxy(x, y, w, h, rnd):
    cx, cy = w / 2 - 0.5, h / 2 - 0.5
    dx, dy = x - cx, (y - cy) * 0.7
    r = math.hypot(dx, dy)
    a = math.atan2(dy, dx)
    arm = math.sin(a * 2 + r * 0.9)
    bg = hexc("#07041a")
    if r < 1.3:
        return hexc("#fff3d6")
    if arm > 0.55 and r < 7:
        return lerp(hexc("#ff7ad9"), hexc("#5b3bff"), r / 7)
    if arm > 0.2 and r < 7.5:
        return lerp(bg, hexc("#3b2a8f"), 0.7)
    s = rnd.random()
    if s < 0.05:
        return hexc("#ffffff")
    if s < 0.09:
        return hexc("#9fb6ff")
    return bg


def obsidian(x, y, w, h, rnd):
    base = rnd.choice([hexc("#140c20"), hexc("#1b1030"), hexc("#100a1a"), hexc("#221338")])
    # purple veins
    if (x + y * 2) % 7 == 0 or (x * 3 - y) % 11 == 0:
        return hexc("#5d2a9e") if rnd.random() < 0.7 else hexc("#8e4fe0")
    if x in (0, w - 1) or y in (0, h - 1):
        return hexc("#2e1a4a")
    return base


def sakura(x, y, w, h, rnd):
    bg = grad([hexc("#ffe3ee"), hexc("#ffc2d6")], y / h)
    # a dark branch across the cape
    by = int(h * 0.3 + x * 0.45)
    if y == by or (x == 6 and by < y < by + 4):
        return hexc("#4a2b2b")
    # blossoms: 5-pixel plus shapes at fixed spots
    for bx, byy in ((2, 4), (7, 6), (4, 10), (8, 12), (1, 14), (6, 2)):
        if abs(x - bx) + abs(y - byy) <= 1:
            return hexc("#ffffff") if (x, y) == (bx, byy) else hexc("#ff7aa8")
    if rnd.random() < 0.05:
        return hexc("#ff9cc0")  # falling petals
    return bg


def ocean(x, y, w, h, rnd):
    col = grad([hexc("#7fe3ff"), hexc("#1a8fd6"), hexc("#0b3b78"), hexc("#06183a")], y / h)
    wave = 2 + math.sin(x * 0.9) * 1.2
    if abs(y - wave) < 0.7:
        return hexc("#e9fbff")
    for k in (6, 10):
        wy = k + math.sin(x * 0.8 + k) * 1.0
        if abs(y - wy) < 0.5:
            return lerp(col, hexc("#8fdcff"), 0.6)
    if y > h - 4 and rnd.random() < 0.08:
        return hexc("#bff3ff")  # bubbles
    return col


def emerald(x, y, w, h, rnd):
    # faceted gem tiles
    tx, ty = x % 5, y % 4
    if tx == 0 or ty == 0:
        return hexc("#0b3d20")
    facet = (tx + ty) % 3
    return [hexc("#17c964"), hexc("#3ff08a"), hexc("#0f9a4a")][facet] if rnd.random() > 0.05 else hexc("#c9ffe0")


def sunset(x, y, w, h, rnd):
    sky = grad([hexc("#2a1550"), hexc("#b0306a"), hexc("#ff7a3c"), hexc("#ffcf5a")], y / (h * 0.7))
    sun_cx, sun_cy = (w - 1) / 2, h * 0.66
    if y >= int(h * 0.72):
        # sea with the sun's reflection
        sea = grad([hexc("#3a1f5c"), hexc("#160b2a")], (y - h * 0.72) / (h * 0.28))
        if abs(x - sun_cx) < 2 - (y - h * 0.72) * 0.3 and y % 2 == 0:
            return hexc("#ffb44a")
        return sea
    if math.hypot(x - sun_cx, (y - sun_cy) * 1.1) < 3.2:
        return hexc("#ffe08a") if y % 3 else hexc("#ffb44a")
    return sky


def neon_grid(x, y, w, h, rnd):
    horizon = 6
    if y < horizon:
        c = grad([hexc("#0d0221"), hexc("#3b0a5c")], y / horizon)
        if (x * 5 + y * 3) % 17 == 0:
            return hexc("#ffffff")
        return c
    if y == horizon:
        return hexc("#ff2bd6")
    # perspective grid
    depth = y - horizon
    if depth in (2, 4, 7):
        return hexc("#00e5ff")
    cx = (w - 1) / 2
    for k in (-3, -1, 1, 3):
        lx = cx + k * (0.4 + depth * 0.35)
        if abs(x - lx) < 0.5:
            return hexc("#ff2bd6")
    return hexc("#12002a")


def checker(x, y, w, h, rnd):
    a, b = hexc("#f2f2f2"), hexc("#1c1c22")
    c = a if ((x // 2) + (y // 2)) % 2 == 0 else b
    if y in (0, h - 1):
        return hexc("#ff6b2c")
    return c


def moonlit(x, y, w, h, rnd):
    sky = grad([hexc("#0b1430"), hexc("#1d2b5c")], y / h)
    mx, my = w * 0.65, h * 0.25
    d = math.hypot(x - mx, y - my)
    if d < 2.6 and math.hypot(x - mx + 1.3, y - my - 0.6) >= 2.2:
        return hexc("#f6f1d0")
    # mountains near the hem
    ridge = h - 4 - abs((x % 6) - 3)
    if y >= ridge:
        return hexc("#0a0f1f") if y > ridge else hexc("#2c3a6b")
    if (x * 11 + y * 7) % 19 == 0:
        return hexc("#cfd8ff")
    return sky


def honeycomb(x, y, w, h, rnd):
    # amber hex cells
    row = y // 3
    off = 2 if row % 2 else 0
    cx = (x + off) % 4
    cy = y % 3
    if cy == 0 or cx == 0:
        return hexc("#5a3205")
    return [hexc("#ffb81c"), hexc("#ffd35a"), hexc("#e89a00")][(cx + cy + row) % 3]


DESIGNS = [
    ("lumos_flame", "Lumos Flame", lumos_flame, hexc("#ff6b2c")),
    ("aurora", "Aurora", aurora, hexc("#21c7b8")),
    ("galaxy", "Galaxy", galaxy, hexc("#5b3bff")),
    ("obsidian", "Obsidian", obsidian, hexc("#2e1a4a")),
    ("sakura", "Sakura", sakura, hexc("#ff7aa8")),
    ("ocean", "Ocean", ocean, hexc("#1a8fd6")),
    ("emerald", "Emerald", emerald, hexc("#0b3d20")),
    ("sunset", "Sunset", sunset, hexc("#ff7a3c")),
    ("neon_grid", "Neon Grid", neon_grid, hexc("#ff2bd6")),
    ("checker", "Checker", checker, hexc("#ff6b2c")),
    ("moonlit", "Moonlit", moonlit, hexc("#2c3a6b")),
    ("honeycomb", "Honeycomb", honeycomb, hexc("#5a3205")),
]


def face(fn, w, h, seed):
    rnd = random.Random(seed)
    return [[fn(x, y, w, h, rnd) for x in range(w)] for y in range(h)]


def paint(img, ox, oy, pixels, mirror=False, dim=1.0):
    for y, row in enumerate(pixels):
        for x, c in enumerate(row):
            sx = len(row) - 1 - x if mirror else x
            img.putpixel((ox + sx, oy + y), shade(c, dim) + (255,))


def fill(img, x0, y0, w, h, color):
    for y in range(y0, y0 + h):
        for x in range(x0, x0 + w):
            img.putpixel((x, y), color + (255,))


def make(cid, fn, edge):
    img = Image.new("RGBA", (64, 32), (0, 0, 0, 0))
    seed = sum(ord(c) for c in cid)
    front = face(fn, 10, 16, seed)
    paint(img, 1, 1, front)  # visible side
    paint(img, 12, 1, front, mirror=True, dim=0.55)  # inner side, darker
    dark = shade(edge, 0.7)
    fill(img, 1, 0, 10, 1, dark)  # top
    fill(img, 11, 0, 10, 1, dark)  # bottom
    fill(img, 0, 1, 1, 16, edge)  # left edge
    fill(img, 11, 1, 1, 16, edge)  # right edge
    # Elytra: the same design drawn on a 10x20 wing.
    wing = face(fn, 10, 20, seed + 1)
    paint(img, 24, 2, wing)
    paint(img, 36, 2, wing, mirror=True, dim=0.6)
    fill(img, 22, 2, 2, 20, edge)
    fill(img, 34, 2, 2, 20, edge)
    fill(img, 24, 0, 10, 2, dark)
    fill(img, 34, 0, 10, 2, dark)
    return img


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    catalog = []
    for cid, name, fn, edge in DESIGNS:
        make(cid, fn, edge).save(OUT / f"{cid}.png", optimize=True)
        catalog.append({"id": cid, "name": name})
    CATALOG.write_text(
        "// Generated by tools/gen_capes.py. Do not edit by hand.\nexport default " + json.dumps(catalog, indent=2) + ";\n"
    )
    print(f"wrote {len(catalog)} capes to {OUT}")


if __name__ == "__main__":
    main()
