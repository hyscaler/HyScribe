#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records whole pages for each request of reference/pages/<list>.list
# (default or ttd) as crates/hyscribe-http/tests/golden/pages-<list>.tsv:
#   options<TAB>hex(path and query)<TAB>status<TAB>location<TAB>body
# where options are the list's (reference/pages/options.php, installed as a
# must-use plugin for the run and removed after) and the body is hex, or
# over the Theme Test Data, whose content is not ours to commit, the
# SHA-256 of the page without its RSD link, with the page itself in
# target/pages/ttd.tsv for reading differences.
#   default: reference/default-install/php.sh up
#   ttd:     HYSCRIBE_PHP_DB=wordpress_ttd reference/default-install/php.sh up
# Page views write: the wp_navigation post php.sh up wrote is
# deleted first, so start from a fresh db.sh up and reset it after.
set -euo pipefail

LIST="${1:?usage: $0 default|ttd}"
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
DATABASE=wordpress
[ "$LIST" = ttd ] && DATABASE=wordpress_ttd
BASE="${HYSCRIBE_PHP_BASE:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/pages-$LIST.tsv"
RAW="$ROOT/target/pages/$LIST.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
HEADERS=$(mktemp)
cleanup() {
  rm -f "$TMP" "$HEADERS"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-options.php"
}
trap cleanup EXIT
"$DOCKER" exec "$NAME" mkdir -p "$MU"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-options.php" < "$ROOT/reference/pages/options.php"
"$DOCKER" exec "$DB" mariadb -uroot -preference-only "$DATABASE" -e "DELETE FROM wp_posts WHERE post_type = 'wp_navigation'"
# The first page view writes caches the later ones read.
curl -s -o /dev/null "$BASE/"
mkdir -p "$(dirname "$RAW")"
: > "$OUT"
: > "$RAW"
while IFS= read -r line; do
  # Not by `read`, which drops a leading tab, and so empty options.
  options=${line%%$'\t'*}
  path=${line#*$'\t'}
  curl -g -s -D "$HEADERS" -o "$TMP" -H "X-HyScribe-Options: $options" "$BASE$path" > /dev/null
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  location=$(grep -i '^location:' "$HEADERS" | cut -d ' ' -f 2 | tr -d '\r' || true)
  key=$(printf '%s' "$path" | xxd -p | tr -d '\n')
  if [ "$LIST" = ttd ]; then
    page=$(python3 -I -c 'import re, sys; sys.stdout.write(re.sub(rb"<link rel=\"EditURI\"[^\n]*?/>\n", b"", open(sys.argv[1], "rb").read()).hex())' "$TMP")
    printf '%s\t%s\t%s\t%s\t%s\n' "$options" "$key" "$status" "$location" "$page" >> "$RAW"
    body=$(printf '%s' "$page" | xxd -r -p | sha256sum | cut -d ' ' -f 1)
  else
    body=$(xxd -p "$TMP" | tr -d '\n')
  fi
  printf '%s\t%s\t%s\t%s\t%s\n' "$options" "$key" "$status" "$location" "$body" >> "$OUT"
done < "$ROOT/reference/pages/$LIST.list"
wc -l "$OUT"
