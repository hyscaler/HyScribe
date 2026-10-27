#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records PHP's json_decode() and WordPress's block parser as goldens:
#   crates/hyscribe-phpser/tests/golden/json-decode.tsv
#   crates/hyscribe-render/tests/golden/parse-blocks.tsv
# Needs docker (or podman with DOCKER=podman) and network access.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
CLI_IMAGE="${CLI_IMAGE:-wordpress:cli-php8.4}"
WP_VERSION="${WP_VERSION:-7.1.3}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

mkdir -p "$ROOT/crates/hyscribe-phpser/tests/golden" "$ROOT/crates/hyscribe-render/tests/golden"
"$DOCKER" run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp -v "$ROOT:/repo" "$CLI_IMAGE" sh -euc "
  php -d memory_limit=1G /usr/local/bin/wp core download --version=$WP_VERSION --path=/tmp/wp --quiet
  php /repo/reference/blocks/golden.php json > /repo/crates/hyscribe-phpser/tests/golden/json-decode.tsv
  php /repo/reference/blocks/golden.php blocks /tmp/wp > /repo/crates/hyscribe-render/tests/golden/parse-blocks.tsv
"
wc -l "$ROOT/crates/hyscribe-phpser/tests/golden/json-decode.tsv" "$ROOT/crates/hyscribe-render/tests/golden/parse-blocks.tsv"
