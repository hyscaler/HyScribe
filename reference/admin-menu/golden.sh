#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's admin menu, the #adminmenumain block of wp-admin
# screens, for each role, as crates/hyscribe-admin/tests/golden/menu.tsv:
#   role<TAB>screen<TAB>status<TAB>hex(menu)
# Needs the default install oracle running (db.sh up, then php.sh up).
# Admin screens write to the database (the dashboard's Quick Draft keeps an
# auto-draft), so start from a fresh db.sh up and take it down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/menu.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
HEADERS=$(mktemp)
cleanup() {
  rm -f "$TMP" "$HEADERS"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
: > "$OUT"
record() {
  local role=$1 screen=$2
  : > "$TMP"
  curl -s -D "$HEADERS" -o "$TMP" -b "hyscribe_role=$role" "$BASE/wp-admin/$screen"
  local status menu
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  menu=$(php -r '$p = file_get_contents( $argv[1] ); $a = strpos( $p, "<div id=\"adminmenumain\"" ); $b = strpos( $p, "<div id=\"wpcontent\">" ); echo false === $a ? "" : bin2hex( substr( $p, $a, $b - $a ) );' "$TMP")
  printf '%s\t%s\t%s\t%s\n' "$role" "$screen" "$status" "$menu" >> "$OUT"
}
for screen in '' 'index.php' 'edit.php' 'post-new.php' 'edit.php?post_type=page' 'post-new.php?post_type=page' \
  'edit-tags.php?taxonomy=category' 'edit-tags.php?taxonomy=post_tag' 'upload.php' 'media-new.php' 'edit-comments.php' \
  'themes.php' 'site-editor.php' 'users.php' 'user-new.php' 'profile.php' 'tools.php' 'import.php' 'export.php' \
  'site-health.php' 'options-general.php' 'options-writing.php' 'options-reading.php' 'options-discussion.php' \
  'options-media.php' 'options-permalink.php' 'options-privacy.php' 'plugins.php' 'update-core.php'; do
  record administrator "$screen"
done
for role in editor author contributor subscriber; do
  for screen in '' 'edit.php' 'profile.php' 'upload.php' 'edit-comments.php' 'edit.php?post_type=page' 'tools.php'; do
    record "$role" "$screen"
  done
done
wc -l "$OUT"
