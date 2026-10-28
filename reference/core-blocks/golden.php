<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-render's dynamic core blocks. For each request and
// set of options, renders each block's markup with do_blocks() and prints
//   hex(markup)<TAB>hex(request JSON)<TAB>hex(options JSON)<TAB>hex(output)
// The request JSON holds the conditionals the blocks check. Run through
// golden.sh inside the PHP WordPress oracle; nothing is written.

require '/var/www/html/wp-load.php';

// The elements support (wp-elements-* classes and styles) is checked on
// its own; here the callbacks are.
remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

$blocks = array(
	'<!-- wp:site-title /-->',
	'<!-- wp:site-title {"level":0} /-->',
	'<!-- wp:site-title {"level":2} /-->',
	'<!-- wp:site-title {"level":6,"textAlign":"center"} /-->',
	'<!-- wp:site-title {"level":0,"textAlign":"center","align":"wide","fontSize":"x-large"} /-->',
	'<!-- wp:site-title {"level":0,"style":{"typography":{"fontSize":"100px","lineHeight":"1.2"}}} /-->',
	'<!-- wp:site-title {"level":0,"style":{"typography":{"writingMode":"vertical-rl"}},"fontSize":"large"} /-->',
	'<!-- wp:site-title {"level":2,"textAlign":"center","style":{"typography":{"textTransform":"uppercase","fontStyle":"normal","fontWeight":"400"}},"fontSize":"x-large"} /-->',
	'<!-- wp:site-title {"isLink":false} /-->',
	'<!-- wp:site-title {"isLink":false,"level":3} /-->',
	'<!-- wp:site-title {"linkTarget":"_blank"} /-->',
	'<!-- wp:site-title {"linkTarget":""} /-->',
	'<!-- wp:site-title {"level":"2"} /-->',
	'<!-- wp:site-title {"className":"extra"} /-->',
	'<!-- wp:site-title {"style":{"elements":{"link":{"color":{"text":"var:preset|color|accent-1"}}}}} /-->',
	'<!-- wp:site-title {"textAlign":"left","style":{"elements":{"link":{"color":{"text":"#f00"}}}}} /-->',
	'<!-- wp:site-tagline /-->',
	'<!-- wp:site-tagline {"textAlign":"center"} /-->',
	'<!-- wp:site-tagline {"level":0} /-->',
	'<!-- wp:site-tagline {"level":3} /-->',
	'<!-- wp:site-tagline {"level":3,"textAlign":"right","fontSize":"small"} /-->',
);

// `paged` forces is_paged() on the home query: one post never fills a page.
// `stale` lists posts on the home page while page_for_posts names a page
// that is not queried.
$requests = array( '', 'p=1', 'page_id=2', 's=hello', 'paged', 'stale' );
$stale    = array(
	'show_on_front'  => 'page',
	'page_on_front'  => 0,
	'page_for_posts' => 99,
);

$options = array(
	array( 'blogname' => null, 'blogdescription' => null ),
	array( 'blogname' => 'Tom & Jerry <b>"Q"</b>', 'blogdescription' => 'A <em>tag</em> & more' ),
	array( 'blogname' => '  ', 'blogdescription' => '' ),
	array( 'blogname' => '0', 'blogdescription' => '0' ),
);

// Not $request or $query_string: WordPress sets those globals.
foreach ( $requests as $case_query ) {
	// A fresh WP, since wp() keeps the previous request's query vars.
	$GLOBALS['wp'] = new WP();
	foreach ( $stale as $name => $value ) {
		remove_all_filters( "pre_option_$name" );
		if ( 'stale' === $case_query ) {
			add_filter( "pre_option_$name", static fn() => $value );
		}
	}
	wp( in_array( $case_query, array( 'paged', 'stale' ), true ) ? '' : $case_query );
	$GLOBALS['wp_query']->is_paged = 'paged' === $case_query;
	$state                   = array(
		'front_page'        => is_front_page(),
		'home'              => is_home(),
		'paged'             => is_paged(),
		'queried_object_id' => get_queried_object_id(),
		'page_for_posts'    => (int) get_option( 'page_for_posts' ),
	);
	foreach ( $options as $set ) {
		$used = array();
		foreach ( $set as $name => $value ) {
			remove_all_filters( "pre_option_$name" );
			if ( null !== $value ) {
				add_filter( "pre_option_$name", static fn() => $value );
			}
			$used[ $name ] = get_option( $name );
		}
		$used['home'] = home_url();
		foreach ( $blocks as $markup ) {
			echo bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( wp_json_encode( $used ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
		}
	}
}
