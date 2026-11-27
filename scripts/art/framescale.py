"""Check that every frame of one state/variant draws the character at the same size; optionally fix it, in place.

AI draws each frame of a walk cycle separately and the size drifts (a capybara walk varied by 8%).
The engine scales a whole variant by one factor, so a frame drawn smaller plays as the body shrinking.

usage: python framescale.py [--box L,T,R,B] [--apply] [--overlay out.png] <frame_01.png> <frame_02.png>...
  --box      the part to compare, as fractions of the first frame (default 0.5,0,1,0.62 = top right,
             the head of a pet facing right). Pick something rigid - head, the orange on top - not the
             whole body: legs and tails move between frames and make the size look different.
             The box is a share of the first frame, so when one frame is taller (a raised tail) a
             loose box catches empty space - narrow it to the head. Frames that turn the head
             (a climb that bends) cannot be matched against frame 1; compare same-pose frames
             with each other. A result at the edge of the search (0.80 or 1.25) with a high error
             means it found nothing.
  --apply    shrink every frame to the size of the smallest one (shrinking keeps the art sharp)
  --overlay  blend the first frame's box with the frame that differs most, at scale 1 and at the
             measured scale; the measured one should line up. Check it before --apply: frames that
             mirror or tilt (a dizzy sway) cannot be matched this way and give a meaningless number.

Prints for each frame how big it is drawn relative to the first (0.92 = 8% smaller) and the match
error (lower is better; around 15-30 for real frames of one animation).
Separate drawings never match exactly (the eye sits a little elsewhere), so a few percent either way
is noise: moving the box changes the answer by that much. Fix only what is clearly off (8% and up)
and what the overlay confirms - the right tile should line up the outline and the orange where the
left one shows them doubled.
Needs numpy and Pillow. Overwrites the files with --apply - commit the originals first.
"""
import argparse

import numpy as np
from PIL import Image

SCALES = np.arange(0.80, 1.25, 0.005)
# Measure on copies shrunk so the first frame is this tall: fast, and 1px is still 0.5%.
WORK_H = 200
# How far the box may move between frames, as a share of the height (a raised tail makes one frame taller).
SHIFT_FRAC = 0.15
TOLERANCE = 0.01  # sizes closer than this are left alone
# Shown as the overlay, enlarged so the doubled lines are easy to see.
OVERLAY_ZOOM = 3


def error(a, b):
    m = (a[..., 3] > 128) | (b[..., 3] > 128)
    return np.abs(a[..., :3] - b[..., :3])[m].mean() + np.abs(a[..., 3] - b[..., 3]).mean()


def match(tpl, from_right, top, im, scales):
    """Best (error, scale, x, y) over `scales`: where `im`, scaled, lines up with the template.
    The search starts where the box sat in the first frame, measured from the right edge (pets face right)."""
    th, tw = tpl.shape[:2]
    shift = round(WORK_H * SHIFT_FRAC)
    best = None
    for s in scales:
        r = np.asarray(im.resize((round(im.width * s), round(im.height * s)), Image.LANCZOS)).astype(float)
        h, w = r.shape[:2]
        x0 = w - from_right - tw
        for y in range(max(0, top - shift), min(h - th, top + shift) + 1):
            for x in range(max(0, x0 - shift), min(w - tw, x0 + shift) + 1):
                e = error(r[y:y + th, x:x + tw], tpl)
                if best is None or e < best[0]:
                    best = (e, s, x, y)
    return best


def on_white(im):
    return Image.alpha_composite(Image.new('RGBA', im.size, (255, 255, 255, 255)), im)


def overlay_tile(ref_box, im, scale, x, y):
    crop = im.resize((round(im.width * scale), round(im.height * scale)), Image.LANCZOS)
    crop = crop.crop((x, y, x + ref_box.width, y + ref_box.height))
    tile = Image.blend(on_white(ref_box), on_white(crop), 0.5)
    return tile.resize((tile.width * OVERLAY_ZOOM, tile.height * OVERLAY_ZOOM), Image.LANCZOS)


def main():
    ap = argparse.ArgumentParser(usage=__doc__)
    ap.add_argument('frames', nargs='+')
    ap.add_argument('--box', default='0.5,0,1,0.62')
    ap.add_argument('--apply', action='store_true')
    ap.add_argument('--overlay')
    a = ap.parse_args()

    originals = [Image.open(f).convert('RGBA') for f in a.frames]
    k = WORK_H / originals[0].height
    ims = [im.resize((max(1, round(im.width * k)), max(1, round(im.height * k))), Image.LANCZOS) for im in originals]
    ref = ims[0]
    l, t, r, b = (float(v) for v in a.box.split(','))
    box = (round(ref.width * l), round(ref.height * t), round(ref.width * r), round(ref.height * b))
    ref_box = ref.crop(box)
    tpl = np.asarray(ref_box).astype(float)
    from_right, top = ref.width - box[2], box[1]

    drawn, found = [1.0], [None]
    for f, im in zip(a.frames[1:], ims[1:]):
        e, s, x, y = match(tpl, from_right, top, im, SCALES)
        drawn.append(1 / s)
        found.append((s, x, y))
        print(f'{f}: drawn {1 / s:.3f}, error {e:.1f}')

    if a.overlay and len(ims) > 1:
        worst = max(range(1, len(ims)), key=lambda i: abs(drawn[i] - 1))
        _, _, x1, y1 = match(tpl, from_right, top, ims[worst], [1.0])
        s, x, y = found[worst]
        tiles = [overlay_tile(ref_box, ims[worst], 1.0, x1, y1), overlay_tile(ref_box, ims[worst], s, x, y)]
        out = Image.new('RGBA', (tiles[0].width * 2 + 20, tiles[0].height), (255, 255, 255, 255))
        out.paste(tiles[0], (0, 0))
        out.paste(tiles[1], (tiles[0].width + 20, 0))
        out.save(a.overlay)
        print(f'overlay: {a.overlay} (left: scale 1, right: scale {s:.3f}, {a.frames[worst]})')

    if a.apply:
        target = min(drawn)
        for f, im, d in zip(a.frames, originals, drawn):
            k = target / d
            if abs(k - 1) > TOLERANCE:
                im = im.convert('RGBa').resize((round(im.width * k), round(im.height * k)), Image.LANCZOS).convert('RGBA')
                im.save(f, optimize=True)
                print(f'{f}: x{k:.3f}')


if __name__ == '__main__':
    main()
