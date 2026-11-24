#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what PHP's fileinfo (libmagic of the oracle's image) says of the
# files of the upload corpus and of samples.py's synthetic files, into
# crates/hyscribe-image/tests/golden/fileinfo.tsv:
#   corpus<TAB>name<TAB>mime                 a file of the corpus
#   sample<TAB>name<TAB>hex(bytes)<TAB>mime  a synthetic file (assemble.py: the
#                                            hex of a cfb- file has runs as bb{n})
# Needs only the image (no database, no PHP container, no lock) and python3.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
IMAGE="${WORDPRESS_IMAGE:-wordpress:7.1.3-php8.4-apache}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-image/tests/golden/fileinfo.tsv"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
python3 -I "$ROOT/reference/fileinfo/samples.py" "$ROOT/crates/hyscribe-image/tests/corpus" "$TMP/files"
"$DOCKER" run --rm -v "$TMP/files:/files:ro" -v "$ROOT/reference/fileinfo/probe.php:/probe.php:ro" \
  "$IMAGE" php /probe.php /files > "$TMP/mimes"
python3 -I "$ROOT/reference/fileinfo/assemble.py" "$TMP/files" "$TMP/mimes" "$OUT"
wc -l "$OUT"
