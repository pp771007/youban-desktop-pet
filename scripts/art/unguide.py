"""Remove pure-blue (#0000FF) guide lines from a sheet before slicing.

Prompts may ask the AI to draw a blue vertical line as the wall (climbing frames) so it lines the
paws up against it; this makes those pixels transparent. Writes a new file.

usage: python unguide.py <sheet.png> <out.png>
"""
import sys

from PIL import Image

BLUE_MARGIN = 40  # blue must exceed both red and green by this much


def main():
    src, dst = sys.argv[1], sys.argv[2]
    im = Image.open(src).convert("RGBA")
    px = im.load()
    removed = 0
    for y in range(im.height):
        for x in range(im.width):
            r, g, b, a = px[x, y]
            if b > r + BLUE_MARGIN and b > g + BLUE_MARGIN:
                px[x, y] = (r, g, b, 0)
                removed += 1
    im.save(dst)
    print("removed px:", removed)


if __name__ == "__main__":
    main()
