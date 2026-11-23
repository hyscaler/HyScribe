#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Exports the social link block's services (names and SVG icons) from the
# PHP WordPress oracle (reference/default-install/php.sh up) into
# crates/hyscribe-render/src/social_link/services.json.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/src/social_link/services.json"

mkdir -p "$(dirname "$OUT")"
"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/social-link/export.php" > "$OUT"
wc -c "$OUT"
