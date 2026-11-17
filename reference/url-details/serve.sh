# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Sourced by the golden scripts that fetch: serves the test site of
# routes.tsv and pages/ beside this script inside the oracle container, by
# PHP's built in server through router.php, as http://pages.test:8080 (an
# /etc/hosts entry for 127.0.0.1), and takes it down when the script exits.
# The oracle reaches no other site. Needs DOCKER, NAME and ROOT.
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
