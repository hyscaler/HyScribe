#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Adds hs-admin's look (hyscribe-look.php, ADR 0008 amendment) to the PHP
# oracle as a must-use plugin, or removes it:
#   reference/admin-look/look.sh add | remove
# Every admin, login and screenshot golden script adds it before its first
# request and removes it on exit.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
MU=/var/www/html/wp-content/mu-plugins
case "${1:-}" in
  add)
    # The footer names the workspace's version, as the binary does.
    version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$(dirname "$0")/../../Cargo.toml" | head -n 1)
    sed "s/@HYSCRIBE_VERSION@/$version/" "$(dirname "$0")/hyscribe-look.php" \
      | "$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-look.php" ;;
  remove) "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-look.php" ;;
  *) echo "usage: $0 add | remove" >&2; exit 2 ;;
esac
