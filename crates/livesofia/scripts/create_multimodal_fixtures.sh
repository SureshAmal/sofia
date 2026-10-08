#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
mkdir -p testdata/generated

magick -size 320x240 xc:white \
  -fill red -draw 'circle 85,120 85,75' \
  -fill blue -draw 'rectangle 205,75 285,155' \
  testdata/generated/still.png

for position in 65 160 255; do
  index=$(( (position - 65) / 95 + 1 ))
  magick -size 320x240 xc:white \
    -fill red -draw "circle ${position},120 ${position},85" \
    "testdata/generated/frame_$(printf '%02d' "$index").png"
done

ffmpeg -hide_banner -loglevel error -y -framerate 1 \
  -i testdata/generated/frame_%02d.png -c:v mpeg4 -q:v 5 -pix_fmt yuv420p \
  testdata/generated/moving_circle.mp4

echo 'Created testdata/generated/still.png and moving_circle.mp4'
