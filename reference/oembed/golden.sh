#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records autoembed and the oEmbed cache as PHP makes them, against the test
# site reference/url-details/serve.sh serves inside the oracle container,
# into crates/hyscribe-render/tests/golden/oembed.tsv (golden.php beside
# this script says what each line holds). Also exports the oEmbed providers
# WP_oEmbed registers into wordpress/wp-includes/oembed-providers.json, with
# the home URL as %home%. Its cache writes are rolled back; start from a
# fresh db.sh up, then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
. "$ROOT/reference/url-details/serve.sh"
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; $home = urlencode( home_url() ); $providers = array(); foreach ( _wp_oembed_get_object()->providers as $pattern => $provider ) { $providers[] = array( $pattern, str_replace( $home, "%home%", $provider[0] ), $provider[1] ); } echo wp_json_encode( $providers, JSON_UNESCAPED_SLASHES | JSON_PRETTY_PRINT ), "\n";' > "$ROOT/wordpress/wp-includes/oembed-providers.json"
OUT="$ROOT/crates/hyscribe-render/tests/golden/oembed.tsv"
"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/oembed/golden.php" > "$OUT"
wc -l "$OUT"
