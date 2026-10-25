# Draws ImGit's app icon: a rounded dark tile holding a small commit graph (a teal main lane, a magenta branch that
# leaves it and merges back), in the app's own palette. Writes the 1024px master, the sizes a Windows .ico holds,
# the .ico itself and the macOS .icns.
#
#   python tools/app_icon.py            -> assets/images/app_icon.png (256px, the Linux window icon), res/imgit.ico,
#                                          res/imgit.icns (the ImGit.app bundle's icon)
#   python tools/app_icon.py <out_dir>  -> previews only, into <out_dir>
import os
import sys

from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

BG_TOP = (0x2E, 0x32, 0x3A)
BG_BOTTOM = (0x1C, 0x1E, 0x23)
BORDER = (255, 255, 255, 26)
TEAL = (0x15, 0xA0, 0xBF)
MAGENTA = (0xC5, 0x17, 0xB6)
ORANGE = (0xF2, 0x5D, 0x2E)


def cubic(p0, p1, p2, p3, steps=200):
    points = []
    for i in range(steps + 1):
        t = i / steps
        u = 1 - t
        points.append((u ** 3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t ** 3 * p3[0],
                       u ** 3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t ** 3 * p3[1]))
    return points


def stroke(draw, points, width, color):
    """A round-capped stroke along `points`: discs stamped at most a sixth of the width apart."""
    r = width / 2
    step = max(1.0, width / 6)
    for (x0, y0), (x1, y1) in zip(points, points[1:]):
        length = ((x1 - x0) ** 2 + (y1 - y0) ** 2) ** 0.5
        n = max(1, int(length / step))
        for i in range(n + 1):
            x, y = x0 + (x1 - x0) * i / n, y0 + (y1 - y0) * i / n
            draw.ellipse((x - r, y - r, x + r, y + r), fill=color)


def node(draw, center, radius, color, ring, bg):
    x, y = center
    # a gap in the tile colour around the node keeps it apart from the lines
    g = radius + ring
    draw.ellipse((x - g, y - g, x + g, y + g), fill=bg)
    draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill=color)
    inner = radius * 0.42
    draw.ellipse((x - inner, y - inner, x + inner, y + inner), fill=bg)


def render(size, scale=4):
    """The icon at `size` px, drawn `scale` times larger and downsampled."""
    s = size * scale
    k = s / 1024.0  # design units are a 1024 canvas

    def P(x, y):
        return (x * k, y * k)

    # Small sizes get heavier strokes and nodes so the graph still reads.
    heavy = 1.0 if size >= 64 else (1.25 if size >= 32 else 1.45)

    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    # tile: vertical gradient inside a rounded square
    inset, radius = 56 * k, 228 * k
    gradient = Image.new("RGBA", (s, s))
    gd = ImageDraw.Draw(gradient)
    for y in range(s):
        t = y / (s - 1)
        gd.line([(0, y), (s, y)], fill=tuple(int(BG_TOP[i] + (BG_BOTTOM[i] - BG_TOP[i]) * t) for i in range(3)) + (255,))
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle((inset, inset, s - inset, s - inset), radius=radius, fill=255)
    img.paste(gradient, (0, 0), mask)
    d = ImageDraw.Draw(img)
    if size >= 32:
        d.rounded_rectangle((inset, inset, s - inset, s - inset), radius=radius, outline=BORDER, width=max(1, int(6 * k)))

    lane = 64 * k * heavy
    main_x, branch_x = 370, 672
    top, mid, bottom = 268, 512, 756
    bg_mid = tuple(int(BG_TOP[i] + (BG_BOTTOM[i] - BG_TOP[i]) * 0.5) for i in range(3))

    # main lane
    stroke(d, [P(main_x, top), P(main_x, bottom)], lane, TEAL)
    # the branch: out of the bottom commit, up to its own commit, back into the top one
    stroke(d, cubic(P(main_x, bottom), P(main_x, 620), P(branch_x, 660), P(branch_x, mid)), lane, MAGENTA)
    stroke(d, cubic(P(branch_x, mid), P(branch_x, 364), P(main_x, 404), P(main_x, top)), lane, MAGENTA)

    r = 84 * k * heavy
    ring = 20 * k * heavy
    node(d, P(main_x, bottom), r, TEAL, ring, bg_mid)
    node(d, P(branch_x, mid), r, MAGENTA, ring, bg_mid)
    node(d, P(main_x, top), r, ORANGE, ring, bg_mid)

    return img.resize((size, size), Image.LANCZOS)


def main():
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    if len(sys.argv) > 1:
        out = sys.argv[1]
        os.makedirs(out, exist_ok=True)
        for size in sizes + [1024]:
            render(size).save(os.path.join(out, f"icon_{size}.png"))
        return
    images = [render(size) for size in sizes]
    os.makedirs(os.path.join(ROOT, "assets", "images"), exist_ok=True)
    images[-1].save(os.path.join(ROOT, "assets", "images", "app_icon.png"))
    os.makedirs(os.path.join(ROOT, "res"), exist_ok=True)
    images[-1].save(os.path.join(ROOT, "res", "imgit.ico"), sizes=[(s, s) for s in sizes],
                    append_images=images[:-1])
    # .icns: the 1024px master plus the sizes drawn with their own stroke weights (Pillow downsamples the rest).
    render(1024).save(os.path.join(ROOT, "res", "imgit.icns"),
                      append_images=[render(size) for size in (32, 64, 128, 256, 512)])


if __name__ == "__main__":
    main()
