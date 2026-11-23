#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records dates in named time zones from the oracle image's PHP, whose
# bundled time zone database is the release jiff-tzdb carries, into
# crates/hyscribe-render/tests/golden/date-zones.tsv. Needs no database.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
IMAGE="${WORDPRESS_IMAGE:-wordpress:7.1.3-php8.4-apache}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/date-zones.tsv"

"$DOCKER" run --rm -i "$IMAGE" php -r 'echo timezone_version_get(), "\n";' >&2
"$DOCKER" run --rm -i "$IMAGE" php < "$ROOT/reference/date/zones-golden.php" > "$OUT"
wc -lc "$OUT"
