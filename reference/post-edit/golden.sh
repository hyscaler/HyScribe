#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's refusals of the block editor's screens,
# post.php?action=edit for posts and pages and post-new.php, for each role,
# over reference/attachment-edit/content.sql (which it adds and deletes
# after), as crates/hyscribe-admin/tests/golden/post-edit.tsv, one row each:
#   variant<TAB>screen?query<TAB>role<TAB>hex(status<TAB>page)
# a redirect kept as 302<TAB>its Location. Only refusals are recorded: the
# editor's page writes the auto-draft and the edit lock, and is not ported.
# Start from a fresh db.sh up, then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MU=/var/www/html/wp-content/mu-plugins
OUT="$ROOT/crates/hyscribe-admin/tests/golden/post-edit.tsv"
TMP=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$TMP.head"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
  echo "DELETE FROM wp_users WHERE ID IN (2, 3); DELETE FROM wp_usermeta WHERE umeta_id BETWEEN 801 AND 806;
    DELETE FROM wp_posts WHERE ID BETWEEN 801 AND 831; DELETE FROM wp_postmeta WHERE meta_id BETWEEN 801 AND 854;
    DELETE FROM wp_comments WHERE comment_ID = 801;" | sql
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
sql < "$ROOT/reference/attachment-edit/content.sql"
record() {
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' -b "hyscribe_role=$3" "$BASE/wp-admin/$2")
  case "$status" in
    302) location=$(grep -i '^location:' "$TMP.head" | cut -d' ' -f2- | tr -d '\r') && printf '302\t%s' "$location" > "$TMP" ;;
    403 | 404 | 409 | 500) { printf '%s\t' "$status"; cat "$TMP"; } > "$TMP.die" && mv "$TMP.die" "$TMP" ;;
    *) echo "PHP answered $status for $2 as $3" >&2; exit 1 ;;
  esac
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$(bin2hex < "$TMP")" >> "$OUT"
}
bin2hex() { od -An -v -tx1 | tr -d ' \n'; }
: > "$OUT"
for role in administrator editor author contributor subscriber; do
  record a8lock 'post.php?post=814&action=edit' "$role"
done
record a8lock 'post.php?post=816&action=edit' contributor
for role in author contributor subscriber; do
  record a8lock 'post.php?post=804&action=edit' "$role"
  record a8lock 'post.php?post=2&action=edit' "$role"
done
for role in contributor subscriber; do
  record a8lock 'post.php?post=1&action=edit' "$role"
done
record a8lock 'post.php?post=806&action=edit' subscriber
record default 'post-new.php' subscriber
for role in author contributor subscriber; do
  record default 'post-new.php?post_type=page' "$role"
done
for role in administrator contributor subscriber; do
  record default 'post-new.php?post_type=attachment' "$role"
  record default 'post-new.php?post_type=nope' "$role"
done
record default 'post-new.php?post_type=' administrator
wc -l "$OUT"
