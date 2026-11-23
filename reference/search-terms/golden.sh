#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the terms WP_Query::parse_search() finds as
# crates/hyscribe-render/tests/golden/search-terms.tsv. Needs the default
# install oracle running (reference/default-install/db.sh up, then php.sh
# up). Only reads from it.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/search-terms.tsv"

"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/search-terms/golden.php" > "$OUT"
wc -l "$OUT"
