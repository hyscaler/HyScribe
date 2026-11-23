<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints block_core_social_link_services(), each service's name and icon,
// as JSON. Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

echo wp_json_encode( block_core_social_link_services(), JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRETTY_PRINT ), "\n";
