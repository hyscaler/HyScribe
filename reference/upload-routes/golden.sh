#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's answers to uploads (MVP 6.2): each case of
# reference/upload-routes/cases.list sent as a visitor and as each role,
# logged in through wp-login.php with reference/rest/users.sql's users
# (added and deleted after) and the fixture administrator, into
# crates/hyscribe-http/tests/golden/uploads.tsv:
#   who<TAB>case<TAB>status<TAB>hex(headers)<TAB>hex(body)<TAB>files<TAB>hex(rows)
# where case is the line's number among the cases, files the paths the
# request wrote under the uploads folder, space separated, and rows the new
# attachments' title, GUID and _wp_attached_file, a tab between them and a
# line each; a file written anywhere else under /var/www or /tmp fails the
# run. Over the default install as reference/upload-routes/state.sql leaves
# it; after a request that writes, the database is put back from a dump
# taken once the users logged in, and the files it wrote are deleted. In
# the answers the user's nonces read %nonce%, a new attachment's %nonce%,
# uniqid()'s values %uniqid%, times of the request's own %now% (dates),
# %nowms% (milliseconds) and %today% (F j, Y), the uploads folder of the
# month %ym%, and sizes in bytes %filesize%, which differ by encoder (ADR
# 0015).
# Cron is off and no one may post unfiltered HTML, as in HyScribe; a PDF
# gets no preview, as on a host without Imagick (no-pdf-preview.php).
# Needs a fresh db.sh up and php.sh up; take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/reference/upload-routes"
OUT="$ROOT/crates/hyscribe-http/tests/golden/uploads.tsv"
CORPUS="$ROOT/crates/hyscribe-image/tests/corpus"
MU=/var/www/html/wp-content/mu-plugins
UPLOADS=/var/www/html/wp-content/uploads
TMP=$(mktemp -d)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress "$@"; }
checksums() { sql -N -e 'CHECKSUM TABLE wp_commentmeta, wp_comments, wp_links, wp_options, wp_postmeta, wp_posts, wp_term_relationships, wp_term_taxonomy, wp_termmeta, wp_terms, wp_usermeta, wp_users' < /dev/null; }
files() { "$DOCKER" exec -u www-data "$NAME" sh -c "mkdir -p $UPLOADS && cd $UPLOADS && find . -type f | sed 's|^\./||' " | LC_ALL=C sort; }
cleanup() {
  rm -rf "$TMP"
  echo "DELETE FROM wp_users WHERE ID BETWEEN 701 AND 704; DELETE FROM wp_usermeta WHERE user_id BETWEEN 701 AND 704;" | sql
  "$DOCKER" exec "$DB" rm -f /tmp/upload-routes.sql
  "$DOCKER" exec "$NAME" rm -f /tmp/upload-routes.mark
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-no-cron.php" "$MU/hyscribe-no-pdf-preview.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
echo '<?php define( "DISABLE_WP_CRON", true ); define( "DISALLOW_UNFILTERED_HTML", true );' | "$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-no-cron.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-no-pdf-preview.php" < "$ROOT/reference/uploads/no-pdf-preview.php"
sql < "$ROOT/reference/rest/users.sql"
sql < "$HERE/state.sql"

# The files that are not images, or not what their names say.
mkdir -p "$TMP/files"
printf '<?php echo "ran";\n' > "$TMP/files/evil.php"
printf '<html><body><script>alert(1)</script></body></html>\n' > "$TMP/files/page.html"
printf 'Plain notes.\n' > "$TMP/files/notes.txt"
printf 'Not a picture at all.\n' > "$TMP/files/fake.jpg"
: > "$TMP/files/empty.jpg"
head -c 2101248 /dev/zero > "$TMP/files/over.jpg"
# The multipart body of a crafted case, as the Rust test makes it: its
# file named by the percent-decoded name, then the fields.
crafted() {
  python3 -I - "$@" <<'PY'
import sys, urllib.parse
part, name, path, out, *fields = sys.argv[1:]
name = urllib.parse.unquote_to_bytes(name)
ext = name.rsplit(b".", 1)[1].lower() if b"." in name else b""
kinds = {b"jpg": b"image/jpeg", b"jpeg": b"image/jpeg", b"png": b"image/png", b"gif": b"image/gif",
         b"txt": b"text/plain", b"html": b"text/html", b"htm": b"text/html"}
body = (b'--BOUNDARY\r\nContent-Disposition: form-data; name="' + part.encode() + b'"; filename="' + name
        + b'"\r\nContent-Type: ' + kinds.get(ext, b"application/octet-stream") + b"\r\n\r\n")
body += open(path, "rb").read()
for field in fields:
    key, value = field.split("=", 1)
    body += b'\r\n--BOUNDARY\r\nContent-Disposition: form-data; name="' + key.encode() + b'"\r\n\r\n' + value.encode()
body += b"\r\n--BOUNDARY--\r\n"
open(out, "wb").write(body)
PY
}
# The attachments a request added: title, GUID and _wp_attached_file.
rows() { sql --raw -N -e "SELECT CONCAT_WS(CHAR(9), p.post_title, p.guid, IFNULL(m.meta_value, '')) FROM wp_posts p LEFT JOIN wp_postmeta m ON m.post_id = p.ID AND m.meta_key = '_wp_attached_file' WHERE p.ID > 900 AND p.post_type = 'attachment' ORDER BY p.ID" < /dev/null; }
source_of() { if [ -f "$TMP/files/$1" ]; then echo "$TMP/files/$1"; else echo "$CORPUS/$1"; fi; }

declare -A NONCE FORM
WHO=(visitor administrator editor author contributor subscriber)
NONCE[visitor]=$("$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; echo wp_create_nonce( "wp_rest" );')
FORM[visitor]=$("$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; echo wp_create_nonce( "media-form" );')
for who in "${WHO[@]:1}"; do
  login=$who
  [ "$who" = administrator ] && login=admin
  curl -s -o /dev/null -c "$TMP/$who.jar" -b 'wordpress_test_cookie=WP%20Cookie%20check' \
    --data-urlencode "log=$login" --data-urlencode 'pwd=reference-only-password' -d 'testcookie=1' "$BASE/wp-login.php"
  NONCE[$who]=$(curl -s -b "$TMP/$who.jar" "$BASE/wp-admin/admin-ajax.php?action=rest-nonce")
  [[ "${NONCE[$who]}" =~ ^[0-9a-f]{10}$ ]] || { echo "no nonce for $who" >&2; exit 1; }
  # The media-form nonce of the session the logged in cookie names.
  cookie=$(awk -F'\t' '$6 ~ /^wordpress_logged_in_/ { print $6 "=" $7 }' "$TMP/$who.jar" | head -1)
  FORM[$who]=$("$DOCKER" exec "$NAME" php -r '
    require "/var/www/html/wp-load.php";
    list( $name, $value ) = explode( "=", $argv[1], 2 );
    $_COOKIE[ $name ] = urldecode( $value );
    wp_set_current_user( wp_validate_logged_in_cookie( false ) );
    echo wp_create_nonce( "media-form" );' "$cookie")
  [[ "${FORM[$who]}" =~ ^[0-9a-f]{10}$ ]] || { echo "no media-form nonce for $who" >&2; exit 1; }
done

"$DOCKER" exec "$DB" sh -c "mariadb-dump -uroot -preference-only wordpress > /tmp/upload-routes.sql"
"$DOCKER" exec "$NAME" touch /tmp/upload-routes.mark
before=$(checksums)
kept=$(files)
: > "$OUT"
for who in "${WHO[@]}"; do
  n=0
  while IFS= read -r line; do
    IFS=$'\t' read -r method target flags file fields <<< "$line"
    n=$((n + 1))
    src=${file%%:*}
    sent=${file#*:}
    path=$(source_of "$src")
    [ "$fields" = - ] && fields=""
    args=(-g -s -X "$method" -o "$TMP/body" -D "$TMP/head" -w '%{http_code}')
    [ "$who" = visitor ] || args+=(-b "$TMP/$who.jar")
    url="$BASE$target"
    case ",$flags," in
      *,wrongnonce,*) fields="${fields:+$fields&}_wpnonce=0123456789" ;;
      *,nonce,*) fields="${fields:+$fields&}_wpnonce=${FORM[$who]}" ;;
    esac
    [[ "$target" == *rest_route* && "$who" != visitor ]] && args+=(-H "X-WP-Nonce: ${NONCE[$who]}")
    if [[ ",$flags," == *,raw,* ]]; then
      # A crafted name is sent percent-decoded.
      [[ ",$flags," != *,crafted,* ]] || sent=$(printf '%b' "${sent//%/\\x}")
      [ -z "$fields" ] || url="$url&$fields"
      type=$(sed -n 's/.*type=\([^,]*\).*/\1/p' <<< "$flags")
      if [[ ",$flags," == *,notype,* ]]; then args+=(-H 'Content-Type:'); else args+=(-H "Content-Type: $type"); fi
      [[ ",$flags," == *,nodisposition,* ]] || args+=(-H "Content-Disposition: attachment; filename=$sent")
      args+=(--data-binary "@$path")
    elif [[ ",$flags," == *,crafted,* ]]; then
      # A name curl cannot send as it is, in a body made here.
      part=async-upload
      [[ ",$flags," == *,restfile,* ]] && part=file
      pairs=()
      if [ -n "$fields" ]; then
        while IFS= read -r pair; do
          pairs+=("${pair%%=*}=$(php -r 'echo urldecode( $argv[1] );' "${pair#*=}")")
        done < <(tr '&' '\n' <<< "$fields")
      fi
      crafted "$part" "${file#*:}" "$path" "$TMP/crafted" ${pairs[@]+"${pairs[@]}"}
      args+=(-H 'Content-Type: multipart/form-data; boundary=BOUNDARY' --data-binary "@$TMP/crafted")
    else
      part=async-upload
      [[ ",$flags," == *,restfile,* ]] && part=file
      args+=(-F "$part=@$path;filename=$sent")
      if [ -n "$fields" ]; then
        while IFS= read -r pair; do
          key=$(printf '%b' "${pair%%=*}"); value=${pair#*=}
          value=$(php -r 'echo urldecode( $argv[1] );' "$value")
          args+=(--form-string "$key=$value")
        done < <(tr '&' '\n' <<< "$fields")
      fi
    fi
    start=$(date +%s)
    status=$(curl "${args[@]}" "$url")
    end=$(date +%s)
    made=$(LC_ALL=C comm -13 <(echo "$kept") <(files) | tr '\n' ' ' | sed 's/ $//')
    elsewhere=$("$DOCKER" exec "$NAME" find /var/www /tmp -newer /tmp/upload-routes.mark -type f ! -path "$UPLOADS/*")
    [ -z "$elsewhere" ] || { echo "case $n as $who wrote outside the uploads: $elsewhere" >&2; exit 1; }
    added=$(rows)
    if [ "$(checksums)" != "$before" ]; then
      "$DOCKER" exec "$DB" sh -c "mariadb -uroot -preference-only wordpress < /tmp/upload-routes.sql"
    fi
    for gone in $made; do "$DOCKER" exec "$NAME" rm -f "$UPLOADS/$gone"; done
    headers=$(tr -d '\r' < "$TMP/head" | grep -iE '^(content-type|location|x-wp-upload-attachment-id|x-redirect-by|x-robots-tag|cache-control|expires):' |
      sed -E 's/^([^:]+):/\L\1:/' | sed "s/${NONCE[$who]}/%nonce%/g; s/${FORM[$who]}/%nonce%/g" || true)
    body=$(php "$HERE/mask.php" "$TMP/body" "$start" "$end" "${NONCE[$who]}" "${FORM[$who]}" | xxd -p | tr -d '\n')
    made=$(php -r 'echo str_replace( gmdate( "Y/m/", (int) $argv[2] ), "%ym%/", $argv[1] );' "$made" "$start")
    added=$(php -r 'echo str_replace( gmdate( "Y/m/", (int) $argv[2] ), "%ym%/", $argv[1] );' "$added" "$start" | xxd -p | tr -d '\n')
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$who" "$n" "$status" "$(printf '%s' "$headers" | xxd -p | tr -d '\n')" "$body" "$made" "$added" >> "$OUT"
  done < <(grep -v '^#' "$HERE/cases.list")
done
[ "$(checksums)" = "$before" ] || { echo "the database was not put back" >&2; exit 1; }
[ "$(files)" = "$kept" ] || { echo "uploads were left behind" >&2; exit 1; }
wc -l "$OUT"
