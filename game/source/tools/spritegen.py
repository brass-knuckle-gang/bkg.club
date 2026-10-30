#!/usr/bin/env python3
"""Builds the pixel art for Brass Knuckles Knock Out and writes firmware/src/sprites.rs.

The fighters are drawn by hand as letter grids in boxing_art.py. This shades each material,
lit from the upper left, and outlines every sprite, so the whole cast shares one look; the
stars, impact bursts and the crowd are drawn here from shapes. Sprites are 4 bits a pixel:
index 0 is clear and the rest index a palette, so one drawing serves every fighter's colors.
The screen draws each art pixel as a 2 x 2 block.

    python3 tools/spritegen.py              writes firmware/src/sprites.rs
    python3 tools/spritegen.py --preview D  also writes contact sheets to D as .ppm files

Needs only the Python standard library.
"""

import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import boxing_art as art  # noqa: E402

# Palette slots. Every opponent palette fills all of them; the player has its own palette.
CLEAR, OUT = 0, 1
SKIN = (2, 3, 4)  # shadow, base, highlight
HAIR = (5, 6, 6)
TOP = (7, 8, 8)
TRUNKS = (9, 10, 10)
ACC = 11
GLOVE = (12, 13, 14)
WHITE = 14
GEAR = 15

LIGHT = (-0.48, -0.62, 0.62)
_n = math.sqrt(sum(c * c for c in LIGHT))
LIGHT = tuple(c / _n for c in LIGHT)


def tone(tones, nx, ny, nz):
    """Shadow, base or highlight for a surface normal."""
    i = nx * LIGHT[0] + ny * LIGHT[1] + nz * LIGHT[2]
    if i < 0.32:
        return tones[0]
    if i > 0.86:
        return tones[2]
    return tones[1]


class Layer:
    """A drawing surface around a pivot: coordinates are relative to the pivot."""

    def __init__(self, size=128):
        self.n = size
        self.c = size // 2
        self.px = [[CLEAR] * size for _ in range(size)]

    def get(self, x, y):
        x, y = x + self.c, y + self.c
        if 0 <= x < self.n and 0 <= y < self.n:
            return self.px[y][x]
        return CLEAR

    def set(self, x, y, v):
        x, y = x + self.c, y + self.c
        if 0 <= x < self.n and 0 <= y < self.n:
            self.px[y][x] = v

    def span(self):
        return range(-self.c, self.n - self.c)

    def ellipse(self, cx, cy, rx, ry, tones, flat=False):
        for y in range(int(cy - ry) - 1, int(cy + ry) + 2):
            for x in range(int(cx - rx) - 1, int(cx + rx) + 2):
                nx, ny = (x + 0.5 - cx) / rx, (y + 0.5 - cy) / ry
                d = nx * nx + ny * ny
                if d <= 1.0:
                    if flat or isinstance(tones, int):
                        self.set(x, y, tones if isinstance(tones, int) else tones[1])
                    else:
                        self.set(x, y, tone(tones, nx, ny, math.sqrt(max(0.0, 1 - d))))

    def poly(self, pts, fill):
        """Fills a polygon; `fill` is an index or a function of (x, y)."""
        ys = [p[1] for p in pts]
        for y in range(int(min(ys)) - 1, int(max(ys)) + 2):
            py = y + 0.5
            xs = []
            for (ax, ay), (bx, by) in zip(pts, pts[1:] + pts[:1]):
                if (ay <= py < by) or (by <= py < ay):
                    xs.append(ax + (py - ay) * (bx - ax) / (by - ay))
            xs.sort()
            for a, b in zip(xs[::2], xs[1::2]):
                for x in range(int(math.floor(a + 0.5)), int(math.floor(b + 0.5))):
                    self.set(x, y, fill(x, y) if callable(fill) else fill)

    def recolor(self, mask, mapping):
        """Maps colors inside `mask(x, y)`."""
        for y in self.span():
            for x in self.span():
                v = self.get(x, y)
                if v in mapping and mask(x, y):
                    self.set(x, y, mapping[v])

    def outlined(self, color=OUT):
        """A copy with a one pixel outline around everything drawn."""
        out = Layer(self.n)
        for y in self.span():
            for x in self.span():
                v = self.get(x, y)
                if v:
                    out.set(x, y, v)
                elif any(self.get(x + dx, y + dy) for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))):
                    out.set(x, y, color)
        return out

    def paste(self, other, dx=0, dy=0):
        for y in other.span():
            for x in other.span():
                v = other.get(x, y)
                if v:
                    self.set(x + dx, y + dy, v)

    def crop(self):
        """(x0, y0, w, h) of everything drawn, relative to the pivot."""
        pts = [(x, y) for y in self.span() for x in self.span() if self.get(x, y)]
        if not pts:
            return (0, 0, 1, 1)
        x0, y0 = min(p[0] for p in pts), min(p[1] for p in pts)
        return (x0, y0, max(p[0] for p in pts) - x0 + 1, max(p[1] for p in pts) - y0 + 1)


