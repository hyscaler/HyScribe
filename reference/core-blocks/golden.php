<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-render's dynamic core blocks. For each request and
// set of options, renders each block's markup with do_blocks() and prints
//   site<TAB>hex(markup)<TAB>hex(request JSON)<TAB>hex(options JSON)<TAB>hex(output)
//   post<TAB>hex(markup)<TAB>hex(state JSON)<TAB>hex(post JSON)<TAB>hex(output)
//   diff<TAB>hex(from)<TAB>hex(to)<TAB>hex(human_time_diff( from, to ))
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
// Post blocks: posts built in memory and put in the object cache, so
// nothing reaches the database.
$posts = array(
	array( 1001, 'post', 'publish', '', 'Hello &amp; "quoted" -- Wordpress...', '2026-10-27 08:15:42', '2026-10-28 09:00:00' ),
	array( 1002, 'page', 'publish', '', '  Sample Page  ', '2025-12-31 23:59:59', '2025-12-31 23:59:59' ),
	array( 1003, 'post', 'private', '', 'Secret & things', '2024-02-29 12:00:00', '2024-03-01 00:00:00' ),
	array( 1004, 'post', 'publish', 'pw', 'Locked', '2021-01-03 13:05:09', '2022-01-03 13:05:09' ),
	array( 1005, 'post', 'draft', '', 'Draft &copy; &#169; &x &AMP; &abcdefghij; & ', '2020-02-02 02:02:02', '2019-01-01 00:00:00' ),
	array( 1006, 'post', 'publish', '', '', '2026-01-01 00:00:00', '2026-01-01 00:00:00' ),
	array( 1007, 'post', 'publish', '', '0', '2026-06-15 18:30:00', '2026-06-15 18:30:00' ),
	array( 1008, 'post', 'publish', '', '<em>Tagged</em> title&nbsp;x&;y&#038;z', '1999-12-31 12:00:00', '2000-01-01 12:00:00' ),
);
foreach ( $posts as list( $id, $type, $status, $password, $title, $date, $modified ) ) {
	wp_cache_set(
		$id,
		(object) array(
			'ID'                => $id,
			'post_author'       => '1',
			'post_date'         => $date,
			'post_date_gmt'     => $date,
			'post_content'      => '',
			'post_title'        => $title,
			'post_excerpt'      => '',
			'post_status'       => $status,
			'comment_status'    => 'open',
			'ping_status'       => 'open',
			'post_password'     => $password,
			'post_name'         => "post-$id",
			'to_ping'           => '',
			'pinged'            => '',
			'post_modified'     => $modified,
			'post_modified_gmt' => $modified,
			'post_content_filtered' => '',
			'post_parent'       => 0,
			'guid'              => '',
			'menu_order'        => 0,
			'post_type'         => $type,
			'post_mime_type'    => '',
			'comment_count'     => '0',
			'filter'            => 'raw',
		),
		'posts'
	);
}
$post_blocks = array(
	'<!-- wp:post-title /-->',
	'<!-- wp:post-title {"level":1} /-->',
	'<!-- wp:post-title {"level":0,"textAlign":"center"} /-->',
	'<!-- wp:post-title {"isLink":true} /-->',
	'<!-- wp:post-title {"isLink":true,"linkTarget":"_blank","rel":"nofollow noopener"} /-->',
	'<!-- wp:post-title {"textAlign":"center","level":1,"isLink":true,"fontSize":"xx-large"} /-->',
	'<!-- wp:post-title {"level":1,"align":"wide","fontSize":"x-large"} /-->',
	'<!-- wp:post-title {"style":{"elements":{"link":{"color":{"text":"#f00"}}}}} /-->',
	'<!-- wp:post-date /-->',
	'<!-- wp:post-date {"isLink":true} /-->',
	'<!-- wp:post-date {"textAlign":"right","isLink":true,"fontSize":"small"} /-->',
	'<!-- wp:post-date {"format":"Y-m-d H:i T"} /-->',
	'<!-- wp:post-date {"format":"c"} /-->',
	'<!-- wp:post-date {"displayType":"modified"} /-->',
	'<!-- wp:post-date {"displayType":"modified","format":"U"} /-->',
	'<!-- wp:post-date {"metadata":{"bindings":{"datetime":{"source":"core/post-data","args":{"field":"date"}}}}} /-->',
	'<!-- wp:post-date {"metadata":{"bindings":{"datetime":{"source":"core/post-data","args":{"field":"modified"}}}}} /-->',
	'<!-- wp:post-date {"datetime":"2024-05-06T07:08:09+02:00","format":"F j, Y g:i a"} /-->',
	'<!-- wp:post-date {"datetime":"2024-05-06 07:08","format":"Y-m-d H:i:s"} /-->',
	'<!-- wp:post-date {"datetime":"2024-05-06T07:08:09Z","format":"U"} /-->',
	'<!-- wp:post-date {"datetime":""} /-->',
	'<!-- wp:post-date {"style":{"elements":{"link":{"color":{"text":"var:preset|color|contrast"}}}},"textColor":"contrast"} /-->',
);
// human_time_diff() itself, since the block measures from the real time().
foreach ( array( 0, 1, 59, 60, 61, 119, 120, 3599, 3600, 3601, 5399, 5400, 7200, 86399, 86400, 129600, 172800, 604799, 604800, 1209600, 2591999, 2592000, 2629799, 2629800, 5259600, 31535999, 31536000, 31557600, 63115200, 315576000 ) as $seconds ) {
	foreach ( array( 1, -1 ) as $sign ) {
		$from = 1700000000;
		$to   = $from + $sign * $seconds;
		echo 'diff', "\t", bin2hex( (string) $from ), "\t", bin2hex( (string) $to ), "\t", bin2hex( human_time_diff( $from, $to ) ), "\n";
	}
}

add_filter( 'pre_option_gmt_offset', static fn() => 0 );
foreach ( array( 0, 1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008 ) as $id ) {
	$GLOBALS['post'] = $id ? get_post( $id ) : null;
	$context         = $id ? array( 'postId' => $id, 'postType' => get_post_type( $id ) ) : array();
	$state           = array( 'post' => $id, 'home' => home_url(), 'date_format' => get_option( 'date_format' ) );
	foreach ( $post_blocks as $markup ) {
		$parsed = parse_blocks( $markup )[0];
		$block  = new WP_Block( $parsed, $context );
		echo 'post', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( wp_json_encode( $id ? get_post( $id, ARRAY_A ) : null ) ), "\t", bin2hex( $block->render() ), "\n";
	}
}

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
			echo 'site', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( wp_json_encode( $used ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
		}
	}
}
