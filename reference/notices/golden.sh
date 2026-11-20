#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's screens as they are redirected to after an action,
# with the notices their arguments print, as
# crates/hyscribe-admin/tests/golden/notices.tsv:
#   variant<TAB>file?query<TAB>role<TAB>hex(page)
# Nothing is acted on: the arguments alone drive the notices. Edit User needs
# the users of reference/users-list/content.sql, which it adds and deletes
# after, read through reference/user-edit/variants.php (variant u3); comment
# 1's status for ?same= comes from reference/notices/variants.php. Masks as
# reference/admin-page/golden.sh, reference/settings/golden.sh and
# reference/settings-more/golden.sh. Start from a fresh db.sh up, then php.sh
# up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/notices.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$TMP.head" "$NONCES"
  echo "DELETE FROM wp_users WHERE ID BETWEEN 601 AND 607; DELETE FROM wp_usermeta WHERE umeta_id BETWEEN 601 AND 629;
    DELETE FROM wp_posts WHERE ID BETWEEN 601 AND 604;" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php" "$MU/hyscribe-notices.php" \
    "$MU/hyscribe-settings.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/user-edit/variants.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-notices.php" < "$ROOT/reference/notices/variants.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts screen-options-nonce inlineeditnonce bulk-posts
  add-tag taxinlineeditnonce bulk-tags delete-tag_1 bulk-comments replyto-comment fetch-list-WP_Comments_List_Table
  internal-linking bulk-destroy approve-comment_1 delete-comment_1 bulk-users save-color-scheme create-user
  dismiss-1_new_email general-options writing-options reading-options discussion-options media-options
  update-permalink create-privacy-page set-privacy-page dismiss-1-new_admin_email media-form media-send-to-editor
  set-attachment-thumbnail"
for id in 0 1 2 3 601 602; do
  actions="$actions trash-post_$id update-user_$id reset-password-for-$id"
done
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  : > "$TMP"
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' -b "hyscribe_role=$3; hyscribe_variant=$1" "$BASE/wp-admin/$2")
  [ "$status" = 200 ] || { echo "PHP answered $status for $2" >&2; exit 1; }
  page=$(php -r '
    $p = file_get_contents( $argv[1] );
    foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
      list( $nonce, $action ) = explode( "\t", $line );
      $p = str_replace( $nonce, "%nonce:$action%", $p );
    }
    $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
    $p = preg_replace( "/data-pw=\"[^\"]+\"/", "data-pw=\"%password%\"", $p );
    $p = preg_replace( "/(name=\"pagegen_timestamp\" value=\")[^\"]*/", "\\1%pagegen%", $p );
    $p = preg_replace( "/<kbd>[A-Za-z0-9]{8}<\/kbd>/", "<kbd>%password%</kbd>", $p );
    $p = str_replace( "/" . gmdate( "Y/m/d" ) . "/", "/%ymd%/", $p );
    $p = str_replace( "/" . gmdate( "Y/m" ) . "/", "/%ym%/", $p );
    echo bin2hex( $p );' "$TMP" "$NONCES")
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$page" >> "$OUT"
}
: > "$OUT"
# The posts and pages lists: every count singular and plural, the Undo and
# Edit links, and the arguments' edge values.
for query in trashed=1 trashed=2 trashed=1000 'trashed=1&ids=1' 'trashed=2&ids=1,2' 'trashed=1&ids=a1%3Cb%27,2-3' \
  'trashed=1&ids=' trashed=0 trashed=-3 trashed=x trashed=2x 'untrashed=1&ids=1' 'untrashed=1&ids=2' \
  'untrashed=1&ids=99' 'untrashed=2&ids=1,2' 'untrashed=1&ids=0' 'untrashed=1&ids=1x' untrashed=1 untrashed=3 \
  deleted=1 deleted=5 updated=1 updated=3 locked=1 locked=2 locked=0 skipped=1 'updated=1&locked=1&skipped=2' \
  'deleted=2&trashed=1&untrashed=1&updated=4&locked=3&ids=1' ids=1 'trashed=1&ids=1&post_status=trash' \
  'trashed=1&paged=1'; do
  record default "edit.php?$query" administrator
done
for query in 'post_type=page&trashed=1&ids=2' 'post_type=page&untrashed=1&ids=2' 'post_type=page&updated=2' \
  'post_type=page&locked=1' 'post_type=page&locked=4&deleted=1' 'post_type=page&untrashed=1&ids=1'; do
  record default "edit.php?$query" administrator
