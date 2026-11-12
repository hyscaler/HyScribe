#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records WordPress's WP_HTML_Tag_Processor and WP_HTML_Processor as goldens
# for hyscribe-html:
#   crates/hyscribe-html/tests/golden/{edge,theme,random,processor}.tsv
# and regenerates the entity tables it needs:
#   crates/hyscribe-html/src/entities.rs
# Needs docker (or podman with DOCKER=podman) and network access.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
CLI_IMAGE="${CLI_IMAGE:-wordpress:cli-php8.4}"
WP_VERSION="${WP_VERSION:-7.1.3}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

mkdir -p "$ROOT/crates/hyscribe-html/tests/golden"
"$DOCKER" run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp -v "$ROOT:/repo" "$CLI_IMAGE" sh -euc "
  php -d memory_limit=1G /usr/local/bin/wp core download --version=$WP_VERSION --path=/tmp/wp --quiet
  out=/repo/crates/hyscribe-html/tests/golden
  php /repo/reference/html/entities.php /tmp/wp > /repo/crates/hyscribe-html/src/entities.rs
  php /repo/reference/html/golden.php /tmp/wp edge > \$out/edge.tsv
  php /repo/reference/html/golden.php /tmp/wp theme /repo/themes/twentytwentyfive > \$out/theme.tsv
  php /repo/reference/html/golden.php /tmp/wp random > \$out/random.tsv
  php /repo/reference/html/processor-golden.php /tmp/wp > \$out/processor.tsv
"
wc -l "$ROOT"/crates/hyscribe-html/tests/golden/*.tsv
