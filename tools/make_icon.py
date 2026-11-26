#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Renders the ZeroTag icon (ZeroStack family logo, assets/logo/zerotag.svg) with rsvg-convert.

usage: make_icon.py [<output_dir>] [<store_icon_dir>]
  output_dir      receives zerotag.png, zerotag_100.png (100 px) and zerotag_80.png (default: assets/images)
  store_icon_dir  receives the 260 px store icon (default: packaging (icon.png))
"""
import os
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
SVG = os.path.join(ROOT, "assets", "logo", "zerotag.svg")
out = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else os.path.join(ROOT, "assets", "images")
store = os.path.abspath(sys.argv[2]) if len(sys.argv) > 2 else os.path.join(ROOT, "packaging")


def render(size, path):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), SVG, "-o", path], check=True)


render(100, os.path.join(out, "zerotag.png"))
render(100, os.path.join(out, "zerotag_100.png"))
render(80, os.path.join(out, "zerotag_80.png"))
render(260, os.path.join(store, "icon.png"))
print("icons written to", out, "and", store)