def part(draw, outline=True):
    layer = Layer()
    draw(layer)
    return layer.outlined() if outline else layer


def stack(*layers):
    out = Layer()
    for layer in layers:
        out.paste(layer)
    return out


# ---------------------------------------------------------------------------------------------
# Hand-drawn grids: see boxing_art.py for the letters.

MATERIALS = {
    "s": SKIN, "h": (HAIR[0], HAIR[1], HAIR[1]), "t": TOP, "k": TRUNKS, "g": GLOVE,
    "w": (WHITE,) * 3, "a": (ACC,) * 3, "e": (GEAR,) * 3, "o": (OUT,) * 3,
}
HIGHLIGHT = {"1": "s", "2": "g", "3": "h"}
# Brass in its base tone, never shaded at an edge: the thin walls of the knuckles' rings.
BASE = {"4": "g"}


def from_grid(rows, pivot, edge=1, rim=False):
    """A layer from a grid, pivot at (column, row). Shades each material's lower right `edge`
    pixels; with `rim`, lights its upper left edge too. Then outlines it."""
    l = Layer()
    px, py = pivot
    h, w = len(rows), max(len(r) for r in rows)

    def mat(x, y):
        if 0 <= y < h and 0 <= x < len(rows[y]):
            c = rows[y][x]
            if c == ".":
                return None
            return BASE.get(c) or HIGHLIGHT.get(c, c.lower())
        return None

    for y in range(h):
        for x in range(len(rows[y])):
            c = rows[y][x]
            if c == ".":
                continue
            m = BASE.get(c) or HIGHLIGHT.get(c, c.lower())
            tones = MATERIALS[m]
            if c in BASE:
                v = tones[1]
            elif c in HIGHLIGHT:
                v = tones[2]
            elif c.isupper():
                v = tones[0]
            elif any(mat(x + d, y) != m or mat(x, y + d) != m for d in range(1, edge + 1)):
                v = tones[0]
            elif rim and (mat(x - 1, y) != m or mat(x, y - 1) != m) and mat(x + 2, y) == m and mat(x, y + 2) == m:
                v = tones[2]
            else:
                v = tones[1]
            l.set(x - px, y - py, v)
    return l.outlined()


def knuckled(pivot, rows):
    """An arm with the brass knuckles over its fist: KNUCKS centred on the fist's brass rows,
    their bar on the fist's second brass row, turned over for a fist that hangs down."""
    pad = 4
    w = max(len(r) for r in rows) + 2 * pad
    grid = [["."] * w for _ in range(pad)] + [list("." * pad + r.ljust(w - pad, ".")) for r in rows] + \
        [["."] * w for _ in range(pad)]
    brass = [(x, y) for y, r in enumerate(grid) for x, c in enumerate(r) if c in "g2"]
    xs, ys = [p[0] for p in brass], [p[1] for p in brass]
    for x, y in brass:
        grid[y][x] = "s"
    cx = (min(xs) + max(xs) + 1) // 2
    down = min(ys) > len(grid) // 2
    knucks = list(reversed(art.KNUCKS)) if down else art.KNUCKS
    bar = 2 if down else 4
    top = (max(ys) if down else min(ys) + 1) - bar
    left = cx - len(knucks[0]) // 2
    for dy, r in enumerate(knucks):
        for dx, c in enumerate(r):
            if c != ".":
                grid[top + dy][left + dx] = c
    return (pivot[0] + pad, pivot[1] + pad), ["".join(r) for r in grid]


EXPRESSIONS = ["normal", "angry", "hurt", "dazed", "grin", "ko"]  # the order of FACE


# ---------------------------------------------------------------------------------------------
# Effects and the crowd.

def star(size):
    def draw(l):
        r = size
        pts = []
        for k in range(10):
            a = -math.pi / 2 + k * math.pi / 5
            rr = r if k % 2 == 0 else r * 0.45
            pts.append((rr * math.cos(a), rr * math.sin(a)))
        l.poly(pts, 2)
        l.set(-1, -1, 3)
        l.set(0, -1, 3)
    return part(draw)


def burst():
    def draw(l):
        pts = []
        for k in range(16):
            a = k * math.pi / 8 + 0.2
            rr = 11 if k % 2 == 0 else 5
            pts.append((rr * math.cos(a), rr * math.sin(a)))
        l.poly(pts, 2)
        inner = [(p[0] * 0.55, p[1] * 0.55) for p in pts]
        l.poly(inner, 3)
        l.ellipse(0, 0, 2.5, 2.5, 4)
    return part(draw)


def sweat():
    def draw(l):
        l.ellipse(0, 1, 1.6, 1.8, 3)
        l.set(0, -1, 3)
        l.set(0, 0, 4)
    return part(draw)


CROWD_W, CROWD_H = 64, 28


