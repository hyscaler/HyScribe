#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's Profile screen for each role and in the variants
# of reference/profile/variants.php as
# crates/hyscribe-admin/tests/golden/profile.tsv, the generated password
# masked as %password%:
#   variant?query<TAB>role<TAB>hex(page)
# The ssl variants are served with X-Forwarded-Proto: https. A refusal is kept as 403<TAB>page and a redirect as 302<TAB>location. Masks
# as reference/admin-page/golden.sh. Start from a fresh db.sh up, then
# php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/profile.tsv"
MU=/var/www/html/wp-content/mu-plugins
CHROME='Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36'
CHROME136='Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36'
TMP=$(mktemp)
NONCES=$(mktemp)
cleanup() {
  rm -f "$TMP" "$NONCES"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/profile/variants.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts update-user_1 save-color-scheme dismiss-1_new_email reset-password-for-0"
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  : > "$TMP"
  # The `ssl` variants arrive as if through a TLS proxy.
  proto=http
  case "$1" in ssl*) proto=https ;; esac
  # The chrome variants send Chromium 137's User-Agent, chrome136 the one
  # before isolation, and the localhost variants the Host of a secure context.
  agent='curl/8'
  case "$1" in *chrome136) agent="$CHROME136" ;; *chrome) agent="$CHROME" ;; esac
  host="127.0.0.1:8081"
  case "$1" in localhost*) host="localhost:8081" ;; esac
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' -A "$agent" -H "Host: $host" -H "X-Forwarded-Proto: $proto" -b "hyscribe_role=$2; hyscribe_variant=${1%%\?*}" "$BASE/wp-admin/profile.php$3")
  case "$status" in
    200) ;;
    302) printf '302\t%s' "$(grep -i '^location:' "$TMP.head" | cut -d' ' -f2 | tr -d '\r')" > "$TMP" ;;
    403) { printf '403\t'; cat "$TMP"; } > "$TMP.403" && mv "$TMP.403" "$TMP" ;;
    *) echo "PHP answered $status for $3" >&2; exit 1 ;;
  esac
  page=$(php -r '
    $p = file_get_contents( $argv[1] );
    foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
      list( $nonce, $action ) = explode( "\t", $line );
      $p = str_replace( $nonce, "%nonce:$action%", $p );
    }
    $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
    $p = preg_replace( "/data-pw=\"[^\"]*\"/", "data-pw=\"%password%\"", $p );
    echo bin2hex( $p );' "$TMP" "$NONCES")
  printf '%s%s\t%s\t%s\n' "$1" "$3" "$2" "$page" >> "$OUT"
}
: > "$OUT"
for role in administrator editor author contributor subscriber; do
  record default "$role" ''
done
for variant in options pending caps sessions1 sessions2 noavatars; do
  record "$variant" administrator ''
done
record options subscriber ''
# Over HTTPS, application passwords are on offer.
for role in administrator editor author contributor subscriber; do
  record ssl "$role" ''
done
for variant in sslapps1 sslapps3 sslhidden sslhiddenall sslhiddenempty sslsessions2; do
  record "$variant" administrator ''
done
record sslapps3 subscriber ''
for variant in sslchrome sslchrome136 localhost localhostchrome; do
  record "$variant" administrator ''
done
rm -f "$TMP.head"
wc -l "$OUT"
