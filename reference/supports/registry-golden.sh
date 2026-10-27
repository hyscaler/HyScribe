#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the block type registry oracle (registry-golden.php) into
# crates/hyscribe-render/tests/golden/registry.tsv. Run golden.sh first:
# its wrapper attribute cases are reused here.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
GOLDEN="$ROOT/crates/hyscribe-render/tests/golden"

"$DOCKER" cp "$ROOT/reference/supports/registry-golden.php" "$NAME:/tmp/registry-golden.php"
"$DOCKER" cp "$GOLDEN/wrapper-attributes.tsv" "$NAME:/tmp/wrapper-attributes.tsv"
"$DOCKER" exec "$NAME" php /tmp/registry-golden.php > "$GOLDEN/registry.tsv"
wc -l "$GOLDEN/registry.tsv"