def crowd():
    """A tile of the crowd: rows of heads and shoulders, dimmer toward the back."""
    rng = 12345

    def rand(n):
        nonlocal rng
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        return (rng >> 8) % n

    tile = [[1] * CROWD_W for _ in range(CROWD_H)]
    rows = [(4, 2, 3.2), (12, 4, 3.8), (21, 6, 4.4)]
    for (y, base, r) in rows:
        x = rand(4)
        while x < CROWD_W + 8:
            shirt = base + rand(2)
            skin = 8 + rand(3)
            for yy in range(CROWD_H):
                for xx in range(-8, 9):
                    tx = (x + xx) % CROWD_W
                    dy = yy - y
                    if dy > r * 0.7 and abs(xx) < r * 1.9 - (dy - r) * 0.1:
                        tile[yy][tx] = shirt
                    elif (xx * xx) / (r * r) + (dy * dy) / ((r + 0.6) ** 2) <= 1:
                        tile[yy][tx] = skin if dy > -r * 0.35 else 11 + (shirt & 1)
            x += int(r * 2.2) + rand(3)
    # Camera flashes.
    for _ in range(3):
        fx, fy = rand(CROWD_W), rand(CROWD_H - 4)
        tile[fy][fx] = 15
    l = Layer(64)
    l.c = 0
    l.px = tile
    return tile


# ---------------------------------------------------------------------------------------------
# The startup screen: the logo, and 彙 and ★ for its text.

LOGO_BOX = (76, 40)  # the most room the logo takes, in art pixels
LOGO_PNG = os.path.join(os.path.dirname(os.path.abspath(__file__)), "art", "logo.png")


def read_png(path):
    """(width, height, rows of (r, g, b, a)) from an 8-bit, non-interlaced PNG."""
    import struct
    import zlib
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    pos, idat, palette, trns = 8, b"", None, None
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        pos += 12 + n
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
        elif kind == b"PLTE":
            palette = [tuple(body[i:i + 3]) for i in range(0, len(body), 3)]
        elif kind == b"tRNS":
            trns = body
        elif kind == b"IDAT":
            idat += body
    if depth != 8 or interlace:
        raise ValueError("logo.png must be 8 bits per channel and not interlaced")
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ctype]
    raw = zlib.decompress(idat)
    stride = w * channels
    rows, prev, i = [], bytearray(stride), 0
    for _ in range(h):
        f, line = raw[i], bytearray(raw[i + 1:i + 1 + stride])
        i += 1 + stride
        for x in range(stride):
            a = line[x - channels] if x >= channels else 0
            b, c = prev[x], prev[x - channels] if x >= channels else 0
            if f == 1:
                line[x] = (line[x] + a) & 255
            elif f == 2:
                line[x] = (line[x] + b) & 255
            elif f == 3:
                line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        prev = line
        px = []
        for x in range(w):
            v = line[x * channels:(x + 1) * channels]
            if ctype == 0:
                px.append((v[0], v[0], v[0], 255))
            elif ctype == 2:
                px.append((v[0], v[1], v[2], 255))
            elif ctype == 3:
                r, g, bl = palette[v[0]]
                px.append((r, g, bl, trns[v[0]] if trns and v[0] < len(trns) else 255))
            elif ctype == 4:
                px.append((v[0], v[0], v[0], v[1]))
            else:
                px.append(tuple(v))
        rows.append(px)
    return w, h, rows


