<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for RSS block markup, fetching from the test site
// reference/rss/golden.sh serves as http://pages.test:8080, and the feed
// cache it leaves. Lines are
//   name<TAB>hex(markup)<TAB>hex(html)<TAB>hex(after)<TAB>hex(before)
// where after and before are the feed transients after the case and as it
// starts, "name=hex(value)" lines in option_id order: times written as
// %t%, and in the cached feed the two expiration times as 0 and the Date
// header left out. A case starts with no feed transients unless it keeps
// the last case's; its setup then changes them. All is rolled back.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

add_filter(
	'http_request_host_is_external',
	function ( $external, $host ) {
		return $external || 'pages.test' === $host;
	},
	10,
	2
);

$f = 'http://pages.test:8080/feed/';

function rss( $attrs ) {
	return '<!-- wp:rss ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES ) . ' /-->';
}

$all = array(
	'displayDate'    => true,
	'displayAuthor'  => true,
	'displayExcerpt' => true,
	'excerptLength'  => 10,
	'itemsToShow'    => 10,
	'openInNewTab'   => true,
	'rel'            => ' nofollow "x" ',
	'blockLayout'    => 'grid',
	'columns'        => 3,
	'className'      => 'extra',
);

global $wpdb;
$transients = "option_name LIKE '\\_site\\_transient\\_%feed\\_%'";

// The feed transient of $url: its data through $change, saved back.
function change_cache( $url, $change ) {
	$name = 'feed_' . md5( $url );
	$data = get_site_transient( $name );
	update_option( '_site_transient_' . $name, $change( $data ), false );
}

$cases = array(
	array( 'rss2', rss( array( 'feedURL' => "{$f}rss2.xml" ) ) ),
	array( 'rss2-cached', rss( array( 'feedURL' => "{$f}rss2.xml", 'displayDate' => true ) ), 'keep' ),
	array( 'rss2-all', rss( array_merge( $all, array( 'feedURL' => "{$f}rss2.xml" ) ) ) ),
	array( 'rss2-excerpt', rss( array( 'feedURL' => "{$f}rss2.xml", 'displayExcerpt' => true, 'itemsToShow' => 2, 'blockLayout' => 'list', 'columns' => 4 ) ) ),
	array( 'atom', rss( array_merge( $all, array( 'feedURL' => "{$f}atom.xml", 'excerptLength' => 3, 'rel' => '' ) ) ) ),
	array( 'many', rss( array( 'feedURL' => "{$f}many.xml", 'displayDate' => true ) ) ),
	array( 'empty', rss( array( 'feedURL' => "{$f}empty.xml" ) ) ),
	array( 'page', rss( array( 'feedURL' => "{$f}page.html" ) ) ),
	array( 'broken', rss( array( 'feedURL' => "{$f}broken.xml" ) ) ),
	array( 'gone', rss( array( 'feedURL' => "{$f}gone" ) ) ),
	array( 'redirect', rss( array( 'feedURL' => "{$f}redirect", 'itemsToShow' => 1 ) ) ),
	array( 'unresolved', rss( array( 'feedURL' => 'http://nowhere.example/feed' ) ) ),
	array( 'home', rss( array( 'feedURL' => home_url( '/' ) ) ) ),
	array( 'empty-url', rss( array( 'feedURL' => '' ) ) ),
	array( 'copied', rss( array( 'feedURL' => "{$f}gone", 'itemsToShow' => 1 ) ), 'copy' ),
	array( 'expired', rss( array( 'feedURL' => "{$f}rss2.xml", 'itemsToShow' => 1 ) ), 'expire' ),
	array( 'stale', rss( array( 'feedURL' => "{$f}rss2.xml", 'itemsToShow' => 1 ) ), 'stale' ),
	array( 'old-build', rss( array( 'feedURL' => "{$f}rss2.xml", 'itemsToShow' => 1 ) ), 'build' ),
);

$wpdb->query( 'START TRANSACTION' );
foreach ( $cases as $case ) {
	list( $name, $markup ) = $case;
	$setup = $case[2] ?? null;
	if ( 'keep' !== $setup ) {
		$wpdb->query( "DELETE FROM $wpdb->options WHERE $transients" );
		wp_cache_flush();
	}
	if ( in_array( $setup, array( 'copy', 'expire', 'stale', 'build' ), true ) ) {
		fetch_feed( "{$f}rss2.xml" );
		$source = 'feed_' . md5( "{$f}rss2.xml" );
		if ( 'copy' === $setup ) {
			// The four rows of the feed's cache, moved to the other URL's names.
			$from = md5( "{$f}rss2.xml" );
			$to   = md5( "{$f}gone" );
			$wpdb->query( "UPDATE $wpdb->options SET option_name = REPLACE( option_name, '$from', '$to' ) WHERE option_name LIKE '%$from'" );
		} elseif ( 'expire' === $setup ) {
			update_option( '_site_transient_timeout_' . $source, 1, false );
		} elseif ( 'stale' === $setup ) {
			change_cache( "{$f}rss2.xml", function ( $data ) {
				$data['cache_expiration_time'] = 1;
				return $data;
			} );
		} else {
			change_cache( "{$f}rss2.xml", function ( $data ) {
				$data['build'] = 1;
				return $data;
			} );
		}
		wp_cache_flush();
	}
	$before = cache_rows();
	$html   = do_blocks( $markup );
	echo $name, "\t", bin2hex( $markup ), "\t", bin2hex( $html ), "\t", bin2hex( cache_rows() ), "\t", bin2hex( $before ), "\n";
}
$wpdb->query( 'ROLLBACK' );

// The feed transients, masked.
function cache_rows() {
	global $wpdb;
	$rows  = $wpdb->get_results( "SELECT option_name, option_value FROM $wpdb->options WHERE option_name LIKE '\\_site\\_transient\\_%feed\\_%' ORDER BY option_id" );
	$cache = '';
	foreach ( $rows as $row ) {
		$value = $row->option_value;
		if ( abs( (int) $value - time() ) < 50000 && (string) (int) $value === $value ) {
			$value = '%t%';
		} elseif ( is_serialized( $value ) ) {
			$data = unserialize( $value );
			foreach ( array( 'cache_expiration_time', '__cache_expiration_time' ) as $time ) {
				if ( isset( $data[ $time ] ) && $data[ $time ] > 1 ) {
					$data[ $time ] = 0;
				}
			}
			unset( $data['headers']['date'] );
			$value = serialize( $data );
		}
		$cache .= $row->option_name . '=' . bin2hex( $value ) . "\n";
	}
	return $cache;
}
