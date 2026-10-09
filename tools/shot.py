#!/usr/bin/env python3
"""Draw a headless Ohm screen as a PNG.

    cargo run --release -- --shot desk --cells | python3 tools/shot.py desk.png

Box drawing and block elements are drawn as shapes, the way VTE (GNOME Terminal) draws them
itself, so lines meet and blocks fill their cells exactly. Text is set in Ubuntu Sans Mono, with
DejaVu Sans Mono for anything outside ASCII. Adapted from SIGPIPE's tools/shot.py.
"""
import re
import sys
from PIL import Image, ImageDraw, ImageFont

CW, CH = 10, 20
TEXT_FONT = ImageFont.truetype("/usr/share/fonts/truetype/ubuntu/UbuntuSansMono[wght].ttf", 17)
TEXT_BOLD = ImageFont.truetype("/usr/share/fonts/truetype/ubuntu/UbuntuSansMono[wght].ttf", 17)
try:
    TEXT_BOLD.set_variation_by_name("Bold")
except Exception:
    pass
WIDE_FONT = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", 16)

# Box drawing: arms (N, E, S, W), each 0 none, 1 light, 2 heavy; half-arms end at the centre.
L, H = 1, 2
BOX = {
    "─": (0, L, 0, L), "│": (L, 0, L, 0), "┌": (0, L, L, 0), "┐": (0, 0, L, L),
    "└": (L, L, 0, 0), "┘": (L, 0, 0, L), "├": (L, L, L, 0), "┤": (L, 0, L, L),
    "┬": (0, L, L, L), "┴": (L, L, 0, L), "┼": (L, L, L, L),
    "╭": (0, L, L, 0), "╮": (0, 0, L, L), "╰": (L, L, 0, 0), "╯": (L, 0, 0, L),
    "╴": (0, 0, 0, L), "╵": (L, 0, 0, 0), "╶": (0, L, 0, 0), "╷": (0, 0, L, 0),
    "━": (0, H, 0, H), "┃": (H, 0, H, 0), "┏": (0, H, H, 0), "┓": (0, 0, H, H),
    "┗": (H, H, 0, 0), "┛": (H, 0, 0, H), "┣": (H, H, H, 0), "┫": (H, 0, H, H),
    "┳": (0, H, H, H), "┻": (H, H, 0, H), "╋": (H, H, H, H),
    "╸": (0, 0, 0, H), "╹": (H, 0, 0, 0), "╺": (0, H, 0, 0), "╻": (0, 0, H, 0),
}
# Block elements as (x0, y0, x1, y1) fractions of the cell.
BLOCK = {
    "█": [(0, 0, 1, 1)], "▀": [(0, 0, 1, .5)], "▄": [(0, .5, 1, 1)],
    "▌": [(0, 0, .5, 1)], "▐": [(.5, 0, 1, 1)],
    "▖": [(0, .5, .5, 1)], "▗": [(.5, .5, 1, 1)], "▘": [(0, 0, .5, .5)], "▝": [(.5, 0, 1, .5)],
    "▚": [(0, 0, .5, .5), (.5, .5, 1, 1)], "▞": [(.5, 0, 1, .5), (0, .5, .5, 1)],
    "▛": [(0, 0, 1, .5), (0, .5, .5, 1)], "▜": [(0, 0, 1, .5), (.5, .5, 1, 1)],
    "▙": [(0, 0, .5, .5), (0, .5, 1, 1)], "▟": [(.5, 0, 1, .5), (0, .5, 1, 1)],
    "▁": [(0, .875, 1, 1)], "▕": [(.875, 0, 1, 1)], "▏": [(0, 0, .125, 1)],
}
SHADE = {"░": .25, "▒": .5, "▓": .75}


def mix(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def main():
    lines = sys.stdin.read().splitlines()
    w, h = map(int, lines[0].split())
    img = Image.new("RGB", (w * CW, h * CH))
    d = ImageDraw.Draw(img)
    rgb = lambda s: tuple(int(s[i:i + 2], 16) for i in (0, 2, 4))
    for line in lines[1:]:
        x, y, fg, bg, bold, sym = line.split(" ", 5)
        x, y, fg, bg = int(x), int(y), rgb(fg), rgb(bg)
        sym = "".join(chr(int(c, 16)) for c in re.findall(r"\{([0-9a-f]+)\}", sym))
        x0, y0 = x * CW, y * CH
        d.rectangle((x0, y0, x0 + CW - 1, y0 + CH - 1), fill=bg)
        if sym in BOX:
            cx, cy = x0 + CW // 2, y0 + CH // 2
            n, e, s, wst = BOX[sym]
            width = lambda k: 2 if k == L else 4
            for arm, (ax, ay) in ((n, (cx, y0)), (s, (cx, y0 + CH))):
                if arm:
                    t = width(arm)
                    d.rectangle((cx - t // 2, min(ay, cy), cx - t // 2 + t - 1, max(ay, cy)), fill=fg)
            for arm, (ax, ay) in ((wst, (x0, cy)), (e, (x0 + CW, cy))):
                if arm:
                    t = width(arm)
                    d.rectangle((min(ax, cx), cy - t // 2, max(ax, cx), cy - t // 2 + t - 1), fill=fg)
        elif sym in BLOCK:
            for fx0, fy0, fx1, fy1 in BLOCK[sym]:
                d.rectangle((x0 + fx0 * CW, y0 + fy0 * CH, x0 + fx1 * CW - 1, y0 + fy1 * CH - 1), fill=fg)
        elif sym in SHADE:
            d.rectangle((x0, y0, x0 + CW - 1, y0 + CH - 1), fill=mix(bg, fg, SHADE[sym]))
        elif sym.strip():
            font = (TEXT_BOLD if bold == "1" else TEXT_FONT) if sym.isascii() else WIDE_FONT
            d.text((x0, y0 + 1), sym, font=font, fill=fg)
    img.save(sys.argv[1])


main()
