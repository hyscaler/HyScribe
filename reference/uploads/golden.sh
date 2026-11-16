#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what PHP WordPress makes of each upload of the corpus
# (crates/hyscribe-image/tests/corpus, from corpus.sh): the metadata of
# wp_read_image_metadata() and wp_create_image_subsizes(), the files the
# editor writes, and the type checks of uploads named otherwise, into
# crates/hyscribe-image/tests/golden/uploads.tsv, with the files PHP made
# under crates/hyscribe-image/tests/golden/uploads/. Writes nothing to the
# database (attachment ID 0); needs reference/default-install/php.sh up.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
GOLDEN="$ROOT/crates/hyscribe-image/tests/golden"
DIR=/tmp/hyscribe-uploads
cleanup() { "$DOCKER" exec "$NAME" rm -rf "$DIR"; }
trap cleanup EXIT
"$DOCKER" exec "$NAME" mkdir -p "$DIR/corpus" "$DIR/uploads/2026/11"
"$DOCKER" cp "$ROOT/crates/hyscribe-image/tests/corpus/." "$NAME:$DIR/corpus/"
"$DOCKER" exec -i "$NAME" sh -c "cat > $DIR/golden.php" < "$ROOT/reference/uploads/golden.php"
"$DOCKER" exec "$NAME" chown -R www-data "$DIR"
"$DOCKER" exec -u www-data "$NAME" php "$DIR/golden.php" "$DIR/corpus" "$DIR/uploads" > "$GOLDEN/uploads.tsv"
rm -rf "$GOLDEN/uploads"
mkdir -p "$GOLDEN/uploads"
for made in $(awk -F'\t' '$1 == "out" { print $3 }' "$GOLDEN/uploads.tsv"); do
  "$DOCKER" cp "$NAME:$DIR/uploads/2026/11/$made" "$GOLDEN/uploads/$made"
done
echo "uploads.tsv: $(wc -l < "$GOLDEN/uploads.tsv") lines, $(ls "$GOLDEN/uploads" | wc -l) files"
