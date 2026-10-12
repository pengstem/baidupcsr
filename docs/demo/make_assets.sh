#!/usr/bin/env bash
# Generate the demo wallpapers served by fake_pcs.py (needs ffmpeg and ImageMagick).
set -euo pipefail
out="$(dirname "$0")/assets"
mkdir -p "$out"
cd "$out"
ffmpeg -loglevel error -y -f lavfi -i "mandelbrot=s=1280x800:start_scale=1.2:end_scale=1.2:inner=period" \
  -frames:v 1 -q:v 3 "曼德博集合.jpg"
magick -seed 7 -size 1280x800 plasma:'#04122e'-'#1fe0a0' -blur 0x10 -modulate 100,140 -quality 88 "极光.jpg"
magick -seed 3 -size 1280x800 plasma:fractal -blur 0x3 -auto-level -modulate 90,160,70 -quality 88 "星云.jpg"
magick -seed 11 -size 1280x800 plasma:fractal -auto-level -quality 88 "等离子.jpg"
magick -size 1280x800 gradient:'#ff9a5a'-'#2b1055' -quality 88 "渐变·黄昏.jpg"
magick -seed 5 -size 1280x800 plasma:'#000010'-'#5a3bd6' -swirl 300 -blur 0x2 -quality 88 galaxy.jpg
