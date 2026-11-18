#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the mail oracle: the wp_mail() calls of each case in
# reference/mail/run.php, kept by the must-use plugin capture.php instead of
# sent, as crates/hyscribe-mail/tests/golden/mail.tsv, and
# wp_specialchars_decode()'s cases as decode.tsv beside it; the saves that
# send them (`run.php flows`) go to
# crates/hyscribe-http/tests/golden/mail-flows.tsv. Each case runs in its
# own PHP process inside a transaction it rolls back, once with the
# fixture's site title and once with an odd one (the saves only with the
# fixture's). Needs reference/default-install/php.sh up over a fresh
# db.sh up; take both down after, as the auto increment counters keep the
# rolled back IDs.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-mail/tests/golden"
MU=/var/www/html/wp-content/mu-plugins
DIR=/tmp/hyscribe-mail
cleanup() { "$DOCKER" exec "$NAME" rm -rf "$DIR" "$MU/hyscribe-mail-capture.php"; }
trap cleanup EXIT
"$DOCKER" exec "$NAME" mkdir -p "$DIR" "$MU"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-mail-capture.php" < "$ROOT/reference/mail/capture.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $DIR/run.php" < "$ROOT/reference/mail/run.php"
mkdir -p "$OUT"
"$DOCKER" exec "$NAME" php "$DIR/run.php" decode > "$OUT/decode.tsv"
: > "$OUT/mail.tsv"
for case in $("$DOCKER" exec "$NAME" php "$DIR/run.php"); do
  for blog in plain odd; do
    "$DOCKER" exec "$NAME" php "$DIR/run.php" "$case" "$blog" >> "$OUT/mail.tsv"
  done
done
"$DOCKER" exec "$NAME" php "$DIR/run.php" new-admin-email empty >> "$OUT/mail.tsv"
FLOWS="$ROOT/crates/hyscribe-http/tests/golden/mail-flows.tsv"
: > "$FLOWS"
for case in $("$DOCKER" exec "$NAME" php "$DIR/run.php" flows); do
  "$DOCKER" exec "$NAME" php "$DIR/run.php" "$case" plain >> "$FLOWS"
done
echo "flows: $(wc -l < "$FLOWS") lines"
echo "decode: $(wc -l < "$OUT/decode.tsv") lines, mail: $(wc -l < "$OUT/mail.tsv") lines"
