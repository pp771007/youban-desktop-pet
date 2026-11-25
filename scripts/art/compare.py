"""Lay frames side by side on a solid background to eyeball them.

usage: python compare.py <out.png> [--zoom Z] [--rev REV] <frame.png>...
  --zoom Z   scale every frame by Z (default 1); use 2-4 to inspect thin lines and paws
  --rev REV  show each frame as it was at git revision REV next to the current file
             (run from the repo; paths relative to the repo root)
Frames are bottom-aligned at their original relative sizes.
"""
import io
import subprocess
import sys

from PIL import Image, ImageDraw

BG = (92, 140, 112, 255)
GAP = 20
LABEL_H = 24


def load_rev(rev, path):
    blob = subprocess.run(
        ["git", "show", f"{rev}:{path.replace(chr(92), '/')}"], capture_output=True, check=True
    ).stdout
    return Image.open(io.BytesIO(blob)).convert("RGBA")


def main():
    args = sys.argv[1:]
    out = args.pop(0)
    zoom, rev = 1.0, None
    while args and args[0].startswith("--"):
        flag = args.pop(0)
        if flag == "--zoom":
            zoom = float(args.pop(0))
        elif flag == "--rev":
            rev = args.pop(0)
    items = []
    for f in args:
        name = f.replace("\\", "/").split("/")[-1]
        if rev:
            items.append((f"{name} @{rev}", load_rev(rev, f)))
        items.append((name, Image.open(f).convert("RGBA")))
    items = [(n, im.resize((round(im.width * zoom), round(im.height * zoom)), Image.NEAREST)) for n, im in items]
    height = max(im.height for _, im in items) + LABEL_H
    sheet = Image.new("RGBA", (sum(im.width for _, im in items) + GAP * len(items), height), BG)
    draw = ImageDraw.Draw(sheet)
    x = 0
    for name, im in items:
        sheet.alpha_composite(im, (x, height - LABEL_H - im.height))
        draw.text((x + 4, height - LABEL_H + 6), name, fill=(255, 255, 255, 255))
        x += im.width + GAP
    sheet.convert("RGB").save(out)


if __name__ == "__main__":
    main()
