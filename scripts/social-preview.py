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
NAME, MUTED = (240, 242, 245), (150, 156, 166)
# The captions window and the reply window: the left column of overview.png.
WINDOWS = (16, 16, 1178, 1338)


def main() -> None:
    canvas = Image.new("RGB", (W, H), BACKGROUND)
    draw = ImageDraw.Draw(canvas)

    shot = Image.open(ROOT / "docs/images/overview.png").convert("RGB").crop(WINDOWS)
    height = H - 2 * 40
    shot = shot.resize((round(shot.width * height / shot.height), height), Image.LANCZOS)
    left = W - 56 - shot.width
    canvas.paste(shot, (left, 40))

    icon = Image.open(ROOT / "src-tauri/icons/128x128@2x.png").convert("RGBA").resize((104, 104), Image.LANCZOS)
    canvas.paste(icon, (64, 196), icon)
    name = ImageFont.truetype("segoeuib.ttf", 76)
    draw.text((64, 324), "OwnVoice Meet", font=name, fill=NAME)
    label = ImageFont.truetype("msjh.ttc", 24)
    draw.text((66, 432), "畫面：程式的 Demo 模式（示範文字）", font=label, fill=MUTED)

    canvas.save(OUT, optimize=True)
    print(f"{OUT}: {canvas.width}x{canvas.height}, {OUT.stat().st_size / 1024:.0f} KB; name block ends at x="
          f"{64 + draw.textlength('OwnVoice Meet', font=name):.0f}, screenshot starts at x={left}")


if __name__ == "__main__":
    main()
