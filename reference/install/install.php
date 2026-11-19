<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Runs PHP WordPress's installer, wp_install(), on an empty database as
// wp-admin/install.php calls it, then writes what it made, with generated
// values masked, to two files (reference/install/golden.sh):
//   php install.php <schema file> <rows file> <title> <login> <email> <public> <password> <url> <1 to mask the site values>
// The schema file holds each table's SHOW CREATE TABLE; the rows file each
// table as `@table<TAB>column...` and one line per row, values escaped as
// the write oracle's (`\N` NULL, `\\`, `\t`, `\n`, `\r`). Masks: %now%
// (the run's time), %unix% and %unix+N% (a time N seconds on) and
// %password% (the hash) always, and with <mask> 1 also %home%, %title%,
// %email%, %login%, %nicename%, %public% (1 or 0, of blog_public and
// default_pingback_flag) and %prefix% (of user_roles, capabilities and
// user_level). The table prefix is the WORDPRESS_TABLE_PREFIX of the environment.

list( , $schema_file, $rows_file, $title, $login, $email, $public, $password, $url, $mask_site ) = $argv;


define( 'WP_INSTALLING', true );
define( 'DISABLE_WP_CRON', true );
// The constants win over wp-config.php's (the second define only warns).
define( 'WP_SITEURL', $url );
define( 'WP_HOME', $url );
// An https site is installed through a request that is_ssl() sees as https,
// so the page's link to the dashboard keeps the site's scheme.
if ( str_starts_with( $url, 'https://' ) ) {
	$_SERVER['HTTPS'] = 'on';
}
error_reporting( E_ALL & ~E_WARNING );
require '/var/www/html/wp-load.php';
require_once ABSPATH . 'wp-admin/includes/upgrade.php';

// The pretty permalink probe is answered as a serving site answers it, with
// the X-Pingback header, so the structure stays, as it will for HyScribe's
// own site (ADR 0021); no mail leaves.
add_filter(
	'pre_http_request',
	fn() => array(
		'headers'  => array( 'X-Pingback' => get_bloginfo( 'pingback_url' ) ),
		'body'     => '',
		'response' => array( 'code' => 200, 'message' => 'OK' ),
		'cookies'  => array(),
		'filename' => null,
	)
);
add_filter( 'pre_wp_mail', '__return_false' );

$start = time();
wp_install( $title, $login, $email, (int) $public, '', $password, 'en_US' );
$end = time();

global $wpdb;
$tables = array(
	'commentmeta'        => 'meta_id',
	'comments'           => 'comment_ID',
	'links'              => 'link_id',
	'options'            => 'option_id',
	'postmeta'           => 'meta_id',
	'posts'              => 'ID',
	'term_relationships' => 'object_id, term_taxonomy_id',
	'term_taxonomy'      => 'term_taxonomy_id',
	'termmeta'           => 'meta_id',
	'terms'              => 'term_id',
	'usermeta'           => 'umeta_id',
	'users'              => 'ID',
);
$title_stored = get_option( 'blogname' );
$email_stored = get_option( 'admin_email' );
$nicename     = $wpdb->get_var( "SELECT user_nicename FROM {$wpdb->users}" );
$login_stored = $wpdb->get_var( "SELECT user_login FROM {$wpdb->users}" );
$prefix       = $wpdb->prefix;

$mask = function ( $value ) use ( $start, $end, $url, $title_stored, $email_stored, $nicename, $login_stored, $mask_site, $prefix ) {
	if ( null === $value ) {
		return '\N';
	}
	if ( preg_match( '/\A\$wp\$2y\$12\$[.\/A-Za-z0-9]{53}\z/', $value ) ) {
		return '%password%';
	}
	if ( preg_match( '/\A\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\z/', $value ) && $value >= gmdate( 'Y-m-d H:i:s', $start ) && $value <= gmdate( 'Y-m-d H:i:s', $end ) ) {
		return '%now%';
	}
	$value = preg_replace_callback(
		'/\d{10}/',
		function ( $m ) use ( $start, $end ) {
			foreach ( array( 0, 3600, 5400, 7200, 43200, 86400, 604800, 15552000 ) as $offset ) {
				if ( $m[0] - $offset >= $start - 2 && $m[0] - $offset <= $end + 2 ) {
					return $offset ? "%unix+$offset%" : '%unix%';
				}
			}
			return $m[0];
		},
		$value
	);
	// Longest first, so the stored title wins over a part of it.
	$replace = ! $mask_site ? array() : array(
		$url          => '%home%',
		$title_stored => '%title%',
		$email_stored => '%email%',
		$nicename     => '%nicename%',
		$login_stored => '%login%',
	);
	unset( $replace[''] );
	uksort( $replace, fn( $a, $b ) => strlen( $b ) <=> strlen( $a ) );
	$value = strtr( $value, $replace );
	if ( $mask_site ) {
		$value = preg_replace( "/\\A" . preg_quote( $prefix, "/" ) . "(user_roles|capabilities|user_level)\\z/", "%prefix%$1", $value );
	}
	return strtr( $value, array( '\\' => '\\\\', "\t" => '\t', "\n" => '\n', "\r" => '\r' ) );
};

$schema = '';
$rows   = "# The rows PHP WordPress's installer writes (reference/install/golden.sh).\n";
foreach ( $tables as $table => $key ) {
	$full    = $wpdb->prefix . $table;
	$create  = $wpdb->get_row( "SHOW CREATE TABLE `$full`", ARRAY_N );
	$schema .= str_replace( "`$full`", "`{prefix}$table`", $create[1] ) . ";\n";
	$columns = $wpdb->get_col( "SHOW COLUMNS FROM `$full`" );
	$rows   .= '@' . $table . "\t" . implode( "\t", $columns ) . "\n";
	foreach ( $wpdb->get_results( "SELECT * FROM `$full` ORDER BY $key", ARRAY_N ) as $row ) {
		$line = array_map( $mask, $row );
		// The two options a site hidden from search engines changes.
		if ( $mask_site && 'options' === $table && in_array( $row[1], array( 'blog_public', 'default_pingback_flag' ), true ) ) {
			$line[2] = '%public%';
		}
		$rows .= implode( "\t", $line ) . "\n";
	}
}
file_put_contents( $schema_file, $schema );
file_put_contents( $rows_file, $rows );
fwrite( STDERR, 'ran from ' . gmdate( 'Y-m-d H:i:s', $start ) . ' to ' . gmdate( 'Y-m-d H:i:s', $end ) . "\n" );
