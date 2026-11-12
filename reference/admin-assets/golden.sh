#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's admin style and script printing
# (reference/default-install/php.sh up) into
# crates/hyscribe-admin/tests/golden/assets.tsv. Writes nothing.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/assets.tsv"

mkdir -p "$(dirname "$OUT")"
"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/admin-assets/golden.php" > "$OUT"
wc -l "$OUT"
