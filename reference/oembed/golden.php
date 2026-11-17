<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints WP_Embed::autoembed() (and the_content for block markup) for the
// cases below, as the default install's post 1 sees them, fetching from the
// test site reference/oembed/golden.sh serves as http://pages.test:8080.
// Lines are
//   name<TAB>hex(content)<TAB>hex(html)<TAB>hex(meta)<TAB>wp-embed enqueued
// where meta is the post's _oembed_ rows after the case, "key=value" lines
// in the order written. The secret wp_filter_oembed_result() draws is
// masked as %secret% and the cache's time as %now%. Every case starts with
// no _oembed_ rows, after its setup rows; all is rolled back.

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

$p = 'http://pages.test:8080/oembed/';

// The cache key of $url: md5( $url . serialize( wp_embed_defaults() ) ).
function key_of( $url ) {
	return md5( $url . serialize( wp_embed_defaults( $url ) ) );
}

// Points the YouTube provider at the test site, as a plugin could.
function to_test_site( $provider, $url, $args ) {
	if ( str_starts_with( $provider, 'https://www.youtube.com/oembed?' ) ) {
		return 'http://pages.test:8080/oembed/trusted.json?' . wp_parse_url( $provider, PHP_URL_QUERY );
	}
	return $provider;
}

$block = function ( $url ) {
	return "<!-- wp:embed {\"url\":\"$url\",\"type\":\"rich\",\"providerNameSlug\":\"test\"} -->\n<figure class=\"wp-block-embed is-type-rich is-provider-test wp-block-embed-test\"><div class=\"wp-block-embed__wrapper\">\n$url\n</div></figure>\n<!-- /wp:embed -->";
};

$cases = array(
	array( 'rich', "{$p}rich.html" ),
	array( 'quote', "{$p}quote.html" ),
	array( 'noframe', "{$p}noframe.html" ),
	array( 'titled', "{$p}titled.html" ),
	array( 'untitled', "{$p}untitled.html" ),
	array( 'photo', "{$p}photo.html" ),
	array( 'badphoto', "{$p}badphoto.html" ),
	array( 'link', "{$p}link.html" ),
	array( 'badjson', "{$p}bad.html" ),
	array( 'relative', "{$p}relative.html" ),
	array( 'xml', "{$p}xml.html" ),
	array( 'doctype', "{$p}doctype.html" ),
	array( 'none', "{$p}none.html" ),
	array( 'single', "{$p}single.html" ),
	array( 'gone', "{$p}gone" ),
	array( 'redirect', "{$p}redirect" ),
	array( 'unresolved', 'http://nowhere.example/page' ),
	array( 'provider', 'https://www.youtube.com/watch?v=abc' ),
	array( 'trusted', 'https://youtu.be/xyz', 'trusted' ),
	array( 'handler', 'https://www.youtube.com/embed/abc' ),
	array( 'paragraph', "<p class=\"x\">\n {$p}rich.html \n</p>" ),
	array( 'amp', "{$p}rich.html?a=1&amp;b=2" ),
	array( 'upper', "  HTTPS://pages.test:8080/oembed/link.html\t" ),
	array( 'inline', "See {$p}rich.html here\n<a href=\"{$p}rich.html\">{$p}rich.html</a>" ),
	array( 'tag', "<div\ndata-x=\"1\">\n{$p}link.html\n</div>" ),
	array( 'twice', "{$p}link.html\n\n{$p}link.html\n{$p}badphoto.html" ),
	array( 'none-url', "no links here\nhttp://\nftp://pages.test/x" ),
	array( 'cached', "{$p}gone", array( key_of( "{$p}gone" ) => '<b>Cached</b> <blockquote class="wp-embedded-content">q</blockquote>' ) ),
	array( 'cached-unknown', "{$p}rich.html", array( key_of( "{$p}rich.html" ) => '{{unknown}}' ) ),
	array( 'expired', "{$p}gone", array( key_of( "{$p}gone" ) => '<i>Old</i>', 'time_' . key_of( "{$p}gone" ) => '1' ) ),
	array( 'content', $block( "{$p}rich.html" ), null, 'content' ),
	array( 'content-failed', $block( "{$p}gone" ), null, 'content' ),
);

global $wpdb;
$wpdb->query( 'START TRANSACTION' );
$GLOBALS['post'] = get_post( 1 );
setup_postdata( $GLOBALS['post'] );
foreach ( $cases as $case ) {
	list( $name, $content ) = $case;
	$setup = $case[2] ?? null;
	$mode  = $case[3] ?? 'autoembed';
	$wpdb->query( "DELETE FROM $wpdb->postmeta WHERE post_id = 1 AND meta_key LIKE '\\_oembed\\_%'" );
	wp_cache_delete( 1, 'post_meta' );
	if ( is_array( $setup ) ) {
		foreach ( $setup as $key => $value ) {
			add_post_meta( 1, '_oembed_' . $key, $value );
		}
	}
	if ( 'trusted' === $setup ) {
		add_filter( 'oembed_fetch_url', 'to_test_site', 10, 3 );
	}
	wp_dequeue_script( 'wp-embed' );
	$html = 'content' === $mode ? apply_filters( 'the_content', $content ) : $GLOBALS['wp_embed']->autoembed( $content );
	remove_filter( 'oembed_fetch_url', 'to_test_site', 10 );
	$html = preg_replace( '/(secret=|data-secret=["\'])[A-Za-z0-9]{10}/', '$1%secret%', $html );
	$rows = $wpdb->get_results( "SELECT meta_key, meta_value FROM $wpdb->postmeta WHERE post_id = 1 AND meta_key LIKE '\\_oembed\\_%' ORDER BY meta_id" );
	$meta = '';
	foreach ( $rows as $row ) {
		$value = $row->meta_value;
		if ( str_starts_with( $row->meta_key, '_oembed_time_' ) && abs( (int) $value - time() ) < 600 ) {
			$value = '%now%';
		}
		$value = preg_replace( '/(secret=|data-secret=["\'])[A-Za-z0-9]{10}/', '$1%secret%', $value );
		$meta .= $row->meta_key . '=' . $value . "\n";
	}
	echo $name, "\t", bin2hex( $content ), "\t", bin2hex( $html ), "\t", bin2hex( $meta ), "\t", wp_script_is( 'wp-embed', 'enqueued' ) ? 1 : 0, "\n";
}
$wpdb->query( 'ROLLBACK' );
