#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's Media screens, the library (upload.php) in its
# grid and list modes and Add New Media (media-new.php), for each role and
# with the variants of reference/media/variants.php, which write nothing, as
#   crates/hyscribe-admin/tests/golden/media.tsv      (default install)
#   crates/hyscribe-admin/tests/golden/media-ttd.tsv  (Theme Test Data, with
#     HYSCRIBE_PHP_DB=wordpress_ttd reference/default-install/php.sh up)
# one row each:
#   variant<TAB>screen?query<TAB>role<TAB>hex(page)
# A refusal is kept as 403<TAB>page and a redirect as 302<TAB>location. Masks
# as reference/admin-page/golden.sh; a page of the Theme Test Data, whose
# content is not ours to commit, as the two hashes of
# reference/media/mask.php instead of hex(page). Start from a fresh db.sh up, then
# php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MU=/var/www/html/wp-content/mu-plugins
TTD=$("$DOCKER" exec "$NAME" sh -c 'echo "$WORDPRESS_DB_NAME"')
if [ "$TTD" = wordpress_ttd ]; then
  OUT="$ROOT/crates/hyscribe-admin/tests/golden/media-ttd.tsv"
else
  OUT="$ROOT/crates/hyscribe-admin/tests/golden/media.tsv"
fi
TMP=$(mktemp)
NONCES=$(mktemp)
cleanup() {
  rm -f "$TMP" "$TMP.head" "$NONCES"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/media/variants.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts screen-options-nonce bulk-media media-form
  media-send-to-editor set-attachment-thumbnail find-posts"
# The attachments' own nonces.
ids=$("$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; echo implode( " ", get_posts( array( "post_type" => "attachment", "post_status" => "any", "numberposts" => -1, "fields" => "ids" ) ) );')
for id in $ids; do
  actions="$actions trash-post_$id delete-post_$id untrash-post_$id update-post_$id"
done
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  : > "$TMP"
  # The iOS variants are a browser's, which PHP reads from the user agent.
  agent=curl
  case "$1" in
    m4ios5) agent='Mozilla/5.0 (iPhone; CPU iPhone OS 5_1 like Mac OS X) Mobile' ;;
    m4ios7) agent='Mozilla/5.0 (iPhone; CPU iPhone OS 7_0 like Mac OS X) Mobile' ;;
  esac
  settings=
  [ "$1" = m4uploader ] && settings="; wp-settings-1=uploader%3D1"
  # Without DISALLOW_UNFILTERED_HTML: the uploader allows HTML and scripts.
  [ "$1" = u5html ] && settings="; hyscribe_unfiltered_html=1"
  status=$(curl -s -A "$agent" -o "$TMP" -D "$TMP.head" -w '%{http_code}' -b "hyscribe_role=$3; hyscribe_variant=$1$settings" "$BASE/wp-admin/$2")
  case "$status" in
    200) ;;
    302) printf '302\t%s' "$(grep -i '^location:' "$TMP.head" | cut -d' ' -f2 | tr -d '\r')" > "$TMP" ;;
    403) { printf '403\t'; cat "$TMP"; } > "$TMP.403" && mv "$TMP.403" "$TMP" ;;
    *) echo "PHP answered $status for $2" >&2; exit 1 ;;
  esac
  hashed=
  [ "$TTD" = wordpress_ttd ] && hashed=1
  page=$(php "$ROOT/reference/media/mask.php" "$TMP" "$NONCES" "$hashed" "$2")
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$page" >> "$OUT"
}
: > "$OUT"
for screen in upload.php upload.php?mode=list media-new.php; do
  for role in administrator editor author contributor subscriber; do
    record default "$screen" "$role"
  done
done
record m4list upload.php administrator
record m4types upload.php?mode=list administrator
for query in 'mode=grid&attachment-filter=detached' 'm=202001&post_mime_type=image&mine=1&cat=x&foo=%3Cb%3E' \
  's=a%22b&attachment-filter=post_mime_type:image' 'mode=bogus' 'posted=1' 'message=4&ids=5,6' 'trashed=2&ids=7' \
  'attached=3' 'detach=1' 'deleted=2' 'untrashed=1' 'error=1' 'mode=list&s=x%22y' 'mode=list&posted=1&detach=2' \
  'mode=list&message=4&ids=5,6&m=2026' 'mode=list&_wp_http_referer=%2Fx&_wpnonce=n&s=b' 'mode=list&action=-1&filter_action=Filter'; do
  record default "upload.php?$query" administrator
done
for query in 'browser-uploader' 'post_id=1' 'post_id=999'; do
  record default "media-new.php?$query" administrator
done
record m4ios5 media-new.php administrator
record m4ios7 media-new.php administrator
record m4ios7 upload.php administrator
record m4large media-new.php administrator
record m4uploader media-new.php administrator
record default upload.php?post_mime_type=bogus administrator
if [ "$TTD" != wordpress_ttd ]; then
  for role in administrator editor author; do
    record u5html upload.php "$role"
  done
fi
if [ "$TTD" = wordpress_ttd ]; then
  for query in 'paged=2' 'attachment-filter=post_mime_type:image' 'attachment-filter=post_mime_type:audio' \
    'attachment-filter=post_mime_type:video' 'attachment-filter=detached' 'attachment-filter=mine' \
    'attachment-filter=trash' 's=canola' 's=jpg' 'orderby=title&order=asc' 'orderby=author&order=desc' \
    'orderby=parent' 'orderby=date&order=asc' 'm=201008' 'author=2' 'post_mime_type=audio'; do
    record default "upload.php?mode=list&$query" administrator
  done
  for role in editor author; do
    record default 'upload.php?mode=list&attachment-filter=detached' "$role"
  done
  record m4paged upload.php?mode=list administrator
  record m4paged 'upload.php?mode=list&paged=3' administrator
  record m4paged 'upload.php?mode=list&paged=9' administrator
  for variant in m4pages m4states m4recent m4hidden; do
    record "$variant" upload.php?mode=list administrator
  done
  record m4trash 'upload.php?mode=list&attachment-filter=trash' administrator
fi
wc -l "$OUT"
