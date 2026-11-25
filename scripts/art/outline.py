"""Tell the dark-brown outline apart from dark fills, and measure how thick the outer outline is.

Tuned for the house style (docs/art-prompts.md, style B): a near-black dark-brown outline around
warm fur, muted mauve-brown paws, green leaves. Other palettes may need the thresholds below adjusted.

usage: python outline.py <frame.png>...
  prints each frame's height, outer outline width, and outline width as % of height.
  On screen a state is scaled to (base height x heightRatio), so compare frames with
  "% of height x heightRatio of that state".
"""
import sys

from PIL import Image

DARK_SUM = 200  # r+g+b below this is outline ...
PAW_MIN_R = 55  # ... unless it is the muted mauve of dark paws / far-side legs:
PAW_BLUE_RATIO = 0.3  # paws have b ~ 0.5 r, the outline b ~ 0.15-0.2 r
PAW_SURE_R = 80  # the outline core never gets this red
OPAQUE = 128
MAX_RUN = 30  # longer dark runs from the edge are a dark area, not the outline
SKIP_TOP_BOTTOM = 5  # measure the middle rows only (1/5 skipped at each end: ears, feet)
ROW_STEP = 3


def is_outline(r, g, b):
    if r + g + b >= DARK_SUM:
        return False
    if r >= PAW_SURE_R or g > r:  # g > r: dark green leaf shading
        return False
    return not (r >= PAW_MIN_R and b >= PAW_BLUE_RATIO * r)


def measure(path):
    """(figure height, median outer-outline width in px, width as % of height)."""
    im = Image.open(path).convert("RGBA")
    w = im.width
    px = im.load()
    bbox = im.getchannel("A").point(lambda v: 255 if v > OPAQUE else 0).getbbox()
    fig_h = bbox[3] - bbox[1]
    runs = []
    margin = fig_h // SKIP_TOP_BOTTOM
    for y in range(bbox[1] + margin, bbox[3] - margin, ROW_STEP):
        for xs in (range(w), range(w - 1, -1, -1)):
            started = False
            n = 0
            for x in xs:
                r, g, b, a = px[x, y]
                if not started:
                    if a <= OPAQUE:
                        continue
                    started = True
                if a > OPAQUE and is_outline(r, g, b):
                    n += 1
                else:
                    break
            if 0 < n < MAX_RUN:
                runs.append(n)
    runs.sort()
    med = runs[len(runs) // 2]
    return fig_h, med, med / fig_h * 100


if __name__ == "__main__":
    for p in sys.argv[1:]:
        fh, med, pct = measure(p)
        name = p.replace("\\", "/").split("/")[-1]
        print(f"{name:24s} height {fh:4d}  outline {med:2d}px  = {pct:.2f}% of height")
