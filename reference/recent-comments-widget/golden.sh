#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the recent comments widget from the PHP WordPress oracle
# (reference/default-install/php.sh up) into
# crates/hyscribe-render/tests/golden/recent-comments-widget.tsv. Its inserts are rolled back.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/recent-comments-widget.tsv"

"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/recent-comments-widget/golden.php" > "$OUT"
wc -l "$OUT"
