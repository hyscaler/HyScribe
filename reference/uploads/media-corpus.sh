#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Makes the files of the upload corpus that are not images, into
# crates/hyscribe-image/tests/corpus beside the images, which
# `corpus.sh` makes (it runs this too): documents, archives, audio and
# video of the default allowed types, with ffmpeg, LibreOffice, Ghostscript
# and the usual tools. Run `corpus.sh media` for these alone.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$ROOT/crates/hyscribe-image/tests/corpus}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$OUT"
cd "$TMP"
F=(ffmpeg -hide_banner -loglevel error -y -fflags +bitexact -flags +bitexact)
SINE=(-f lavfi -i sine=frequency=440:duration=0.5:sample_rate=22050)
PICTURE=(-f lavfi -i testsrc=duration=0.5:size=160x120:rate=10)

# Text.
printf 'Plain notes.\nA second line of plain notes.\n' > "$OUT/plain.txt"
printf 'name,count\nalpha,1\nbeta,2\ngamma,3\n' > "$OUT/words.csv"
printf 'Hello, world.\nSecond line, here.\n' > "$OUT/commas.txt"
printf 'WEBVTT\n\n00:00.000 --> 00:01.000\nHello\n' > "$OUT/captions.vtt"
printf 'BEGIN:VCALENDAR\nVERSION:2.0\nEND:VCALENDAR\n' > "$OUT/event.ics"
printf '<?php echo "ran";\n' > "$OUT/script.txt"
# Files that are nothing libmagic names: sent as anything, PHP takes them as
# fileinfo's `text/plain`, `data` and one byte.
printf 'Not a picture at all.\n' > "$OUT/not-a-picture.bin"
printf '\x01\x02\x03\x04\x05\x06random bytes\xfe\xfd\xfc' > "$OUT/random.bin"
printf 'a' > "$OUT/one-byte.bin"

# Documents. LibreOffice writes the Office and OpenDocument files.
printf 'Hello world\nThis is a document.\n' > doc.txt
printf 'a;b\n1;2\n' > sheet.csv
for type in docx odt doc rtf; do
  HOME="$TMP" soffice --headless --convert-to "$type" --outdir "$TMP" doc.txt > /dev/null 2>&1
  cp "doc.$type" "$OUT/sample.$type"
done
for type in xlsx ods xls; do
  HOME="$TMP" soffice --headless --convert-to "$type" --outdir "$TMP" sheet.csv > /dev/null 2>&1
  cp "sheet.$type" "$OUT/sample.$type"
done
HOME="$TMP" soffice --headless --convert-to odp --outdir "$TMP" doc.txt > /dev/null 2>&1 || true
if [ -f doc.odp ]; then
  cp doc.odp "$OUT/sample.odp"
  HOME="$TMP" soffice --headless --convert-to pptx --outdir "$TMP" doc.odp > /dev/null 2>&1
  cp doc.pptx "$OUT/sample.pptx"
fi
# PowerPoint 97 from a one slide deck (LibreOffice cannot convert text to it).
HOME="$TMP" soffice --headless --convert-to ppt --outdir "$TMP" "$ROOT/reference/uploads/deck.fodp" > /dev/null 2>&1
cp deck.ppt "$OUT/sample.ppt"
printf '%%!PS\n/Helvetica findfont 24 scalefont setfont 72 700 moveto (Hello) show showpage\n' > hello.ps
gs -q -dNOPAUSE -dBATCH -sDEVICE=pdfwrite -sOutputFile="$OUT/sample.pdf" hello.ps
printf 'hello\n' > hello.txt
rm -f "$OUT/sample.zip" "$OUT/sample.tar" "$OUT/sample.gz"
zip -q -X "$OUT/sample.zip" hello.txt
tar --mtime=@0 --owner=0 --group=0 -cf "$OUT/sample.tar" hello.txt
gzip -n -c hello.txt > "$OUT/sample.gz"

# Audio.
"${F[@]}" "${SINE[@]}" -ac 1 -c:a libmp3lame -b:a 64k -id3v2_version 0 -write_id3v1 0 "$OUT/cbr.mp3"
"${F[@]}" "${SINE[@]}" -ac 1 -c:a libmp3lame -b:a 64k -write_xing 0 -id3v2_version 0 -write_id3v1 0 "$OUT/noxing.mp3"
"${F[@]}" "${SINE[@]}" -ac 1 -c:a libmp3lame -b:a 64k -metadata title="A Title" -metadata artist="An Artist" -metadata album="An Album" -id3v2_version 3 "$OUT/tagged.mp3"
"${F[@]}" "${SINE[@]}" -ac 1 -c:a pcm_s16le "$OUT/tone.wav"
"${F[@]}" "${SINE[@]}" -ac 1 -c:a flac "$OUT/tone.flac"
"${F[@]}" "${SINE[@]}" -ac 1 -c:a libvorbis "$OUT/tone.ogg"
"${F[@]}" "${SINE[@]}" -ac 1 -c:a aac "$OUT/tone.m4a"
"${F[@]}" "${SINE[@]}" -ac 1 -c:a aac -f adts "$OUT/tone.aac"
"${F[@]}" "${SINE[@]}" -ac 1 -c:a wmav2 "$OUT/tone.wma"
printf 'MThd\x00\x00\x00\x06\x00\x00\x00\x01\x00\x60MTrk\x00\x00\x00\x04\x00\xff\x2f\x00' > "$OUT/tune.mid"

# Video.
"${F[@]}" "${PICTURE[@]}" -c:v libx264 -pix_fmt yuv420p "$OUT/clip.mp4"
"${F[@]}" "${PICTURE[@]}" "${SINE[@]}" -ac 1 -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest "$OUT/clip-audio.mp4"
"${F[@]}" "${PICTURE[@]}" -c:v libx264 -pix_fmt yuv420p "$OUT/clip.mov"
"${F[@]}" "${PICTURE[@]}" -c:v mpeg4 "$OUT/clip.avi"
"${F[@]}" "${PICTURE[@]}" -c:v libvpx "$OUT/clip.webm"
"${F[@]}" "${PICTURE[@]}" -c:v libx264 -pix_fmt yuv420p "$OUT/clip.mkv"
"${F[@]}" "${PICTURE[@]}" -c:v libx264 -pix_fmt yuv420p "$OUT/clip.3gp"
"${F[@]}" "${PICTURE[@]}" -c:v wmv2 "$OUT/clip.wmv"
"${F[@]}" "${PICTURE[@]}" -c:v flv1 "$OUT/clip.flv"
"${F[@]}" -f lavfi -i testsrc=duration=0.5:size=320x240:rate=25 -c:v mpeg1video -f mpeg "$OUT/clip.mpg"
ls -l "$OUT"
