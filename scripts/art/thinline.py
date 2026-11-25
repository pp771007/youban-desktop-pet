"""Cap the outer outline at a maximum width, in place.

Keeps only the outline within N px of the colored interior; lines already thinner than N
(an orange, a leaf, ears) are left as they are. Anti-aliased via 4x supersampling.

usage: python thinline.py <target_pct> <height_ratio> <frame.png>...
  target_pct    outline width as % of height on screen, measured on the existing frames with outline.py
  height_ratio  the state's heightRatio in pet.json (1.0 if not set)
  Pass all frames of ONE state/variant at once: N is computed from the tallest frame,
  because the engine scales a whole variant by its tallest frame.

Overwrites the files - commit the originals first.
"""
import math
import sys

from PIL import Image, ImageChops, ImageFilter

from outline import OPAQUE, is_outline, measure

SS = 4  # supersampling factor
SIGMA_PX = 3.0  # blur used to grow the interior; must stay well above the cap width / 2
INTERIOR_ALPHA = 200


def cap(path, width):
    im = Image.open(path).convert("RGBA")
    alpha = im.getchannel("A")
    interior = Image.new("L", im.size, 0)
    ip = interior.load()
    px = im.load()
    for y in range(im.height):
        for x in range(im.width):
            r, g, b, a = px[x, y]
            if a > INTERIOR_ALPHA and not is_outline(r, g, b):
                ip[x, y] = 255
    big = interior.resize((im.width * SS, im.height * SS), Image.BICUBIC)
    big = big.point(lambda v: 255 if v >= OPAQUE else 0)
    sigma = SIGMA_PX * SS
    blurred = big.filter(ImageFilter.GaussianBlur(sigma))
    # grow the interior by `width`: a point d px outside its edge blurs to 1 - phi(d / sigma)
    phi = 0.5 * (1 + math.erf(width * SS / sigma / math.sqrt(2)))
    threshold = (1 - phi) * 255
    band = blurred.point(lambda v: 255 if v > threshold else 0).resize(im.size, Image.BOX)
    im.putalpha(ImageChops.darker(alpha, band))
    im.save(path)


def main():
    target_pct, ratio, files = float(sys.argv[1]), float(sys.argv[2]), sys.argv[3:]
    tallest = max(measure(f)[0] for f in files)
    width = target_pct / 100 * tallest / ratio
    print(f"tallest {tallest}px -> cap outline at {width:.1f}px")
    for f in files:
        cap(f, width)


if __name__ == "__main__":
    main()
