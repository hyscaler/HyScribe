<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-render's dynamic core blocks. For each request and
// set of options, renders each block's markup with do_blocks() and prints
//   site<TAB>hex(markup)<TAB>hex(request JSON)<TAB>hex(options JSON)<TAB>hex(output)
//   post<TAB>hex(markup)<TAB>hex(state JSON)<TAB>hex(post JSON)<TAB>hex(output)
//   diff<TAB>hex(from)<TAB>hex(to)<TAB>hex(human_time_diff( from, to ))
//   class<TAB>hex(case JSON)<TAB>hex(post JSON)<TAB>hex(get_post_class( 'wp-block-post' ))
//   query<TAB>hex(markup)<TAB>hex(request)<TAB>hex(home_url())<TAB>hex(output)
//   body<TAB>hex(request)<TAB>hex(implode( ' ', get_body_class() ))
//   content<TAB>hex(markup)<TAB>hex(state JSON)<TAB>hex(post JSON)<TAB>hex(output)
//   teaser<TAB>hex(state JSON)<TAB>hex(post JSON)<TAB>hex(get_the_content())
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
// get_post_class(), with terms, thumbnails and sticky posts given through
// the filters WordPress offers for them.
$term_sets = array(
	array(),
	array( 'category' => array( array( 1, 'uncategorized' ) ) ),
	array(
		'category'    => array( array( 5, 'news' ), array( 6, '%e2%9c%93' ), array( 7, '123' ), array( 8, '---' ), array( 11, '' ), array( 12, 'a b!' ) ),
		'post_tag'    => array( array( 9, 'tag-a' ), array( 13, '42' ) ),
		'post_format' => array( array( 10, 'post-format-gallery' ) ),
	),
	array( 'post_format' => array( array( 14, 'post-format-a%20side' ) ), 'category' => array( array( 5, 'news' ), array( 5, 'news' ) ) ),
);
foreach ( array( 1001, 1002, 1003, 1004 ) as $id ) {
	foreach ( $term_sets as $terms ) {
		foreach ( array( array( 0, array(), 'home' ), array( 55, array( $id ), 'home' ), array( 55, array( "$id" ), 'paged' ), array( 0, array( $id ), 'single' ) ) as list( $thumbnail, $sticky, $where ) ) {
			$GLOBALS['wp'] = new WP();
			wp( 'single' === $where ? 'p=1' : '' );
			$GLOBALS['wp_query']->is_paged = 'paged' === $where;
			remove_all_filters( 'get_the_terms' );
			remove_all_filters( 'pre_option_sticky_posts' );
			remove_all_filters( 'post_thumbnail_id' );
			add_filter(
				'get_the_terms',
				static function ( $found, $post_id, $taxonomy ) use ( $terms ) {
					if ( empty( $terms[ $taxonomy ] ) ) {
						return false;
					}
					return array_map( static fn( $t ) => new WP_Term( (object) array( 'term_id' => $t[0], 'slug' => $t[1], 'name' => $t[1], 'taxonomy' => $taxonomy ) ), $terms[ $taxonomy ] );
				},
				10,
				3
			);
			add_filter( 'pre_option_sticky_posts', static fn() => $sticky );
			add_filter( 'post_thumbnail_id', static fn() => $thumbnail );
			$case = array(
				'terms'     => $terms,
				'thumbnail' => $thumbnail,
				'sticky'    => serialize( $sticky ),
				'home'      => is_home(),
				'paged'     => is_paged(),
			);
			echo 'class', "\t", bin2hex( wp_json_encode( $case ) ), "\t", bin2hex( wp_json_encode( get_post( $id, ARRAY_A ) ) ), "\t", bin2hex( implode( ' ', get_post_class( 'wp-block-post', $id ) ) ), "\n";
		}
	}
}
remove_all_filters( 'get_the_terms' );
remove_all_filters( 'pre_option_sticky_posts' );
remove_all_filters( 'post_thumbnail_id' );

