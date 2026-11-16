#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Makes the upload corpus, crates/hyscribe-image/tests/corpus, with
# ImageMagick (`convert`, with WebP and AVIF) and PHP (exif.php). The files
# are committed: run it only to change the corpus, then record the goldens
# again (golden.sh, header-golden.php).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-image/tests/corpus"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
rm -rf "$OUT"
mkdir -p "$OUT"
cd "$TMP"
# No creation dates, so the PNGs come out the same each run.
NODATE=(-define png:exclude-chunk=date,time,tEXt,zTXt)

# A textured picture: a gradient, a plasma and shapes, so resizing shows.
picture() { # <size> <out>
  convert -seed 7 -size "$1" gradient:'#c04020-#2060c0' \
    \( -size "$1" plasma:fractal -blur 0x1 \) -compose overlay -composite \
    -fill '#ffffff' -draw 'circle 60,60 90,90' -fill '#103010' -draw 'rectangle 10,100 70,160' \
    -strip "$2"
}
exif() { php "$ROOT/reference/uploads/exif.php" "$1" "$OUT/$2" "$3"; }

picture 640x480 photo.ppm
convert photo.ppm -quality 90 photo.jpg
exif photo.jpg photo.jpg '{"exif": {"ImageDescription": "A short description", "Make": "Maker", "Model": "Camera Model ", "Orientation": 1, "Artist": "The Artist", "Copyright": "Copyright Holder", "ExposureTime": "1/250", "FNumber": "28/10", "ISOSpeedRatings": 200, "DateTimeDigitized": "2024:05:06 07:08:09", "FocalLength": "50/1", "UserComment": "A user comment"}, "iptc": {"2#005": ["Object name"], "2#025": ["one", "two", "three"], "2#080": ["By line"], "2#105": ["The headline"], "2#110": ["The credit"], "2#116": ["IPTC copyright"], "2#120": ["The caption"]}}'
exif photo.jpg iptc.jpg '{"iptc": {"2#120": ["A caption long enough that WordPress does not take it as the title of the image"], "2#080": ["Byline only"], "2#055": ["20240506"], "2#060": ["070809"]}}'
exif photo.jpg described.jpg '{"exif": {"ImageDescription": "Description and comment", "UserComment": "Description and comment", "FocalLength": "35/10", "ExposureTime": "1/3", "FNumber": "0/0"}}'
exif photo.jpg commented.jpg '{"exif": {"UserComment": "Only a comment", "Artist": "  Spaced artist  "}}'
exif photo.jpg combined.jpg '{"exif": {"ImageDescription": "Desc", "UserComment": "Comment"}}'
exif photo.jpg headline-both.jpg '{"exif": {"ImageDescription": "Desc", "UserComment": "Comment"}, "iptc": {"2#105": ["Headline"]}}'
exif photo.jpg headline-same.jpg '{"exif": {"ImageDescription": "Same", "UserComment": "Same"}, "iptc": {"2#105": ["Headline"]}}'
exif photo.jpg desc-only.jpg '{"exif": {"ImageDescription": "A description of eighty characters or more, which WordPress keeps as caption only", "Copyright": "Exif copyright"}}'
exif photo.jpg object-name.jpg '{"iptc": {"2#005": ["Object name only"], "2#120": ["hex:436166e9"], "2#025": ["hex:e96c6c65", "two"]}}'
exif photo.jpg undefined-comment.jpg '{"exif": {"UndefinedComment": "Undefined comment  ", "ISOSpeedRatings": 0}}'
exif photo.jpg orient-9.jpg '{"exif": {"Orientation": 9}}'
for o in 1 2 3 4 5 6 7 8; do
  picture 320x200 o.ppm
  convert o.ppm -quality 85 o.jpg
  exif o.jpg "orient-$o.jpg" "{\"exif\": {\"Orientation\": $o}}"
done
picture 2600x260 big.ppm
convert big.ppm -quality 85 big.jpg
cp big.jpg "$OUT/big.jpg"
exif big.jpg big-orient-6.jpg '{"exif": {"Orientation": 6}}'
picture 260x2700 tall.ppm
convert tall.ppm -quality 85 "$OUT/tall.jpg"
picture 1200x400 wide.ppm
convert wide.ppm -quality 85 "$OUT/wide.jpg"
convert wide.ppm -resize 120x90! -quality 85 "$OUT/small.jpg"
convert photo.ppm -resize 400x300! -colorspace CMYK -quality 85 "$OUT/cmyk.jpg"
convert photo.ppm -resize 400x300! -colorspace Gray -quality 85 "$OUT/gray.jpg"
convert photo.ppm -resize 400x300! -interlace Plane -quality 85 "$OUT/progressive.jpg"
convert photo.ppm -resize 400x300! -sampling-factor 1x1 -quality 95 "$OUT/444.jpg"

convert photo.ppm -resize 400x300! "${NODATE[@]}" "PNG24:$OUT/rgb.png"
convert photo.ppm -resize 400x300! \( -size 400x300 gradient:white-black \) -alpha off -compose copy-opacity -composite "${NODATE[@]}" "PNG32:$OUT/rgba.png"
convert photo.ppm -resize 400x300! -colors 64 "${NODATE[@]}" "PNG8:$OUT/indexed.png"
convert "$OUT/rgba.png" -colors 64 "${NODATE[@]}" "PNG8:$OUT/indexed-alpha.png"
convert photo.ppm -resize 200x150! -depth 16 "${NODATE[@]}" "PNG48:$OUT/rgb16.png"
convert photo.ppm -resize 400x300! -colorspace Gray "${NODATE[@]}" "PNG:$OUT/gray.png"
convert photo.ppm -resize 1100x800! -quality 95 "${NODATE[@]}" "PNG8:$OUT/large.png"

convert photo.ppm -resize 400x300! -colors 128 "$OUT/still.gif"
convert photo.ppm -resize 320x240! -colors 64 \( +clone -rotate 180 \) \( +clone -flop \) -set delay 20 -loop 0 "$OUT/anim.gif"

convert photo.ppm -resize 400x300! -quality 80 "$OUT/lossy.webp"
convert photo.ppm -resize 400x300! -define webp:lossless=true "$OUT/lossless.webp"
convert "$OUT/rgba.png" -quality 80 "$OUT/alpha.webp"
convert photo.ppm -resize 320x240! \( +clone -rotate 180 \) -set delay 20 -loop 0 "$OUT/anim.webp"
convert photo.ppm -resize 400x300! -quality 60 "$OUT/photo.avif"
convert photo.ppm -resize 160x120! "$OUT/photo.bmp"
convert photo.ppm -resize 80x60! "$OUT/photo.tif"
# Over the oracle's ImageMagick area limit by its header alone.
php -r 'echo "\x89PNG\r\n\x1a\n" . pack("N", 13) . "IHDR" . pack("NNCCCCC", 40000, 40000, 8, 2, 0, 0, 0) . "\0\0\0\0" . pack("N", 0) . "IEND" . "\xAE\x42\x60\x82";' > "$OUT/huge.png"
ls "$OUT"