def logo_from_png(path):
    """The logo PNG fitted into LOGO_BOX and cut down to 15 colors: (rows of indices, palette)."""
    w, h, px = read_png(path)
    scale = min(LOGO_BOX[0] / w, LOGO_BOX[1] / h)
    tw, th = max(1, round(w * scale)), max(1, round(h * scale))
    cells = []
    for y in range(th):
        row = []
        for x in range(tw):
            # Average the source pixels this art pixel covers.
            x0, x1 = int(x / scale), max(int(x / scale) + 1, int((x + 1) / scale))
            y0, y1 = int(y / scale), max(int(y / scale) + 1, int((y + 1) / scale))
            acc, n = [0, 0, 0, 0], 0
            for yy in range(y0, min(y1, h)):
                for xx in range(x0, min(x1, w)):
                    for k in range(4):
                        acc[k] += px[yy][xx][k]
                    n += 1
            row.append(tuple(v // max(n, 1) for v in acc))
        cells.append(row)
    # Median cut down to 15 colors.
    colors = [c[:3] for row in cells for c in row if c[3] >= 128]
    boxes = [colors] if colors else [[(0, 0, 0)]]
    while len(boxes) < 15:
        boxes.sort(key=lambda b: -len(b))
        b = boxes[0]
        if len(b) < 2:
            break
        spans = [max(c[k] for c in b) - min(c[k] for c in b) for k in range(3)]
        k = spans.index(max(spans))
        b.sort(key=lambda c: c[k])
        boxes = boxes[1:] + [b[:len(b) // 2], b[len(b) // 2:]]
    pal = [tuple(sum(c[k] for c in b) // len(b) for k in range(3)) for b in boxes if b]
    def nearest(c):
        return 1 + min(range(len(pal)), key=lambda i: sum((c[k] - pal[i][k]) ** 2 for k in range(3)))
    rows = [[nearest(c[:3]) if c[3] >= 128 else 0 for c in row] for row in cells]
    palette = [0] + [r << 16 | g << 8 | b for r, g, b in pal] + [0] * (15 - len(pal))
    return rows, palette


def logo_placeholder():
    """Brass knuckles in gold, until a logo.png is given: four rings over a curved grip."""
    l = Layer()
    for k in range(4):
        cx = -21 + k * 14
        l.ellipse(cx, -8, 8, 8, (2, 3, 4))
        l.ellipse(cx, -8, 4.2, 4.2, 0, flat=True)
    l.poly([(-29, -1), (29, -1), (26, 6), (18, 12), (-18, 12), (-26, 6)], lambda x, y: 3 if y < 3 else 2)
    l.poly([(-20, 2), (20, 2), (15, 7), (-15, 7)], 0)
    for k in range(4):
        l.set(-24 + k * 14, -13, 4)
        l.set(-23 + k * 14, -14, 4)
    return l.outlined()


LOGO_PALETTE = [0x000000, 0x120806, 0x7A5208, 0xB8861A, 0xE8B428, 0xFFE27A, 0xFFF8D8] + [0] * 9


def logo_from_mask(mask):
    """The logo silhouette in brass: lit from the top left, darker toward the bottom right,
    with a dark rim and a black outline two pixels wide."""
    h, w = len(mask), len(mask[0])
    on = lambda x, y: 0 <= x < w and 0 <= y < h and mask[y][x] == "#"
    l = Layer()
    for y in range(h):
        for x in range(w):
            if not on(x, y):
                continue
            if not on(x - 1, y) or not on(x, y - 1):
                v = 6 if not on(x - 1, y - 1) and not on(x, y - 1) else 5
            elif not on(x + 1, y) or not on(x, y + 1):
                v = 2
            elif not on(x + 2, y) or not on(x, y + 2):
                v = 3
            else:
                v = 5 if (x + 2 * y) < w // 2 + h // 3 else 4
            l.set(x - w // 2, y - h // 2, v)
    return l.outlined(1).outlined(1)


def glyph(rows):
    """A 1-bit grid as a sprite of index 1."""
    return [[1 if c == "#" else 0 for c in r] for r in rows]


# ---------------------------------------------------------------------------------------------
# Stage backdrops: one 64 x 28 tile each, repeated across the back of the ring.

def tile_shack():
    """A ham shack: wood panelling, QSL cards, and shelves of radios with lit dials."""
    w, h = CROWD_W, CROWD_H
    t = [[2 if (x % 9) else 3 for x in range(w)] for y in range(h)]
    for y in range(h):
        t[y][(y * 7) % w] = 1
    # QSL cards along the top.
    for k, x in enumerate(range(2, w, 11)):
        for yy in range(1, 5):
            for xx in range(x, min(x + 7, w)):
                t[yy][xx] = 14 if k % 3 == 0 else 15 if k % 3 == 1 else 12
        t[2][x + 2] = 1
    for shelf in (12, 25):
        for x in range(w):
            t[shelf][x] = 4
            t[shelf + 1][x] = 5
    # Radios on each shelf.
    for shelf, top in ((12, 6), (25, 15)):
        x = 1
        k = 0
        while x < w - 4:
            rw = 9 + (k * 5) % 7
            body = 6 if k % 2 == 0 else 7
            for yy in range(top, shelf):
                for xx in range(x, min(x + rw, w)):
                    t[yy][xx] = body
            # The display, a knob or two, and a meter on some.
            for xx in range(x + 1, min(x + rw - 3, w)):
                t[top + 1][xx] = 9 if k % 3 else 10
            if top + 3 < shelf:
                t[top + 3][min(x + rw - 3, w - 1)] = 11
                t[top + 3][min(x + 2, w - 1)] = 11
            if k % 2 == 1 and top + 4 < shelf:
                for xx in range(x + 3, min(x + 6, w)):
                    t[top + 3][xx] = 12
                t[top + 3][min(x + 4, w - 1)] = 1
            t[top][min(x + rw - 2, w - 1)] = 13
            x += rw + 2
            k += 1
    return t


def tile_field():
    """Field Day: sky, a lattice tower with its wires, tents and grass."""
    w, h = CROWD_W, CROWD_H
    t = [[1 if y < 6 else 2 if y < 12 else 3 for x in range(w)] for y in range(h)]
    for cx, cy in ((8, 3), (40, 5)):
        for dx in range(-5, 6):
            for dy in range(-1, 2):
                if abs(dx) + 2 * abs(dy) < 6:
                    t[cy + dy][(cx + dx) % w] = 4
    # The tower: two legs and cross braces, and a beam on top.
    for y in range(2, 22):
        lx, rx = 46 - y // 6, 50 + y // 6
        t[y][lx] = 5
        t[y][rx] = 5
        if y % 4 == 0:
            for x in range(lx, rx + 1):
                t[y][x] = 5
        else:
            t[y][lx + (y % 4) * (rx - lx) // 4] = 6
    for x in range(42, 55):
        t[1][x] = 5
    # Wires down from the top of the tower.
    for k in range(1, 20):
        t[1 + k // 2][47 - k] = 6
        t[1 + k // 2][(49 + k) % w] = 6
    # Tents.
    for cx, color in ((14, 7), (28, 8), (60, 7)):
        for y in range(14, 22):
            half = (y - 14) * 3 // 4 + 1
            for x in range(cx - half, cx + half + 1):
                t[y][x % w] = color
        t[21][cx % w] = 1
    for y in range(20, h):
        for x in range(w):
            t[y][x] = 9 if (x * 3 + y) % 7 else 10
    return t


def tile_rooftop():
    """A rooftop at night: stars, the city's lit windows, and a beam antenna on its mast."""
    w, h = CROWD_W, CROWD_H
    t = [[1 if y < 8 else 2 for x in range(w)] for y in range(h)]
    for x, y in ((3, 1), (17, 4), (29, 0), (41, 3), (52, 6), (60, 2), (11, 7), (36, 9)):
        t[y][x] = 3
    x = 0
    k = 0
    while x < w:
        bw = 7 + (k * 3) % 6
        top = 9 + (k * 5) % 9
        for y in range(top, h):
            for xx in range(x, min(x + bw, w)):
                t[y][xx] = 4 if k % 2 else 5
        for y in range(top + 2, h - 1, 3):
            for xx in range(x + 1, min(x + bw - 1, w), 2):
                if (xx * 7 + y * 3 + k) % 5 < 2:
                    t[y][xx] = 6 if (xx + y) % 3 else 7
        x += bw
        k += 1
    # The mast and the yagi on it.
    for y in range(2, 14):
        t[y][30] = 8
    for x in range(22, 39):
        t[3][x] = 8
    for x in range(23, 39, 3):
        t[2][x] = 8
        t[4][x] = 8
    t[0][58] = 9
    t[1][58] = 9
    return t


STAGES = [
    # name, tile, palette
    ("arena", crowd, [0x000000, 0x0C0A12, 0x2A2238, 0x3A2A46, 0x3A3448, 0x4A3A5C, 0x584A6C, 0x6A5478, 0x7A5A48,
                      0x8C6A54, 0x9E7A60, 0x221A22, 0x302430, 0, 0, 0xFFFFE0]),
    ("shack", tile_shack, [0x000000, 0x1E120A, 0x3D2616, 0x2E1C10, 0x9A6A3A, 0x5A3818, 0x1C1C22, 0x4A4A54,
                           0x7A7A86, 0xFFB020, 0x40FF70, 0xD0D0D8, 0xE8E4D0, 0xFF3030, 0x4070E0, 0xF0D040]),
    ("field", tile_field, [0x000000, 0x3C8CE0, 0x5AA6F0, 0x82C0F8, 0xFFFFFF, 0x6A6A74, 0x2A2A30, 0xF07A20,
                           0x2E6AD8, 0x3E8A30, 0x56A646, 0, 0, 0, 0, 0]),
    ("rooftop", tile_rooftop, [0x000000, 0x060A1E, 0x0C1430, 0xFFFFFF, 0x14141E, 0x1C1C2A, 0xFFD050, 0xFF9030,
                               0x6A6A7A, 0xFF3030, 0, 0, 0, 0, 0, 0]),
]


# ---------------------------------------------------------------------------------------------
# Palettes. Slot order: see the constants at the top.

def pal(out, skin, hair, top, trunks, acc, glove, white, gear):
    return [0x000000, out, *skin, *hair, *top, *trunks, acc, *glove[:2], white, gear]


SKINS = {
    "light": (0xC98A62, 0xF2C39B, 0xFFE2C6),
    "pale": (0xD09C80, 0xF6D3B6, 0xFFF0E0),
    "tan": (0xA86A44, 0xD9A06E, 0xF2C694),
    "brown": (0x7A4A2C, 0xA86E45, 0xC99062),
    "deep": (0x4E2C1A, 0x7A4A2E, 0x9C6844),
}

FIGHTERS = [
    # name (the key into boxing_art's HAIR and FACEWEAR), build, top, legs, palette
    ("larry", "medium", "singlet", "stand",
     pal(0x24140E, SKINS["light"], (0x96501C, 0xD8792E), (0xA8A49A, 0xEAE6DA), (0x4E3618, 0x7C5A30), 0xD8B848,
         (0x7E2016, 0xC0432E), 0xFFFFFF, 0x5C6676)),
    ("queen", "medium", "sports", "stand",
     pal(0x200A1C, SKINS["brown"], (0x7E1658, 0xC42A86), (0x4E1880, 0x8A36C8), (0x9C7414, 0xE2B232), 0xFFE070,
         (0x9E2660, 0xE8549A), 0xFFFFFF, 0xFFD03C)),
    ("pete", "medium", "bare", "stand",
     pal(0x1C1010, SKINS["tan"], (0x0E0A0E, 0x2C2430), SKINS["tan"][:2], (0x861010, 0xD02A2A), 0xF2F2F2,
         (0x1A2A84, 0x3454CC), 0xFFFFFF, 0xD02A2A)),
    ("bob", "medium", "bare", "barefoot",
     pal(0x20140C, SKINS["light"], (0x5A3410, 0x8C5A2A), SKINS["light"][:2], (0x1E6222, 0x3C9C42), 0xE8D040,
         (0x683818, 0xA46232), 0xFFFFFF, 0xE04848)),
    ("carl", "heavy", "singlet", "stand",
     pal(0x14141C, SKINS["pale"], (0x5C4C3C, 0x846C54), (0x2E6EAE, 0x5EA0E0), (0x141C4A, 0x283A84), 0xEAEAEA,
         (0x161616, 0x3A3A40), 0xFFFFFF, 0x3E4652)),
    ("sadie", "medium", "sports", "stand",
     pal(0x180C08, SKINS["deep"], (0xAE8620, 0xE8C24A), (0x0E6E62, 0x22B2A2), (0x121212, 0x34343A), 0x46E2D2,
         (0xA8A8B8, 0xEAEAF2), 0xFFFFFF, 0xB8702A)),
    ("sid", "medium", "vest", "stand",
     pal(0x0C0C10, SKINS["light"], (0x08080C, 0x24242E), (0x18181C, 0x34343C), (0x801010, 0xC82222), 0xC4C4CC,
         (0x861414, 0xDA3434), 0xFFFFFF, 0x0E0E14)),
    ("baron", "heavy", "singlet", "stand",
     pal(0x180818, SKINS["pale"], (0xA0A0B0, 0xE2E2EA), (0x341048, 0x5C2284), (0x3A1052, 0x6C2A92), 0xF2C232,
         (0xA07010, 0xE8B424), 0xFFFFFF, 0xF2C232)),
]

# Your brass knuckles take the glove slots: shadow, brass, and the shine.
PLAYER = pal(0x10121C, SKINS["tan"], (0x1C1210, 0x3A2A22), (0x18245E, 0x2C3E96), (0x8E1C1C, 0xC83030), 0xF4F4F4,
             (0x8A5E12, 0xE2B23A), 0xFFF4C8, 0x000000)

EFFECTS = [0x000000, 0x24140A, 0xFFC832, 0xFFFFFF, 0xFF6A1E, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
SWEAT = [0x000000, 0x1A2A4A, 0, 0x8ECBFF, 0xFFFFFF] + [0] * 11

# The crowd: shirts in three rows of brightness, skin, and hair.
CROWD = [0x000000, 0x0C0A12, 0x2A2238, 0x3A2A46, 0x3A3448, 0x4A3A5C, 0x584A6C, 0x6A5478, 0x7A5A48, 0x8C6A54,
         0x9E7A60, 0x221A22, 0x302430, 0, 0, 0xFFFFE0]
STAGES[0] = ("arena", crowd, CROWD)


# ---------------------------------------------------------------------------------------------
# Output.

class Sheet:
    """Every sprite, in the order the Rust tables list them."""

    def __init__(self):
        self.sprites = []

    def add(self, name, layer):
        x0, y0, w, h = layer.crop()
        rows = [[layer.get(x0 + x, y0 + y) for x in range(w)] for y in range(h)]
        self.sprites.append((name, w, h, -x0, -y0, rows))
        return len(self.sprites) - 1

    def tile(self, name, rows):
        self.sprites.append((name, len(rows[0]), len(rows), 0, 0, rows))


def head_part(name, pieces, layer):
    """Hair, gear or facewear pieces in the head frame."""
    hx, hy = art.HEAD_PIVOT
    return [from_grid(rows, (hx - left, hy - top), edge=1, rim=True)
            for (where, left, top, rows) in pieces if where == layer]


def fighter_head(name):
    hx, hy = art.HEAD_PIVOT
    base = from_grid(art.HEAD, (hx, hy), edge=2, rim=True)
    return stack(*head_part(name, art.HAIR[name], "back"), base, *head_part(name, art.HAIR[name], "front"))


def facewear(name):
    return stack(*head_part(name, art.FACEWEAR[name], "front"))


def face(kind):
    hx, hy = art.HEAD_PIVOT
    l = Layer()
    rows = art.FACES[kind]
    for y, row in enumerate(rows):
        for x, c in enumerate(row):
            if c != ".":
                m = HIGHLIGHT.get(c, c.lower())
                v = MATERIALS[m][0] if c.isupper() else MATERIALS[m][1]
                l.set(x - hx, y + art.FACE_TOP - hy, v)
    return l


def body(build, top):
    tx, ty = art.TORSO_PIVOT
    rows = list(art.TORSO)
    if build == "heavy":
        belly = art.BELLY
        off = (len(rows[0]) - len(belly[0])) // 2
        for k, brow in enumerate(belly):
            y = len(rows) - len(belly) + k
            r = list(rows[y])
            for x, c in enumerate(brow):
                if c != ".":
                    r[x + off] = c
            rows[y] = "".join(r)
    l = from_grid(rows, (tx, ty), edge=2, rim=True)
    to_top = {SKIN[0]: TOP[0], SKIN[1]: TOP[1], SKIN[2]: TOP[1]}
    if top == "singlet":
        def mask(x, y):
            d = abs(x + 0.5)
            if d > 17 and y < 16:
                return False
            if 8 <= d <= 13:
                return y >= 2
            return y >= 7 + max(0, (9 - d) // 2) if d < 8 else y >= 6
        l.recolor(mask, to_top)
    elif top == "sports":
        l.recolor(lambda x, y: (9 <= y <= 16 and abs(x + 0.5) < 22) or (9 <= abs(x + 0.5) <= 12 and 2 <= y < 9), to_top)
    elif top == "vest":
        l.recolor(lambda x, y: abs(x + 0.5) >= 5 + y // 4 and not (abs(x + 0.5) > 18 and y < 15), to_top)
        for y in range(1, 28):
            l.set(5 + y // 4, y, OUT)
            l.set(-6 - y // 4, y, OUT)
    return l


def build():
    sheet = Sheet()
    tables = {}

    def table(name, items):
        tables[name] = [sheet.add(f"{name}_{k}", layer) for k, layer in enumerate(items)]

    torsos = [("medium", "singlet"), ("medium", "sports"), ("medium", "bare"), ("heavy", "singlet"),
              ("medium", "vest")]
    table("TORSO", [body(b, t) for b, t in torsos])
    table("LEGS", [from_grid(art.LEGS, art.LEGS_PIVOT, edge=2, rim=True),
                   from_grid(art.LEGS_BARE, art.LEGS_PIVOT, edge=2, rim=True),
                   from_grid(art.KNEEL, art.KNEEL_PIVOT, edge=2, rim=True)])
    table("ARM", [from_grid(rows, pivot, edge=2, rim=True) for pivot, rows in
                  (art.ARMS[p] for p in ("guard", "cock", "punch", "swing", "low", "block", "raise"))])
    table("HEAD", [fighter_head(f[0]) for f in FIGHTERS])
    table("GEAR", [facewear(f[0]) for f in FIGHTERS])
    table("FACE", [face(k) for k in EXPRESSIONS])
    table("EXTRA", [from_grid(art.BOLT, art.BOLT_PIVOT), from_grid(art.LYING, art.LYING_PIVOT, edge=2, rim=True)])
    table("HERO_BODY", [from_grid(art.HERO_BODY, art.HERO_BODY_PIVOT, edge=2, rim=True)])
    table("HERO_ARM", [from_grid(*reversed(knuckled(*art.HERO_ARMS[p])), edge=2, rim=True)
                       for p in ("guard", "jab", "hook", "upper", "raise", "low", "tuck")])
    table("FX", [star(3.6), star(5.5), burst(), sweat()])
    tables["STAGE_TILE"] = []
    for name, make, _ in STAGES:
        sheet.tile("STAGE_" + name, make())
        tables["STAGE_TILE"].append(len(sheet.sprites) - 1)
    global LOGO_PALETTE
    if os.path.exists(LOGO_PNG):
        rows, LOGO_PALETTE = logo_from_png(LOGO_PNG)
        sheet.tile("LOGO", rows)
        sheet.sprites[-1] = sheet.sprites[-1][:3] + (len(rows[0]) // 2, len(rows) // 2) + sheet.sprites[-1][5:]
        tables["LOGO"] = [len(sheet.sprites) - 1]
    else:
        table("LOGO", [logo_from_mask(art.LOGO_MASK)])
    tables["GLYPH"] = []
    for name, rows in (("HUI", art.HUI), ("STAR", art.STAR)):
        sheet.tile(name, glyph(rows))
        tables["GLYPH"].append(len(sheet.sprites) - 1)
    return sheet, tables, torsos


def encode(rows, raw=False):
    """(data, marks) for a sprite, as firmware/src/art.rs reads them: run-length coded with a
    mark every eight rows, or packed raw, whichever is smaller."""
    packed = []
    for row in rows:
        padded = list(row) + [0] * (len(row) % 2)
        packed += [padded[k] << 4 | padded[k + 1] for k in range(0, len(padded), 2)]
    runs, marks = [], []
    for y, row in enumerate(rows):
        if y % 8 == 0:
            marks.append(len(runs))
        k = 0
        while k < len(row):
            j = k
            while j < len(row) and row[j] == row[k] and j - k < 16:
                j += 1
            runs.append((j - k - 1) << 4 | row[k])
            k = j
    if raw or len(packed) <= len(runs) + 2 * len(marks):
        return packed, []
    assert len(runs) < 65536
    return runs, marks


def rust(sheet, tables, torsos):
    out = []
    w = out.append
    w("// Generated by tools/spritegen.py: the pixel art of Brass Knuckles Knock Out.")
    w("// Regenerate with the tool; hand edits are lost.")
    w("")
    w("use crate::gfx::rgb;")
    w("")
    w("pub use crate::art::{Palette, Sprite};")
    w("")
    total = 0
    for name, ids in tables.items():
        w(f"pub static {name}: [Sprite; {len(ids)}] = [")
        for i in ids:
            _, sw, sh, ox, oy, rows = sheet.sprites[i]
            data, marks = encode(rows)
            total += len(data) + 2 * len(marks)
            w(f"    Sprite {{ w: {sw}, h: {sh}, ox: {ox}, oy: {oy}, marks: &{marks}, data: &[")
            for k in range(0, len(data), 24):
                w("        " + " ".join(f"0x{b:02X}," for b in data[k:k + 24]))
            w("    ] },")
        w("];")
        w("")

    def palette(name, colors, doc):
        w(f"/// {doc}")
        w(f"pub static {name}: Palette = [")
        for k in range(0, 16, 4):
            w("    " + " ".join(f"rgb(0x{c:06X})," for c in colors[k:k + 4]))
        w("];")
        w("")

    w(f"/// Each opponent: torso, legs standing and kneeling, head and gear, by index into the tables.")
    w("pub struct Look {")
    w("    pub torso: u8,")
    w("    pub legs: u8,")
    w("    pub kneel: u8,")
    w("    /// Lightning bolt on the chest.")
    w("    pub emblem: bool,")
    w("}")
    w("")
    w(f"pub static LOOKS: [Look; {len(FIGHTERS)}] = [")
    for f in FIGHTERS:
        _, b, t, lg, _ = f
        ti = torsos.index(("heavy" if b == "heavy" else "medium", t))
        bare = lg == "barefoot"
        emb = f[0] == "baron"
        w(f"    Look {{ torso: {ti}, legs: {1 if bare else 0}, kneel: 2, emblem: {'true' if emb else 'false'} }},")
    w("];")
    w("")
    w(f"pub static FIGHTER_PALETTES: [Palette; {len(FIGHTERS)}] = [")
    for f in FIGHTERS:
        w("    [")
        for k in range(0, 16, 4):
            w("        " + " ".join(f"rgb(0x{c:06X})," for c in f[4][k:k + 4]))
        w("    ],")
    w("];")
    w("")
    palette("PLAYER_PALETTE", PLAYER, "You.")
    palette("FX_PALETTE", EFFECTS, "Stars and impact bursts.")
    palette("SWEAT_PALETTE", SWEAT, "Sweat drops.")
    w(f"pub static STAGE_PALETTES: [Palette; {len(STAGES)}] = [")
    for _, _, colors in STAGES:
        w("    [")
        for k in range(0, 16, 4):
            w("        " + " ".join(f"rgb(0x{c:06X})," for c in colors[k:k + 4]))
        w("    ],")
    w("];")
    w("")
    palette("LOGO_PALETTE", LOGO_PALETTE, "The startup logo.")
    w(f"pub const ART_BYTES: usize = {total};")
    return "\n".join(out) + "\n", total


def preview(sheet, path, palette_for):
    """A contact sheet of every sprite, at 3x, as a PPM."""
    scale, pad = 3, 4
    cells = []
    x = y = 0
    row_h = 0
    width = 1100
    for i, (name, w, h, ox, oy, rows) in enumerate(sheet.sprites):
        if x + w * scale + pad > width:
            x, y, row_h = 0, y + row_h + pad, 0
        cells.append((x, y, i))
        x += w * scale + pad
        row_h = max(row_h, h * scale)
    height = y + row_h + pad
    img = [[(40, 44, 52)] * width for _ in range(height)]
    for (cx, cy, i) in cells:
        name, w, h, ox, oy, rows = sheet.sprites[i]
        colors = palette_for(name)
        for yy in range(h):
            for xx in range(w):
                v = rows[yy][xx]
                if v:
                    c = colors[v]
                    rgb = ((c >> 16) & 255, (c >> 8) & 255, c & 255)
                    for sy in range(scale):
                        for sx in range(scale):
                            img[cy + yy * scale + sy][cx + xx * scale + sx] = rgb
    with open(path, "wb") as f:
        f.write(b"P6\n%d %d\n255\n" % (width, height))
        f.write(bytes(v for row in img for px in row for v in px))


def main():
    sheet, tables, torsos = build()
    text, total = rust(sheet, tables, torsos)
    here = os.path.dirname(os.path.abspath(__file__))
    target = os.path.join(here, "..", "firmware", "src", "sprites.rs")
    with open(target, "w") as f:
        f.write(text)
    print(f"{target}: {len(sheet.sprites)} sprites, {total} bytes of art")
    if "--preview" in sys.argv:
        d = sys.argv[sys.argv.index("--preview") + 1]
        os.makedirs(d, exist_ok=True)
        for k, f in enumerate(FIGHTERS):
            def pf(name, f=f):
                if name.startswith("HERO"):
                    return PLAYER
                if name.startswith("FX_3"):
                    return SWEAT
                if name.startswith("FX"):
                    return EFFECTS
                if name.startswith("STAGE_"):
                    return next(p for n, _, p in STAGES if name == "STAGE_" + n)
                if name.startswith("LOGO"):
                    return LOGO_PALETTE
                if name in ("HUI", "STAR"):
                    return [0, 0xFFE040] + [0] * 14
                return f[4]
            preview(sheet, os.path.join(d, f"sheet_{f[0]}.ppm"), pf)


if __name__ == "__main__":
    main()
