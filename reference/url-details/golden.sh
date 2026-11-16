#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the link previews (wp-block-editor/v1/url-details) as PHP answers
# them, against the test site of routes.tsv and pages/ beside this script,
# served inside the oracle container by PHP's built in server through
# router.php as http://pages.test:8080 (an /etc/hosts entry for 127.0.0.1,
# which the must-use plugin reference/rest/url-details.php lets PHP fetch
# from): reference/rest/golden.sh reference/rest/url-details.list, into
# crates/hyscribe-rest/tests/golden/url-details.tsv. The oracle reaches no
# other site. Its requests write the cache, put back after each; start from
# a fresh db.sh up, then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SITE=/tmp/hs-url-details
cleanup() {
  "$DOCKER" exec "$NAME" sh -c "pkill -f '^php -S 127.0.0.1:8080' || true; rm -rf $SITE; sed '/ pages\.test\$/d' /etc/hosts > /tmp/hosts && cat /tmp/hosts > /etc/hosts"
}
trap cleanup EXIT
"$DOCKER" exec "$NAME" mkdir -p "$SITE/pages"
for file in routes.tsv router.php; do
  "$DOCKER" exec -i "$NAME" sh -c "cat > $SITE/$file" < "$ROOT/reference/url-details/$file"
done
for page in "$ROOT"/reference/url-details/pages/*; do
  "$DOCKER" exec -i "$NAME" sh -c "cat > $SITE/pages/$(basename "$page")" < "$page"
done
"$DOCKER" exec "$NAME" sh -c 'echo "127.0.0.1 pages.test" >> /etc/hosts'
"$DOCKER" exec -d "$NAME" php -S 127.0.0.1:8080 "$SITE/router.php"
until "$DOCKER" exec "$NAME" curl -fsS -o /dev/null http://pages.test:8080/page.html; do
  sleep 1
done
"$ROOT/reference/rest/golden.sh" "$ROOT/reference/rest/url-details.list"