// The query loop, on the fixture's own posts through WP_Query.
$inner = '<!-- wp:post-title {"isLink":true} /--><!-- wp:post-date /-->';
$loop  = static fn( $query, $template = '', $extra = '' ) => '<!-- wp:query {"queryId":7,"query":' . $query . $extra . '} --><div class="wp-block-query"><!-- wp:post-template' . $template . ' -->' . $inner . '<!-- /wp:post-template --></div><!-- /wp:query -->';
$query_blocks = array(
	$loop( '{"perPage":10,"pages":0,"offset":0,"postType":"post","order":"desc","orderBy":"date","inherit":true}' ),
	$loop( '{"perPage":4,"offset":0,"postType":"post","inherit":false}' ),
	$loop( '{"perPage":4,"postType":"page","inherit":false}' ),
	$loop( '{"perPage":4,"postType":"wp_navigation","inherit":false}' ),
	$loop( '{"perPage":1,"offset":"1","postType":"post","sticky":"ignore","inherit":false}' ),
	$loop( '{"perPage":3,"exclude":[1],"postType":"post","inherit":false}' ),
	$loop( '{"perPage":3,"exclude":["1","x"],"postType":"post","inherit":false}' ),
	$loop( '{"perPage":3,"sticky":"only","inherit":false}' ),
	$loop( '{"perPage":3,"sticky":"exclude","inherit":false}' ),
	$loop( '{"perPage":"abc","order":"ASC","inherit":false}' ),
	$loop( '{"inherit":false}' ),
	$loop( '{"perPage":3,"excludeCurrent":true,"inherit":false}' ),
	$loop( '{"perPage":3,"inherit":false}', ' {"layout":{"type":"grid","columnCount":3,"minimumColumnWidth":"12rem"}}' ),
	$loop( '{"perPage":3,"inherit":false}', ' {"layout":{"type":"grid","columnCount":2}}' ),
	$loop( '{"perPage":3,"inherit":false}', ' {"style":{"elements":{"link":{"color":{"text":"#f00"}}}},"fontSize":"small"}' ),
	$loop( '{"perPage":3,"inherit":false}', '', ',"displayLayout":{"type":"flex","columns":2}' ),
	$loop( '{"perPage":3,"inherit":false}', '', ',"enhancedPagination":true' ),
	'<!-- wp:post-template --><!-- wp:post-title /--><!-- /wp:post-template -->',
	$loop( '{"perPage":3,"inherit":true}', '', '} --><!-- wp:query-no-results --><p>None found.</p><!-- /wp:query-no-results --><!-- wp:query {"query":{"inherit":true}' ),
	'<!-- wp:query {"query":{"perPage":3,"postType":"page","offset":5,"inherit":false}} --><div><!-- wp:query-no-results {"style":{"elements":{"link":{"color":{"text":"#f00"}}}}} --><p>None.</p><!-- /wp:query-no-results --></div><!-- /wp:query -->',
	'<!-- wp:query {"query":{"inherit":true}} --><div><!-- wp:query-no-results --><p>None.</p><!-- /wp:query-no-results --><!-- wp:query-no-results --> <!-- /wp:query-no-results --></div><!-- /wp:query -->',
	'<!-- wp:query-pagination --><b>pages</b><!-- /wp:query-pagination -->',
	'<!-- wp:query-pagination {"style":{"elements":{"link":{"color":{"text":"#f00"}}}}} --><b>pages</b><!-- /wp:query-pagination -->',
	'<!-- wp:query-pagination --> <!-- /wp:query-pagination -->',
	'<!-- wp:query-title {"type":"archive"} /-->',
	'<!-- wp:query-title {"type":"archive","showPrefix":false,"level":0,"textAlign":"center"} /-->',
	'<!-- wp:query-title {"type":"search"} /-->',
	'<!-- wp:query-title {"type":"search","showSearchTerm":false,"level":3} /-->',
	'<!-- wp:query-title {"type":"post-type"} /-->',
	'<!-- wp:query-title {"type":"post-type","showPrefix":false} /-->',
	'<!-- wp:query {"query":{"postType":"page"}} --><div><!-- wp:query-title {"type":"post-type"} /--></div><!-- /wp:query -->',
	'<!-- wp:query {"query":{"postType":"wp_block"}} --><div><!-- wp:query-title {"type":"post-type"} /--></div><!-- /wp:query -->',
	'<!-- wp:query {"query":{"postType":"book"}} --><div><!-- wp:query-title {"type":"post-type"} /--></div><!-- /wp:query -->',
	'<!-- wp:query-title /-->',
	'<!-- wp:query-title {"type":"other"} /-->',
);
foreach ( array( '', 'p=1', 's=nothingmatches', 's=hello <b>"x"</b>', 'cat=1', 'm=2026', 'm=202610', 'm=20261027' ) as $case_query ) {
	$GLOBALS['wp'] = new WP();
	wp( $case_query );
	foreach ( $query_blocks as $markup ) {
		echo 'query', "\t", bin2hex( $markup ), "\t", bin2hex( $case_query ), "\t", bin2hex( home_url() ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
	}
}

// get_body_class() for the requests the query cases use, and a paged one.
foreach ( array( '', 'p=1', 'page_id=2', 'p=999', 's=hello', 's=nothingmatches', 'cat=1', 'cat=999', 'm=2026', 'm=202610', 'm=20261027', 'paged=1', 'paged=2', 'p=1&paged=2', 'cat=1&paged=3' ) as $case_query ) {
	$GLOBALS['wp'] = new WP();
	wp( $case_query );
	echo 'body', "\t", bin2hex( $case_query ), "\t", bin2hex( implode( ' ', get_body_class() ) ), "\n";
}
// Page 3 of lists, and a child page: one post fills no second page, so
// both are set on the query in memory.
foreach ( array( '', 'page_id=2', 'cat=1', 'm=2026', 's=hello', 'page_id=2&child', 'cat=1&slug', 'p=1&book' ) as $case_query ) {
	$GLOBALS['wp'] = new WP();
	wp( preg_replace( '/&(child|slug|book)$/', '', $case_query ) );
	$label = $case_query;
	if ( str_ends_with( $case_query, '&child' ) ) {
		$GLOBALS['wp_query']->get_queried_object()->post_parent = 7;
	} elseif ( str_ends_with( $case_query, '&slug' ) ) {
		$GLOBALS['wp_query']->get_queried_object()->slug = '1';
	} elseif ( str_ends_with( $case_query, '&book' ) ) {
		$GLOBALS['wp_query']->get_queried_object()->post_type = 'book';
	} else {
		$GLOBALS['wp_query']->set( 'paged', 3 );
		$GLOBALS['wp_query']->is_paged = true;
		$label = "$case_query&forced=3";
	}
	echo 'body', "\t", bin2hex( $label ), "\t", bin2hex( implode( ' ', get_body_class() ) ), "\n";
}

// core/post-content on posts built in memory, in a singular request and
// in a list.
$contents = array(
	3001 => array( '', '<!-- wp:paragraph --><p>Hello "world" -- Wordpress (Wordpress) and Wordpress ]]> :)</p><!-- /wp:paragraph -->' ),
	3002 => array( '', '<!-- wp:paragraph --><p>Teaser</p><!-- /wp:paragraph --><!-- wp:more --><!--more--><!-- /wp:more --><!-- wp:paragraph --><p>Rest</p><!-- /wp:paragraph -->' ),
	3003 => array( '', '<p>Short</p><!--more Read <b>on</b> --><p>Long</p>' ),
	3004 => array( 'secret', '<!-- wp:paragraph --><p>Hidden</p><!-- /wp:paragraph -->' ),
	3005 => array( '', '<!-- wp:paragraph --><p>One</p><!-- /wp:paragraph --><!--nextpage--><!-- wp:paragraph --><p>Two</p><!-- /wp:paragraph -->' ),
	3006 => array( '', '' ),
	3008 => array( '', '<!-- wp:post-content /--><p>Nested</p>' ),
	3007 => array( '', '<!--noteaser--><p>Lead</p><!--more--><p>Body</p>' ),
);
// Classic content and the password form also go through wpautop(), not
// ported yet, so for those only get_the_content() is recorded.
$needs_autop = array( 3003, 3004, 3007 );
foreach ( $contents as $id => list( $password, $content ) ) {
	wp_cache_set( $id, (object) array( 'ID' => $id, 'post_author' => '1', 'post_date' => '2026-01-02 03:04:05', 'post_date_gmt' => '2026-01-02 03:04:05', 'post_content' => $content, 'post_title' => "Post $id", 'post_excerpt' => '', 'post_status' => 'publish', 'comment_status' => 'open', 'ping_status' => 'open', 'post_password' => $password, 'post_name' => "post-$id", 'to_ping' => '', 'pinged' => '', 'post_modified' => '2026-01-02 03:04:05', 'post_modified_gmt' => '2026-01-02 03:04:05', 'post_content_filtered' => '', 'post_parent' => 0, 'guid' => '', 'menu_order' => 0, 'post_type' => 'post', 'post_mime_type' => '', 'comment_count' => '0', 'filter' => 'raw' ), 'posts' );
}
$content_blocks = array(
	'<!-- wp:post-content /-->',
	'<!-- wp:post-content {"tagName":"MAIN","align":"wide"} /-->',
	'<!-- wp:post-content {"tagName":"span"} /-->',
);
foreach ( array( true, false ) as $singular ) {
	$GLOBALS['wp'] = new WP();
	wp( $singular ? 'p=1' : '' );
	foreach ( array_keys( $contents ) as $id ) {
		$post            = get_post( $id );
		$GLOBALS['post'] = $post;
		setup_postdata( $post );
		$GLOBALS['more'] = $singular ? 1 : 0;
		$state           = array( 'singular' => $singular, 'home' => home_url(), 'siteurl' => site_url() );
		echo 'teaser', "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( wp_json_encode( get_post( $id, ARRAY_A ) ) ), "\t", bin2hex( get_the_content() ), "\n";
		if ( in_array( $id, $needs_autop, true ) ) {
			continue;
		}
		foreach ( $content_blocks as $markup ) {
			$block = new WP_Block( parse_blocks( $markup )[0], array( 'postId' => $id, 'postType' => 'post' ) );
			echo 'content', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( wp_json_encode( get_post( $id, ARRAY_A ) ) ), "\t", bin2hex( $block->render() ), "\n";
		}
	}
}

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