done
for role in editor author contributor; do
  record default 'edit.php?untrashed=1&ids=1' "$role"
done
# The terms lists.
for taxonomy in category post_tag; do
  for message in 1 2 3 4 5 6; do
    record default "edit-tags.php?taxonomy=$taxonomy&message=$message" administrator
  done
done
for query in message=0 message=7 message=x message=3abc 'message=%203' message=-1 'message=1&error=1' \
  'message=4&error=' error=1 'message=2&s=a' 'message=1&paged=1'; do
  record default "edit-tags.php?taxonomy=category&$query" administrator
done
record default 'edit-tags.php?taxonomy=post_tag&message=1' editor
# The comments list.
for query in error=1 error=2 error=3 error=x error= approved=1 approved=2 approved=0 approved=-1 'spammed=1&ids=1' \
  'spammed=2&ids=1,2' spammed=1 'spammed=1&ids=a%27b%3Cc%20d' 'trashed=1&ids=1' 'trashed=3&ids=1,2,3' trashed=1 \
  untrashed=1 untrashed=3 unspammed=1 unspammed=2 deleted=1 deleted=2 unapproved=1 same=1 'same=1&approved=1' \
  'same=999&approved=1' 'approved=1&spammed=1&unspammed=1&trashed=1&untrashed=1&deleted=1&same=1&ids=1' \
  'error=1&approved=1' 'approved=1&comment_status=moderated' 'deleted=1&p=1' 'approved=1&paged=1' approved=1x; do
  record default "edit-comments.php?$query" administrator
done
for variant in n6trash n6spam n6pending; do
  record "$variant" 'edit-comments.php?same=1&deleted=1' administrator
  record "$variant" 'edit-comments.php?same=1' administrator
done
record default 'edit-comments.php?same=999' administrator
record default 'edit-comments.php?approved=1' editor
# The users list.
for query in update=del 'update=del&delete_count=1' 'update=del_many&delete_count=3' 'update=del_many&delete_count=0' \
  'update=del&delete_count=1000' 'update=del&delete_count=x' update=add 'update=add&id=1' 'update=add&id=999' \
  'update=add&id=0' 'update=add&id=1%27x' 'update=add&id=' update=resetpassword 'update=resetpassword&reset_count=1' \
  'update=resetpassword&reset_count=2' update=promote update=err_admin_role 'update=err_admin_role&delete_count=2' \
  update=err_admin_del 'update=err_admin_del&delete_count=1' update=err_missing_reassign \
  'update=err_missing_reassign&delete_count=2' update=remove update=err_admin_remove update=bogus update= \
  'update=promote&role=administrator' 'update=del&s=admin' 'update=add&id=1&paged=1'; do
  record default "users.php?$query" administrator
done
# Profile, Edit User and Add User.
for query in updated=1 updated= updated=0 error=new-email error=x error= 'updated=1&error=new-email'; do
  record default "profile.php?$query" administrator
done
record default profile.php?updated=1 subscriber
record default profile.php?error=new-email subscriber
for query in update=add update=addnoconfirmation update=bogus; do
  record default "user-new.php?$query" administrator
done
# The settings screens, their dates of now printed at the test site's clock:
# without the settings_errors transient a save leaves, no notice.
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-settings.php" < "$ROOT/reference/settings/variants.php"
for screen in general writing reading discussion media permalink privacy; do
  record default "options-$screen.php?settings-updated=true" administrator
done
record default options-general.php?settings-updated=0 administrator
record default options-general.php?settings-updated= administrator
"$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-settings.php"
sql < "$ROOT/reference/users-list/content.sql"
for query in 'user_id=601&updated=1' 'user_id=601&updated=1&wp_http_referer=%2Fwp-admin%2Fusers.php%3Frole%3Deditor%26update%3Dx' \
  'user_id=601&updated=1&wp_http_referer=%2Fwp-admin%2Fuser-new.php' 'user_id=601&updated=1&wp_http_referer=http%3A%2F%2Fexample.test%2Fx' \
  'user_id=601&updated=1&wp_http_referer=javascript%3Aalert(1)' 'user_id=601&error=new-email' 'user_id=601&error=other' \
  'user_id=1&updated=1&wp_http_referer=%2Fwp-admin%2Fusers.php' 'user_id=1&error=new-email' 'user_id=602&updated=&error='; do
  record u3 "user-edit.php?$query" administrator
done
wc -l "$OUT"
