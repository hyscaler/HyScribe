#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records WordPress's add_query_arg() and remove_query_arg() as
# crates/hyscribe-phpser/tests/golden/add-query-arg.tsv. Needs the default
# install oracle running (reference/default-install/db.sh up, then php.sh up).
# Only reads from it.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-phpser/tests/golden/add-query-arg.tsv"

"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/query-arg/golden.php" > "$OUT"
wc -l "$OUT"
