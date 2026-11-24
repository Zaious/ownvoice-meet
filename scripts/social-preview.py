"""Make the GitHub social preview (docs/media/social-preview.png, 1280x640) from the repository's own
images: the app icon, and the captions and reply windows from docs/images/overview.png, a capture of
the app in Demo mode. It carries the name, large enough to read at a third of its size, and says the
picture is Demo mode; no tagline, because the share card shows the description next to it.

    py -3 scripts/social-preview.py

Upload it by hand: the repository's Settings, General, Social preview (GitHub has no API for it).
Needs Pillow and the Segoe UI and Microsoft JhengHei fonts (Windows).
"""
import pathlib

from PIL import Image, ImageDraw, ImageFont

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "docs/media/social-preview.png"
W, H = 1280, 640
BACKGROUND = (11, 13, 16)  # the background around the windows in overview.png, so the capture blends in
NAME, MUTED = (240, 242, 245), (190, 196, 206)
PANEL = (20, 22, 26)  # the windows' background in overview.png
# The captions window and the reply window: the left column of overview.png.
WINDOWS = (16, 16, 1178, 1338)


def main() -> None:
    canvas = Image.new("RGB", (W, H), BACKGROUND)
    draw = ImageDraw.Draw(canvas)

    shot = Image.open(ROOT / "docs/images/overview.png").convert("RGB").crop(WINDOWS)
    # In the capture each window has an 11 px pure black frame on its left, right and bottom, inside
    # the width of its title bar. Paint it in the windows' own panel colour, so the panel meets the
    # title bar's edges instead of a black border. Nothing else in the capture is this dark.
    shot = Image.eval(shot, lambda v: v)  # copy
    px = shot.load()
    for y in range(shot.height):
        for x in range(shot.width):
            if max(px[x, y]) <= 6:
                px[x, y] = PANEL
    height = H - 2 * 40
    shot = shot.resize((round(shot.width * height / shot.height), height), Image.LANCZOS)
    left = W - 56 - shot.width
    canvas.paste(shot, (left, 40))

    icon = Image.open(ROOT / "src-tauri/icons/128x128@2x.png").convert("RGBA").resize((104, 104), Image.LANCZOS)
    canvas.paste(icon, (64, 196), icon)
    name = ImageFont.truetype("segoeuib.ttf", 76)
    draw.text((64, 324), "OwnVoice Meet", font=name, fill=NAME)
    label = ImageFont.truetype("msjh.ttc", 30)
    draw.text((66, 432), "畫面內容為 Demo 模式的示範文字 · DEMO", font=label, fill=MUTED)

    canvas.save(OUT, optimize=True)
    print(f"{OUT}: {canvas.width}x{canvas.height}, {OUT.stat().st_size / 1024:.0f} KB; name block ends at x="
          f"{64 + draw.textlength('OwnVoice Meet', font=name):.0f}, screenshot starts at x={left}")


if __name__ == "__main__":
    main()
