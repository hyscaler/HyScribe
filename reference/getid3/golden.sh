#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what getID3 makes of the audio and video of the upload corpus and
# of samples.py's synthetic files, as wp_read_audio_metadata() and
# wp_read_video_metadata() keep it, into
# crates/hyscribe-image/tests/golden/getid3.tsv:
#   corpus<TAB>name<TAB>hex(serialize(metadata))
#   sample<TAB>name<TAB>hex(bytes)<TAB>hex(serialize(metadata))
# The metadata is `b:0;` for a file PHP cannot read. Needs php.sh up over a
# fresh db.sh up (WordPress loads); writes nothing to the database.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-image/tests/golden/getid3.tsv"
TMP="$(mktemp -d)"
DIR=/tmp/hyscribe-getid3
cleanup() { rm -rf "$TMP"; "$DOCKER" exec "$NAME" rm -rf "$DIR"; }
trap cleanup EXIT
python3 -I "$ROOT/reference/getid3/samples.py" "$ROOT/crates/hyscribe-image/tests/corpus" "$TMP/files"
"$DOCKER" exec "$NAME" mkdir -p "$DIR"
"$DOCKER" cp "$TMP/files" "$NAME:$DIR/files"
"$DOCKER" exec -i "$NAME" sh -c "cat > $DIR/golden.php" < "$ROOT/reference/getid3/golden.php"
"$DOCKER" exec "$NAME" chown -R www-data "$DIR"
"$DOCKER" exec -u www-data "$NAME" php "$DIR/golden.php" "$DIR/files" > "$TMP/serialized"
: > "$OUT"
while IFS=$'\t' read -r name serialized; do
  case "$name" in
    corpus-*) printf 'corpus\t%s\t%s\n' "${name#corpus-}" "$serialized" >> "$OUT" ;;
    *) printf 'sample\t%s\t%s\t%s\n' "$name" "$(xxd -p "$TMP/files/$name" | tr -d '\n')" "$serialized" >> "$OUT" ;;
  esac
done < "$TMP/serialized"
wc -l "$OUT"
