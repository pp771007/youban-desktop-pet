"""Make pets/<pet>/thumb.png: the standing pose, trimmed and shrunk, shown when picking a pet.

The installer only ships the default pet's frames; every other pet ships pet.json + thumb.png and
downloads its frames when first picked (crates/petkit/src/pet_download.rs), so each pet needs one.

usage: python thumb.py <pets/<pet> folder>...
"""
import sys
from pathlib import Path

from PIL import Image

# The settings page draws pet pictures at 112 CSS px; twice that stays sharp at 200% scaling.
MAX_SIDE = 224
SOURCE = "frames/idle_01.png"
OUT = "thumb.png"
# Near-invisible haze AI images leave around the character would make the trim box a full square.
ALPHA_FLOOR = 8


def make(pet: Path) -> None:
    im = Image.open(pet / SOURCE).convert("RGBA")
    mask = im.getchannel("A").point(lambda a: 255 if a > ALPHA_FLOOR else 0)
    im = im.crop(mask.getbbox())
    scale = MAX_SIDE / max(im.size)
    if scale < 1:
        size = (max(1, round(im.width * scale)), max(1, round(im.height * scale)))
        # Resize premultiplied so the transparent edge does not pick up dark fringes.
        im = im.convert("RGBa").resize(size, Image.LANCZOS).convert("RGBA")
    im.save(pet / OUT, optimize=True)
    print(f"{pet / OUT} {im.size[0]}x{im.size[1]}")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    for arg in sys.argv[1:]:
        make(Path(arg))
