<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
// A configuration that reads its settings from the environment, the way
// container images do. Written for HyScribe.

if ( ! function_exists( 'env_or' ) ) {
	function env_or( $name, $default ) {
		$value = getenv( $name );
		return false === $value ? $default : $value;
	}
}

define( 'DB_NAME', env_or( 'WORDPRESS_DB_NAME', 'wordpress' ) );
define( 'DB_USER', env_or( 'WORDPRESS_DB_USER', 'example username' ) );
define( 'DB_PASSWORD', env_or( 'WORDPRESS_DB_PASSWORD', 'example password' ) );
define( 'DB_HOST', env_or( 'WORDPRESS_DB_HOST', 'mysql' ) );
define( 'DB_CHARSET', 'utf8mb4' );
define( 'DB_COLLATE', '' );

define( 'AUTH_KEY', env_or( 'WORDPRESS_AUTH_KEY', 'put your unique phrase here' ) );

$table_prefix = env_or( 'WORDPRESS_TABLE_PREFIX', 'wp_' );

define( 'WP_DEBUG', ! ! env_or( 'WORDPRESS_DEBUG', '' ) );

if ( isset( $_SERVER['HTTP_X_FORWARDED_PROTO'] ) && 'https' === $_SERVER['HTTP_X_FORWARDED_PROTO'] ) {
	$_SERVER['HTTPS'] = 'on';
}

if ( ! defined( 'ABSPATH' ) ) {
	define( 'ABSPATH', __DIR__ . '/' );
}

require_once ABSPATH . 'wp-settings.php';
