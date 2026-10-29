#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the elements support oracle (elements-golden.php) into
# crates/hyscribe-render/tests/golden/elements.tsv. Needs php.sh up; only
# reads from the database.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/elements.tsv"

"$DOCKER" cp "$ROOT/reference/supports/elements-golden.php" "$NAME:/tmp/elements-golden.php"
"$DOCKER" exec "$NAME" php /tmp/elements-golden.php > "$OUT"
wc -l "$OUT"
