#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the link previews (wp-block-editor/v1/url-details) as PHP answers
# them, against the test site of routes.tsv and pages/ beside this script,
# served inside the oracle container by PHP's built in server through
# router.php as http://pages.test:8080 (an /etc/hosts entry for 127.0.0.1,
# which the must-use plugin reference/rest/url-details.php lets PHP fetch
# from): reference/rest/golden.sh reference/rest/url-details.list, into
# crates/hyscribe-rest/tests/golden/url-details.tsv. The oracle reaches no
# other site. Its requests write the cache, put back after each; start from
# a fresh db.sh up, then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
. "$ROOT/reference/url-details/serve.sh"
"$ROOT/reference/rest/golden.sh" "$ROOT/reference/rest/url-details.list"
