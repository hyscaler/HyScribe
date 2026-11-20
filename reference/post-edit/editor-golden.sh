#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the pieces of the PHP oracle's block editor page, post-new.php
# and post.php?action=edit for posts and pages, for each role that reaches
# it, as crates/hyscribe-admin/tests/golden/post-editor.tsv, one row each:
#   case<TAB>part<TAB>value
# The parts: `post` (the type and ID the editor starts with), `settings.<key>`
# for each key of the editor's settings, `edits` (the initial edits),
# `categories`, `bindings` and `blocks` (the inline scripts of wp-blocks),
# `body` (hex of the page's `block-editor` div, with the meta boxes area),
# `preloads` (the paths PHP preloads) and `html` (hex of the page, its
# preloads' data left out). Another value over 2000 bytes is kept as
# `sha256:<hex> <length>`. Nonces become %nonce:<action>%, the edit lock's
# and the user settings' times %time%. With RAW=<dir> each page is kept
# there, also masked. The views write (the auto-drafts, the edit locks, the
# theme's global styles post): start from a fresh db.sh up and php.sh up,
# and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MU=/var/www/html/wp-content/mu-plugins
OUT="$ROOT/crates/hyscribe-admin/tests/golden/post-editor.tsv"
TMP=$(mktemp -d)
cleanup() {
  rm -rf "$TMP"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
DESKTOP='Mozilla/5.0 (X11; Linux x86_64)'
CHROME='Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36'
: > "$OUT"
record() {
  local case=$1 target=$2 role=$3 agent=$4
  status=$(curl -s -o "$TMP/page" -w '%{http_code}' -A "$agent" -b "hyscribe_role=$role" "$BASE/wp-admin/$target")
  [ "$status" = 200 ] || { echo "PHP answered $status for $target as $role" >&2; exit 1; }
  [ -z "${RAW:-}" ] || cp "$TMP/page" "$RAW/$case.html"
  id=$(grep -oP "initializeEditor\( 'editor', \"[a-z]+\", \K[0-9]+" "$TMP/page")
  # User 1's nonces, without a session as the requests have none.
  "$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
    "lock-post_$id" "update-post_$id" meta-box-loader meta-box-order closedpostboxes samplepermalink \
    toggle-custom-fields install-plugin_classic-editor wp_rest heartbeat-nonce log-out media-form \
    media-send-to-editor set-attachment-thumbnail internal-linking update_can_compress_scripts \
    screen-options-nonce "image_editor-$id" "set_post_thumbnail-$id" > "$TMP/nonces"
  php "$ROOT/reference/post-edit/editor-parts.php" "$TMP/page" "$TMP/nonces" "$case" ${RAW:+"$RAW"} >> "$OUT"
}
for role in administrator editor author contributor; do
  record "new-post-$role" post-new.php "$role" "$DESKTOP"
done
record new-post-chrome post-new.php administrator "$CHROME"
for role in administrator editor; do
  record "new-page-$role" 'post-new.php?post_type=page' "$role" "$DESKTOP"
done
for role in administrator editor author; do
  record "edit-post-$role" 'post.php?post=1&action=edit' "$role" "$DESKTOP"
done
for role in administrator editor; do
  record "edit-page-$role" 'post.php?post=2&action=edit' "$role" "$DESKTOP"
done
wc -l "$OUT"
