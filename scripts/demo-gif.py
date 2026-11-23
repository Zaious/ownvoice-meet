"""Cut the README GIF (docs/media/demo.gif) from the recorded MP4 (docs/media/demo.mp4).

The GIF starts on a frame that already has a caption on screen, so a reader who sees only the first
frame (autoplay off, reduced motion, a preview) still sees the app at work; it runs to the end of the
MP4, where the English to read aloud stays on screen. The MP4 stays the full recording.

    py -3 scripts/demo-gif.py --start 5.133

--start is the second in the MP4 where the GIF begins: for the current recording, the first question
with its Chinese meaning under it. Pick it by looking at the frames after a new recording
(scripts/record-demo.ps1). Needs ffmpeg on PATH and Pillow.

Two changes on top of the recording, both said in the README caption, each with a switch to leave it
out:
- the Script window on the right is darkened (DIM), so the eye goes first to the captions and the
  reply on the left, which carry the story; the recording itself is not changed (--no-dim);
- a strip under the recording says it is a demo with text-to-speech questions, because the picture
  can be shared without its caption (--no-label).

Video compression changes pixels slightly in every frame. Each 16x16 block is copied from the new
frame only when something in it really changed, so the still parts of the picture stay identical and
the GIF only stores what moves.
"""
import argparse
import pathlib
import subprocess
import sys
import tempfile

from PIL import Image, ImageChops, ImageDraw, ImageFont

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Where the Script window sits in demo.mp4 (1280x788), and how much it is darkened.
SCRIPT_WINDOW = (698, 11, 1268, 776)
DIM = 0.55
# The strip under the recording: the background colour around the windows, neutral light text.
LABEL = "示範 DEMO · 兩個提問是電腦語音唸的，不是真實會議 · The two questions are text-to-speech, not a real meeting"
LABEL_HEIGHT, LABEL_FONT, LABEL_SIZE = 40, "msjh.ttc", 20
BACKGROUND, LABEL_TEXT = (6, 8, 16), (210, 210, 210)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--start", type=float, required=True, help="second in the MP4 where the GIF starts")
    ap.add_argument("--fps", type=int, default=10)
    ap.add_argument("--mp4", default=str(ROOT / "docs/media/demo.mp4"))
    ap.add_argument("--out", default=str(ROOT / "docs/media/demo.gif"))
    ap.add_argument("--no-dim", action="store_true", help="leave the Script window as recorded")
    ap.add_argument("--no-label", action="store_true", help="no demo strip under the recording")
    args = ap.parse_args()

    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(
            ["ffmpeg", "-v", "error", "-i", args.mp4, "-vf",
             f"trim=start={args.start},setpts=PTS-STARTPTS,fps={args.fps}",
             f"{tmp}/f%04d.png"],
            check=True,
        )
        paths = sorted(pathlib.Path(tmp).glob("f*.png"))
        frames = [Image.open(p).convert("RGB") for p in paths]

    if not args.no_dim:
        for frame in frames:
            frame.paste(frame.crop(SCRIPT_WINDOW).point(lambda v: round(v * DIM)), SCRIPT_WINDOW[:2])
    if not args.no_label:
        font = ImageFont.truetype(LABEL_FONT, LABEL_SIZE)
        w, h = frames[0].size
        strip = Image.new("RGB", (w, LABEL_HEIGHT), BACKGROUND)
        draw = ImageDraw.Draw(strip)
        draw.text((w / 2, LABEL_HEIGHT / 2), LABEL, font=font, fill=LABEL_TEXT, anchor="mm")
        labelled = []
        for frame in frames:
            canvas = Image.new("RGB", (w, h + LABEL_HEIGHT), BACKGROUND)
            canvas.paste(frame, (0, 0))
            canvas.paste(strip, (0, h))
            labelled.append(canvas)
        frames = labelled

    # Keep still blocks identical from frame to frame. A block changed when one of its pixels moved by
    # more than `strong`, or at least `pixels` of them by more than `weak`: compression noise in
    # demo.mp4 moves a few pixels by up to about 60, while text that appears (even a small grey 。)
    # moves some pixels by more than 80. A changed block is copied from the new frame together with the
    # blocks around it, so the edges of a word that crosses into the next block come along.
    block, weak, pixels, strong = 16, 48, 8, 80
    clean = [frames[0]]
    for frame in frames[1:]:
        prev = clean[-1]
        out = prev.copy()
        diff = ImageChops.difference(frame, prev).convert("L")
        w, h = frame.size
        changed = set()
        for y in range(0, h, block):
            for x in range(0, w, block):
                hist = diff.crop((x, y, x + block, y + block)).histogram()
                if sum(hist[strong + 1:]) or sum(hist[weak + 1:]) >= pixels:
                    changed.add((x, y))
        around = {(x + dx * block, y + dy * block) for x, y in changed
                  for dx in (-1, 0, 1) for dy in (-1, 0, 1)}
        for x, y in around:
            if 0 <= x < w and 0 <= y < h:
                box = (x, y, min(x + block, w), min(y + block, h))
                out.paste(frame.crop(box), box)
        clean.append(out)

    # One palette for all frames, taken from the first and the last frame (the last one has every
    # colour that appears: both questions, their translations, the reply).
    w, h = clean[0].size
    sample = Image.new("RGB", (w, h * 2))
    sample.paste(clean[0], (0, 0))
    sample.paste(clean[-1], (0, h))
    palette = sample.quantize(colors=256, method=Image.Quantize.MEDIANCUT)
    indexed = [f.quantize(palette=palette, dither=Image.Dither.NONE) for f in clean]

    # Nothing the video shows may be lost on the way: every GIF frame is compared with its video frame,
    # and a pixel more than `strong` away from it (a missing character, a stale word) stops the script.
    worst = max(
        sum(ImageChops.difference(f.convert("RGB"), src).convert("L").histogram()[strong + 1:])
        for f, src in zip(indexed, frames)
    )
    if worst:
        print(f"a GIF frame differs from its video frame in {worst} pixels by more than {strong}")
        return 1

    delay = round(1000 / args.fps)
    indexed[0].save(args.out, save_all=True, append_images=indexed[1:], duration=delay, loop=0,
                    disposal=1, optimize=False)
    print(f"{args.out}: {len(indexed)} frames at {args.fps} fps, {len(indexed) * delay / 1000:.2f} s, "
          f"{pathlib.Path(args.out).stat().st_size / 1024:.0f} KB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
