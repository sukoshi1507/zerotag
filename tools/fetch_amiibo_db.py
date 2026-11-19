#!/usr/bin/env python3
"""Download the AmiiboAPI catalogue and write a compact offline cache.

usage: tools/fetch_amiibo_db.py [output.json]   (default: assets/amiibo/amiibo_db.json)

Only metadata is kept (no images, no release dates): the app shows a list on a 320x170 screen.
Data comes from https://www.amiiboapi.org, whose terms recommend caching.
The AmiiboAPI data contains no NFC dump and no keys.
"""

import json
import re
import sys
import time
import urllib.request

BASE = "https://www.amiiboapi.org/api"
USER_AGENT = "zerotag-db-fetch"
FIELDS = ("name", "character", "gameSeries", "amiiboSeries", "type")


def get_json(path, attempts=3):
    request = urllib.request.Request(BASE + path, headers={"User-Agent": USER_AGENT})
    for attempt in range(attempts):
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                return json.load(response)
        except Exception as error:  # network hiccups: retry, then give up loudly
            if attempt == attempts - 1:
                raise SystemExit(f"fetch failed for {path}: {error}")
            time.sleep(2 * (attempt + 1))


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "assets/amiibo/amiibo_db.json"

    last_updated = get_json("/lastupdated/").get("lastUpdated", "")
    raw = get_json("/amiibo/")["amiibo"]

    entries = {}
    for item in raw:
        amiibo_id = (item["head"] + item["tail"]).lower()
        if not re.fullmatch(r"[0-9a-f]{16}", amiibo_id):
            raise SystemExit(f"unexpected id: {amiibo_id!r}")
        entries[amiibo_id] = {"id": amiibo_id, **{key: item.get(key, "") for key in FIELDS}}

    if len(entries) != len(raw):
        raise SystemExit("duplicate ids in the source data")

    db = {
        "source": "https://www.amiiboapi.org",
        "lastUpdated": last_updated,
        "count": len(entries),
        "amiibo": sorted(entries.values(), key=lambda e: (e["amiiboSeries"], e["name"], e["id"])),
    }
    with open(out_path, "w", encoding="utf-8") as out:
        json.dump(db, out, ensure_ascii=False, separators=(",", ":"))
        out.write("\n")
    print(f"{len(entries)} amiibo written to {out_path} (lastUpdated {last_updated})")


if __name__ == "__main__":
    main()
