#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what PHP WordPress's installer, wp_install(), writes to an empty
# database, for each case of cases.list (name, title, login, email, public,
# password, URL and table prefix, separated by |), as install.php writes it:
# the `default` case, with the values that depend on the answers masked,
# becomes the data HyScribe's installer copies,
# crates/hyscribe-write/src/install/schema.sql and rows.tsv; the `subfolder`
# case, a site with a path, also gives rewrite_rules_subfolder.txt there, the
# one value that differs with a path (no robots.txt, favicon or sitemap
# rules); the others, with their answers as stored, crates/hyscribe-write/tests/golden/install/<case>.tsv
# and <case>.sql (the tables as SHOW CREATE TABLE prints them).
# Every case is installed as a site that is serving, whose pretty permalink
# probe succeeds (ADR 0021): permalink_structure and rewrite_rules are set.
# Each case runs twice and must come out the same, which keeps a clock that
# ticks over between two events from changing the masked cron option.
# Needs only the PHP image and the database server: it makes and drops its own
# database, `wordpress_install`, and writes nothing else. Start from a fresh
# db.sh up, then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
PHP="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DATA="$ROOT/crates/hyscribe-write/src/install"
OUT="$ROOT/crates/hyscribe-write/tests/golden/install"
TMP=$(mktemp -d)
cleanup() {
  rm -rf "$TMP"
  "$DOCKER" exec "$DB" mariadb -uroot -preference-only -e 'DROP DATABASE IF EXISTS wordpress_install' || true
  "$DOCKER" exec "$PHP" rm -f /tmp/install.php /tmp/install-schema.sql /tmp/install-rows.tsv || true
}
trap cleanup EXIT
mkdir -p "$DATA" "$OUT"
"$DOCKER" exec -i "$PHP" sh -c 'cat > /tmp/install.php' < "$ROOT/reference/install/install.php"
# One run of a case into $TMP/<n>.schema and <n>.rows: n, title, login, email,
# public, password, URL, mask (1 to mask the answers) and prefix.
run() {
  "$DOCKER" exec "$DB" mariadb -uroot -preference-only \
    -e "DROP DATABASE IF EXISTS wordpress_install; CREATE DATABASE wordpress_install; GRANT ALL ON wordpress_install.* TO 'wordpress'@'%'"
  "$DOCKER" exec -e WORDPRESS_DB_NAME=wordpress_install -e WORDPRESS_TABLE_PREFIX="$9" "$PHP" php /tmp/install.php \
    /tmp/install-schema.sql /tmp/install-rows.tsv "$2" "$3" "$4" "$5" "$6" "$7" "$8"
  "$DOCKER" cp "$PHP:/tmp/install-schema.sql" "$TMP/$1.schema"
  "$DOCKER" cp "$PHP:/tmp/install-rows.tsv" "$TMP/$1.rows"
}
while IFS='|' read -r name title login email public password url prefix; do
  [ -n "$name" ] || continue
  # Only the default case masks the answers, as the data the installer copies;
  # the others keep them, so the tests see how each answer is stored.
  mask=0
  [ "$name" = default ] && mask=1
  tries=0
  while :; do
    run a "$title" "$login" "$email" "$public" "$password" "$url" "$mask" "$prefix" 2>/dev/null
    run b "$title" "$login" "$email" "$public" "$password" "$url" "$mask" "$prefix" 2>/dev/null
    if cmp -s "$TMP/a.rows" "$TMP/b.rows"; then break; fi
    tries=$((tries + 1))
    [ "$tries" -lt 6 ] || { echo "$name: two runs never matched" >&2; exit 1; }
  done
  if [ "$name" = default ]; then
    cp "$TMP/a.schema" "$DATA/schema.sql"
    cp "$TMP/a.rows" "$DATA/rows.tsv"
  else
    cp "$TMP/a.rows" "$OUT/$name.tsv"
    if [ "$name" = subfolder ]; then
      awk -F'\t' '$2 == "rewrite_rules" { print $3 }' "$TMP/a.rows" > "$DATA/rewrite_rules_subfolder.txt"
    fi
    cp "$TMP/a.schema" "$OUT/$name.sql"
  fi
  echo "$name: $(grep -vc '^[#@]' "$TMP/a.rows") rows"
done < "$ROOT/reference/install/cases.list"
