#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what PHP and WordPress make of the wp-config.php corpus, as the
# oracle for hyscribe-config's reader:
#   crates/hyscribe-config/tests/golden/wp-config/<name>.tsv  (eval.php)
#   crates/hyscribe-config/tests/golden/db-host.tsv           (db-host.php)
# Needs docker (or podman with DOCKER=podman) and network access.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
CLI_IMAGE="${CLI_IMAGE:-wordpress:cli-php8.4}"
WP_VERSION="${WP_VERSION:-7.1.3}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

"$DOCKER" run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp -v "$ROOT:/repo" "$CLI_IMAGE" sh -euc "
  cd /tmp
  php -d memory_limit=1G /usr/local/bin/wp core download --version=$WP_VERSION --path=/tmp/wp --quiet
  out=/repo/crates/hyscribe-config/tests/golden
  mkdir -p \$out/wp-config
  for file in /repo/crates/hyscribe-config/tests/corpus/wp-config/*.php; do
    name=\$(basename \$file .php)
    php /repo/reference/wpconfig/eval.php \$file > \$out/wp-config/\$name.tsv
  done
  php /repo/reference/wpconfig/db-host.php /tmp/wp < /repo/reference/wpconfig/db-hosts.txt > \$out/db-host.tsv
"
echo "wrote $ROOT/crates/hyscribe-config/tests/golden"
