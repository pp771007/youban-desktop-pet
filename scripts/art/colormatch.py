"""Match a batch of new frames to the existing frames' colors, in place.

Fits a per-channel linear map from two anchors - the outline color and the mid fur color -
measured on one existing frame (<old_ref>) and one frame of the new batch (<new_ref>),
then applies it to every file. Each AI batch drifts in color, so run it once per batch.

usage: python colormatch.py measure <frame.png>...
         prints the outline / fur / snout mean colors (snout = a check the map got right)
       python colormatch.py apply <old_ref> <new_ref> <frame.png>...
         <new_ref> should be a frame of the batch being fixed (may be in the file list too:
         the map is computed before anything is written). Pick refs with a similar pose.

Fur detection assumes warm, light fur (the capybara); other palettes need the thresholds adjusted.
Overwrites the files - commit the originals first, and never run apply twice on the same batch.
"""
import sys

from PIL import Image

OPAQUE = 250
LINE_MAX_R, LINE_MAX_G = 60, 40
FUR_MIN_R, FUR_MIN_G = 150, 100
FUR_R_MINUS_B = (50, 140)
SNOUT_R = (90, 150)
SNOUT_R_MINUS_G = (20, 60)
SNOUT_MIN_R_MINUS_B = 25


def anchors(path):
    """Mean (outline, fur, snout) colors of a frame."""
    im = Image.open(path).convert("RGBA")
    line, fur, snout = [], [], []
    px = im.load()
    for y in range(im.height):
        for x in range(im.width):
            classify(px[x, y], line, fur, snout)

    def mean(px):
        return tuple(round(sum(p[i] for p in px) / len(px), 1) for i in range(3))

    return mean(line), mean(fur), mean(snout)


def classify(pixel, line, fur, snout):
    r, g, b, a = pixel
    if a >= OPAQUE:
        if r < LINE_MAX_R and g < LINE_MAX_G:
            line.append((r, g, b))
        elif r > FUR_MIN_R and g > FUR_MIN_G and FUR_R_MINUS_B[0] <= r - b <= FUR_R_MINUS_B[1]:
            fur.append((r, g, b))
        elif (
            SNOUT_R[0] <= r <= SNOUT_R[1]
            and SNOUT_R_MINUS_G[0] <= r - g <= SNOUT_R_MINUS_G[1]
            and r - b >= SNOUT_MIN_R_MINUS_B
        ):
            snout.append((r, g, b))


def fit(old_ref, new_ref):
    (old_line, old_fur, _), (new_line, new_fur, _) = anchors(old_ref), anchors(new_ref)
    maps = []
    for i in range(3):
        k = (old_fur[i] - old_line[i]) / (new_fur[i] - new_line[i])
        maps.append((k, old_line[i] - k * new_line[i]))
    return maps


def apply_map(path, maps):
    im = Image.open(path).convert("RGBA")
    r, g, b, a = im.split()
    chans = [
        c.point(lambda v, k=k, o=o: max(0, min(255, round(v * k + o))))
        for c, (k, o) in zip((r, g, b), maps)
    ]
    Image.merge("RGBA", (*chans, a)).save(path)


def main():
    if sys.argv[1] == "measure":
        for p in sys.argv[2:]:
            name = p.replace("\\", "/").split("/")[-1]
            print(f"{name:24s} outline / fur / snout: {anchors(p)}")
    elif sys.argv[1] == "apply":
        maps = fit(sys.argv[2], sys.argv[3])
        print("map (gain, offset) per channel:", [(round(k, 3), round(o, 1)) for k, o in maps])
        for p in sys.argv[4:]:
            apply_map(p, maps)
            print("fixed", p)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
