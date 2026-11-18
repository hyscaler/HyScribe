#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the pages PHP answers for attachments, by ID and by name, with
# attachment pages off and on, over the default install with
# reference/attachment-edit/content.sql and reference/attachment-page/extra.sql,
# which it adds and deletes after, as
#   crates/hyscribe-http/tests/golden/attachment-pages.tsv:
#   options<TAB>hex(path and query)<TAB>status<TAB>location<TAB>hex(body)
# Options are reference/pages/options.php's. Page views write: the
# wp_navigation post php.sh up wrote is deleted first, so start from a fresh
# db.sh up with php.sh up, and take both down after.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_BASE:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/attachment-pages.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
HEADERS=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$HEADERS"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-options.php"
  echo "DELETE FROM wp_users WHERE ID IN (2, 3); DELETE FROM wp_usermeta WHERE umeta_id BETWEEN 801 AND 806;
    DELETE FROM wp_posts WHERE ID BETWEEN 801 AND 831 OR ID BETWEEN 9001 AND 9007;
    DELETE FROM wp_postmeta WHERE meta_id BETWEEN 801 AND 854 OR meta_id BETWEEN 9001 AND 9005;
    DELETE FROM wp_comments WHERE comment_ID = 801;" | sql
}
trap cleanup EXIT
"$DOCKER" exec "$NAME" mkdir -p "$MU"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-options.php" < "$ROOT/reference/pages/options.php"
sql < "$ROOT/reference/attachment-edit/content.sql"
sql < "$ROOT/reference/attachment-page/extra.sql"
echo "DELETE FROM wp_posts WHERE post_type = 'wp_navigation'" | sql
curl -s -o /dev/null "$BASE/"
: > "$OUT"
record() {
  local options=$1 path=$2
  curl -g -s -D "$HEADERS" -o "$TMP" -H "X-HyScribe-Options: $options" "$BASE$path" > /dev/null
  local status location
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  location=$(grep -i '^location:' "$HEADERS" | cut -d ' ' -f 2 | tr -d '\r' || true)
  printf '%s\t%s\t%s\t%s\t%s\n' "$options" "$(printf '%s' "$path" | xxd -p | tr -d '\n')" "$status" "$location" \
    "$(xxd -p "$TMP" | tr -d '\n')" >> "$OUT"
}
paths="12345"
for id in 801 802 803 804 805 806 807 808 809 810 811 812 813 814 815 816 817 818 819 820 821 822 823 824 825 826 828 829 830 \
  9001 9002 9003 9004 9005 9006 9007; do
  paths="$paths $id"
done
for options in "" "wp_attachment_pages_enabled=1"; do
  for id in $paths; do
    record "$options" "/?attachment_id=$id"
  done
  record "$options" "/?attachment_id=801&x=y"
  record "$options" "/?attachment_id=802&preview=true"
  record "$options" "/?subpost_id=801"
  record "$options" "/?attachment_id=801&subpost_id=802"
  for path in '/?attachment=report' '/?attachment=twin' '/?attachment=nope' '/?attachment=a/report' '/?subpost=report' \
    '/?name=report&post_type=attachment' '/?name=twin&post_type=attachment' '/?name=nope&post_type=attachment' \
    '/?attachment=on-a-block' '/?attachment=orphan' '/?attachment=nowhere' '/?attachment=archive' '/?attachment=report&x=y'; do
    record "$options" "$path"
  done
done
wc -l "$OUT"
