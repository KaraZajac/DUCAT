#!/usr/bin/env python3
"""Generate the desk's platform icons from the phone's own artwork.

The outputs are committed beside this script: a build must not need Pillow,
and an icon nobody can regenerate is an icon that drifts from the app it
names. Run it after the artwork changes:

    python3 applications/desktop/icons/make-icons.py

Design: the same thing the phone shows on a home screen — the maneki-neko on
the Catppuccin base tile (#1E1E2E, the app's own ic_launcher_background), so
the desk and the phone are visibly one product. The cat alone, cut out on
transparency, disappears into a dark dock; the tile carries it anywhere.

macOS gets Apple's padding convention (the tile inset in a larger canvas);
Windows and Linux fill their canvas, because a taskbar is already small
enough without spending pixels on air.
"""
import struct
from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
RES = HERE.parents[1] / "android/src/main/res"
SOURCE = RES / "drawable-nodpi/ic_splash.png"   # 512², the largest cat we have
BASE = (30, 30, 46, 255)                        # #1E1E2E, ic_launcher_background


def tile(size: int, inset: float = 0.0) -> Image.Image:
    """The icon at `size`, the tile inset by `inset` of the canvas each side."""
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    pad = round(size * inset)
    box = size - 2 * pad
    # Rounded square, radius ≈ Apple's squircle. Drawn at 4× and downsampled:
    # PIL's rounded_rectangle aliases badly at icon sizes otherwise.
    ss = 4
    plate = Image.new("RGBA", (box * ss, box * ss), (0, 0, 0, 0))
    ImageDraw.Draw(plate).rounded_rectangle(
        (0, 0, box * ss - 1, box * ss - 1), radius=round(box * ss * 0.2237), fill=BASE,
    )
    plate = plate.resize((box, box), Image.LANCZOS)
    canvas.paste(plate, (pad, pad), plate)

    cat = Image.open(SOURCE).convert("RGBA")
    cat = cat.crop(cat.getchannel("A").getbbox())
    # Fill ~78% of the tile's width, sitting a touch high — the cat's mass is
    # low (it is sitting down), so optical centre is above geometric centre.
    w = round(box * 0.78)
    h = round(cat.height * w / cat.width)
    if h > box * 0.86:
        h = round(box * 0.86)
        w = round(cat.width * h / cat.height)
    cat = cat.resize((w, h), Image.LANCZOS)
    canvas.paste(cat, (pad + (box - w) // 2, pad + round((box - h) * 0.44)), cat)
    return canvas


def write_icns(path: Path, art) -> None:
    """An .icns without macOS tooling: PNG payloads in the modern chunk types.

    Every type here (ic07 and up) takes a PNG verbatim, which is why this can
    be built on Linux at all — the old is32/il32/s8mk masks would need the
    platform's own encoders.
    """
    types = {
        b"ic07": 128, b"ic08": 256, b"ic09": 512, b"ic10": 1024,
        b"ic11": 32, b"ic12": 64, b"ic13": 256, b"ic14": 512,
    }
    chunks = b""
    for tag, px in types.items():
        buf = __import__("io").BytesIO()
        art(px).save(buf, format="PNG")
        data = buf.getvalue()
        chunks += tag + struct.pack(">I", len(data) + 8) + data
    path.write_bytes(b"icns" + struct.pack(">I", len(chunks) + 8) + chunks)


def main() -> None:
    # macOS: inset, per Apple's grid — a full-bleed tile looks oversized
    # beside every other icon in the Dock.
    write_icns(HERE / "ducat.icns", lambda px: tile(px, inset=0.09))

    # Windows: every size the shell asks for, in one file.
    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    tile(256).save(
        HERE / "ducat.ico", format="ICO",
        sizes=[(s, s) for s in ico_sizes],
    )

    # Linux packaging, and the app's own window/tray icon.
    tile(512).save(HERE / "ducat.png")
    res = HERE.parent / "src/main/resources"
    res.mkdir(parents=True, exist_ok=True)
    tile(256).save(res / "desk-icon.png")
    # The tray lives at 16–24 px on most panels: a separate, tighter crop, so
    # the cat is not a smudge inside a margin.
    tile(64, inset=0.0).save(res / "desk-tray.png")
    print("wrote ducat.icns, ducat.ico, ducat.png, desk-icon.png, desk-tray.png")


if __name__ == "__main__":
    main()
