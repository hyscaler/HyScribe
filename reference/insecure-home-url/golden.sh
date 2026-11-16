#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records wp_replace_insecure_home_url() from the PHP WordPress oracle
# (reference/default-install/php.sh up) into
# crates/hyscribe-render/tests/golden/insecure-home-url.tsv. Only reads.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/insecure-home-url.tsv"

"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/insecure-home-url/golden.php" > "$OUT"
wc -l "$OUT"
