#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the RSS block and its feed cache as PHP makes them, against the
# test site reference/url-details/serve.sh serves inside the oracle
# container, into crates/hyscribe-render/tests/golden/rss.tsv (golden.php
# beside this script says what each line holds). Its cache writes are
# rolled back; start from a fresh db.sh up, then php.sh up, and take both
# down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
. "$ROOT/reference/url-details/serve.sh"
OUT="$ROOT/crates/hyscribe-render/tests/golden/rss.tsv"
"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/rss/golden.php" > "$OUT"
wc -l "$OUT"
