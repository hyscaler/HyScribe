#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the Interactivity API's script module data after interactive
# blocks render, from the PHP WordPress oracle (reference/default-install/
# php.sh up), into crates/hyscribe-render/tests/golden/script-module-data.tsv,
# each case in a request of its own. Writes nothing.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/script-module-data.tsv"

: > "$OUT"
for case in 0 1 2 3 4 5 6; do
  "$DOCKER" exec -i -e CASE="$case" "$NAME" php < "$ROOT/reference/script-module-data/golden.php" >> "$OUT"
done
wc -l "$OUT"
