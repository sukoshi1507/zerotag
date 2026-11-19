#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.9"
# dependencies = ["pillow"]
# ///
"""Download AmiiboAPI thumbnails and shrink them for the 320x170 screen.

usage: tools/fetch_amiibo_images.py [--limit N] [--box WxH] [--out DIR] [--workers N]

Reads assets/amiibo/amiibo_db.json, downloads the WebP image of each amiibo from the AmiiboAPI image
repository (https://github.com/8bitDream/AmiiboAPI) and writes <out>/<id>.png, scaled to fit inside the box
with the aspect ratio kept. Existing files are skipped, so the script can be re-run to resume.

The images are photos of the physical amiibo from the AmiiboAPI image repository. The generated files are
committed under assets/amiibo/images/; re-run this script to refresh them.
"""

import argparse
import concurrent.futures
import io
import json
import sys
import time
import urllib.request
from pathlib import Path

from PIL import Image

BASE = "https://raw.githubusercontent.com/8bitDream/AmiiboAPI/master/webp/"
USER_AGENT = "zerotag-image-fetch"
ROOT = Path(__file__).resolve().parent.parent


def source_url(amiibo_id):
    return f"{BASE}icon_{amiibo_id[:8]}-{amiibo_id[8:]}.webp"


def download(url, attempts=4):
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    for attempt in range(attempts):
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                return response.read()
        except Exception:
            if attempt == attempts - 1:
                raise
            time.sleep(2 * (attempt + 1))


def fetch_one(amiibo_id, out_dir, box):
    target = out_dir / f"{amiibo_id}.png"
    if target.exists():
        return "skipped"
    with Image.open(io.BytesIO(download(source_url(amiibo_id)))) as image:
        image = image.convert("RGBA")
        image.thumbnail(box, Image.LANCZOS)  # keeps the aspect ratio, never upscales
        partial = target.with_suffix(".tmp")
        image.save(partial, format="PNG", optimize=True)
        partial.replace(target)
    return "downloaded"


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--db", default=str(ROOT / "assets/amiibo/amiibo_db.json"))
    parser.add_argument("--out", default=str(ROOT / "assets/amiibo/images"))
    parser.add_argument("--box", default="120x88", help="maximum size WxH (default 120x88)")
    parser.add_argument("--limit", type=int, default=0, help="only process the first N amiibo")
    parser.add_argument("--workers", type=int, default=4)
    args = parser.parse_args()

    width, height = (int(v) for v in args.box.lower().split("x"))
    ids = [entry["id"] for entry in json.load(open(args.db, encoding="utf-8"))["amiibo"]]
    if args.limit:
        ids = ids[: args.limit]
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    counts = {"downloaded": 0, "skipped": 0, "failed": 0}
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, min(args.workers, 8))) as pool:
        futures = {pool.submit(fetch_one, i, out_dir, (width, height)): i for i in ids}
        for number, future in enumerate(concurrent.futures.as_completed(futures), 1):
            try:
                counts[future.result()] += 1
            except Exception as error:
                counts["failed"] += 1
                print(f"failed {futures[future]}: {error}", file=sys.stderr)
            if number % 100 == 0 or number == len(ids):
                print(f"{number}/{len(ids)}  {counts}", flush=True)

    print(f"done: {counts}")
    return 1 if counts["failed"] else 0


if __name__ == "__main__":
    sys.exit(main())
