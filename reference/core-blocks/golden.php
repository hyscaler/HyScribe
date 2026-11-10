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
// its own; here the callbacks are, except in the query loops' do_blocks().
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
	array( 1005, 'wp_block', 'draft', '', 'Draft &copy; &#169; &x &AMP; &abcdefghij; & ', '2020-02-02 02:02:02', '2019-01-01 00:00:00' ),
	array( 1006, 'post', 'publish', '', '', '2026-01-01 00:00:00', '2026-01-01 00:00:00' ),
	array( 1007, 'post', 'publish', '', '0', '2026-06-15 18:30:00', '2026-06-15 18:30:00' ),
	array( 1008, 'post', 'publish', '', '<em>Tagged</em> title&nbsp;x&;y&#038;z', '1999-12-31 12:00:00', '2000-01-01 12:00:00' ),
);
// Authors: 1006 has a user that does not exist, 1007 none.
$authors = array( 1006 => '2', 1007 => '0' );
foreach ( $posts as list( $id, $type, $status, $password, $title, $date, $modified ) ) {
	wp_cache_set(
		$id,
		(object) array(
			'ID'                => $id,
			'post_author'       => $authors[ $id ] ?? '1',
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
	'<!-- wp:post-author-name /-->',
	'<!-- wp:post-author-name {"isLink":true} /-->',
	'<!-- wp:post-author-name {"isLink":true,"linkTarget":"_blank","textAlign":"right","style":{"elements":{"link":{"color":{"text":"red"}}}}} /-->',
	'<!-- wp:post-author-name {"isLink":true,"linkTarget":"a\\"b"} /-->',
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
remove_all_filters( 'pre_option_sticky_posts' );
remove_all_filters( 'post_thumbnail_id' );

// The post terms block over each term set, given by the get_the_terms filter.
$terms_blocks = array(
	'<!-- wp:post-terms {"term":"category"} /-->',
	'<!-- wp:post-terms {"term":"post_tag"} /-->',
	'<!-- wp:post-terms {"term":"post_format"} /-->',
	'<!-- wp:post-terms {"term":"nav_menu"} /-->',
	'<!-- wp:post-terms {"term":"no_such_taxonomy"} /-->',
	'<!-- wp:post-terms /-->',
	'<!-- wp:post-terms {"term":"category","separator":" | ","prefix":"In <b>cats</b><script>x</script>: ","suffix":" <i>ok</i>","textAlign":"center"} /-->',
	'<!-- wp:post-terms {"term":"post_tag","separator":"","prefix":"0","style":{"elements":{"link":{"color":{"text":"red"}}},"typography":{"fontWeight":"300"}}} /-->',
	'<!-- wp:post-terms {"term":"category","separator":"<&>","className":"x y"} /-->',
);
foreach ( $term_sets as $terms ) {
	remove_all_filters( 'get_the_terms' );
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
	foreach ( $terms_blocks as $markup ) {
		foreach ( array( array( 'postId' => 1001, 'postType' => 'post' ), array() ) as $context ) {
			$block = new WP_Block( parse_blocks( $markup )[0], $context );
			echo 'terms', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( array( 'terms' => $terms, 'context' => (bool) $context, 'home' => home_url() ) ) ), "\t", bin2hex( $block->render() ), "\n";
		}
	}
}
remove_all_filters( 'get_the_terms' );
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
add_filter( 'render_block', 'wp_render_elements_class_name', 10, 2 );
add_filter( 'render_block_data', 'wp_render_elements_support_styles', 10, 1 );
foreach ( array( '', 'p=1', 's=nothingmatches', 's=hello <b>"x"</b>', 'cat=1', 'm=2026', 'm=202610', 'm=20261027' ) as $case_query ) {
	$GLOBALS['wp'] = new WP();
	wp( $case_query );
	foreach ( $query_blocks as $markup ) {
		$counter = substr( wp_unique_prefixed_id( 'wp-elements-' ), 12 );
		echo 'query', "\t", bin2hex( $markup ), "\t", bin2hex( $case_query ), "\t", bin2hex( home_url() ), "\t", bin2hex( do_blocks( $markup ) ), "\t", bin2hex( $counter ), "\n";
	}
}
remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

// get_body_class() for the requests the query cases use, and a paged one.
foreach ( array( '', 'p=1', 'page_id=2', 'p=999', 's=hello', 's=nothingmatches', 'cat=1', 'cat=999', 'm=2026', 'm=202610', 'm=20261027', 'paged=1', 'paged=2', 'p=1&paged=2', 'cat=1&paged=3', 'page=2', 's=hello&page=2' ) as $case_query ) {
	$GLOBALS['wp'] = new WP();
	wp( $case_query );
	echo 'body', "\t", bin2hex( $case_query ), "\t", bin2hex( implode( ' ', get_body_class() ) ), "\n";
}
// The query pagination blocks of a query that inherits the main one, by
// request, page and page count; one post fills no second page, so the page
// and the count are set on the query in memory. Lines are
//   pagination<TAB>hex(markup)<TAB>hex(state JSON)<TAB>hex(output)
$pagination_markups = array(
	'<!-- wp:query-pagination {"paginationArrow":"arrow","layout":{"type":"flex","justifyContent":"space-between"}} --><!-- wp:query-pagination-previous /--><!-- wp:query-pagination-numbers /--><!-- wp:query-pagination-next /--><!-- /wp:query-pagination -->',
	'<!-- wp:query-pagination {"paginationArrow":"chevron","showLabel":false} --><!-- wp:query-pagination-previous {"label":"Back & <b>forth</b>"} /--><!-- wp:query-pagination-next {"label":"On"} /--><!-- /wp:query-pagination -->',
	'<!-- wp:query-pagination --><!-- wp:query-pagination-numbers {"midSize":0} /--><!-- wp:query-pagination-next {"label":"&amp; more"} /--><!-- /wp:query-pagination -->',
	'<!-- wp:query-pagination-previous /--><!-- wp:query-pagination-numbers {"midSize":-1} /--><!-- wp:query-pagination-next /-->',
	'<!-- wp:query-total /--><!-- wp:query-total {"displayType":"range-display","fontSize":"small"} /-->',
);
foreach ( array( '{"inherit":true}', '{"pages":3,"inherit":true}', '{"inherit":false}', '{"pages":-1,"inherit":true}', '{"perPage":1,"offset":5,"inherit":false}', '{"perPage":"0","inherit":false}' ) as $case_query_attr ) {
	foreach ( $pagination_markups as $inner ) {
		$markup = '<!-- wp:query {"query":' . $case_query_attr . '} --><div class="wp-block-query">' . $inner . '</div><!-- /wp:query -->';
		foreach ( array( array( '/', 0, 1 ), array( '/', 1, 2 ), array( '/?paged=2', 2, 2 ), array( '/?s=a+b%20c&paged=3', 3, 5 ), array( '/?cat=1', 1, 10 ), array( '/?paged=6&x=%26', 6, 10 ), array( '/?paged=10', 10, 10 ), array( '/?paged=999', 999, 1200 ), array( '/?p=1&paged=2', 2, 3 ), array( '/BLOG/?s=x&paged=3', 3, 4, 'http://127.0.0.1:8081/blog' ), array( '/x/y?paged=2', 2, 3 ), array( '/', 0, 0, null, 0 ), array( '/?paged=1', 1, 1, null, 1 ), array( '/?paged=2', 2, 2, null, 11 ), array( '/?paged=3&7=q', 3, 5 ) ) as $case ) {
			list( $case_uri, $case_paged, $case_max ) = $case;
			$case_home   = $case[3] ?? 'http://127.0.0.1:8081';
			$case_found  = $case[4] ?? $case_max * 10 - 7;
			$home_filter = static fn( $url ) => str_replace( 'http://127.0.0.1:8081', $case_home, $url );
			add_filter( 'home_url', $home_filter );
			$GLOBALS['wp'] = new WP();
			wp( str_starts_with( $case_uri, '/?p=1' ) ? 'p=1' : '' );
			$_SERVER['REQUEST_URI'] = $case_uri;
			$GLOBALS['wp_query']->set( 'paged', $case_paged );
			$GLOBALS['wp_query']->max_num_pages = $case_max;
			$GLOBALS['wp_query']->found_posts   = $case_found;
			// The global WP::register_globals() sets.
			$GLOBALS['paged'] = $case_paged;
			$state            = array( 'uri' => $case_uri, 'paged' => $case_paged, 'max' => $case_max, 'single' => is_single(), 'home' => home_url(), 'found' => $case_found );
			echo 'pagination', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
			remove_filter( 'home_url', $home_filter );
		}
	}
}
$_SERVER['REQUEST_URI'] = '/';

// The same blocks in queries of their own, which page by `query-<id>-page`,
// with four more posts inserted for them and rolled back after. Lines are
//   ownpage<TAB>hex(markup)<TAB>hex(state JSON)<TAB>hex(output)
global $wpdb;
$wpdb->query( 'START TRANSACTION' );
$extra = array();
foreach ( range( 1, 4 ) as $n ) {
	$extra_id = wp_insert_post( array( 'post_title' => "Extra $n", 'post_status' => 'publish', 'post_date' => "2026-0$n-01 00:00:00", 'post_content' => '' ) );
	$extra[]  = array_intersect_key( get_post( $extra_id, ARRAY_A ), array_flip( array( 'ID', 'post_title', 'post_date', 'post_date_gmt', 'post_name', 'post_type', 'post_status' ) ) );
}
$own_markups = array_merge( $pagination_markups, array( '<!-- wp:post-template --><!-- wp:post-title /--><!-- /wp:post-template -->' ) );
foreach ( array( '{"query":{"perPage":2,"inherit":false}}', '{"queryId":7,"query":{"perPage":2,"inherit":false}}', '{"query":{"perPage":2,"offset":1,"inherit":false}}', '{"query":{"perPage":1,"pages":2,"inherit":false}}' ) as $own_attrs ) {
	foreach ( $own_markups as $inner ) {
		$markup = '<!-- wp:query ' . $own_attrs . ' --><div class="wp-block-query">' . $inner . '</div><!-- /wp:query -->';
		foreach ( array( '/', '/?query-page=2', '/?query-page=3', '/?query-page=9', '/?query-7-page=2&paged=3', '/?query-page=-1', '/?query-page=abc&s=x', '/?query-page=2&a=%26', '/?query-page=2&cst=x', '/?query-page=0', '/?query-page=', '/?query-page=2&paged=abc', '/?query-page=2&5=z&9=y' ) as $own_uri ) {
			$GLOBALS['wp'] = new WP();
			wp( '' );
			$_SERVER['REQUEST_URI'] = $own_uri;
			$_GET                   = array();
			parse_str( (string) parse_url( $own_uri, PHP_URL_QUERY ), $_GET );
			$state = array( 'uri' => $own_uri, 'home' => home_url(), 'posts' => $extra );
			echo 'ownpage', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
		}
	}
}
$wpdb->query( 'ROLLBACK' );
$_SERVER['REQUEST_URI'] = '/';
$_GET                   = array();

// Query filters and orders, on posts and pages inserted for them and rolled
// back after. Lines are
//   filter<TAB>hex(markup)<TAB>hex(state JSON)<TAB>hex(output)
$wpdb->query( 'START TRANSACTION' );
$fruit   = wp_insert_term( 'Fruit', 'category' )['term_id'];
$red     = wp_insert_term( 'Red', 'post_tag' )['term_id'];
$inserts = array(
	array( 'post_title' => 'Banana', 'post_date' => '2026-01-01 00:00:00', 'post_author' => 1 ),
	array( 'post_title' => 'Apple', 'post_date' => '2026-02-01 00:00:00', 'post_author' => 2, 'post_category' => array( $fruit ), 'tags_input' => array( 'red' ), 'post_content' => 'Banana bread' ),
	array( 'post_title' => 'Cherry', 'post_date' => '2026-03-01 00:00:00', 'post_author' => 2, 'post_category' => array( $fruit, 1 ) ),
	array( 'post_title' => 'Top', 'post_type' => 'page', 'post_date' => '2026-01-02 00:00:00', 'menu_order' => 3 ),
	array( 'post_title' => 'Child B', 'post_type' => 'page', 'post_parent' => 2, 'post_date' => '2026-01-03 00:00:00', 'menu_order' => 1 ),
	array( 'post_title' => 'Child A', 'post_type' => 'page', 'post_parent' => 2, 'post_date' => '2026-01-04 00:00:00', 'menu_order' => 2 ),
);
$columns = array_flip( array( 'ID', 'post_author', 'post_title', 'post_date', 'post_date_gmt', 'post_modified', 'post_name', 'post_type', 'post_status', 'post_parent', 'menu_order', 'post_content' ) );
$filter_posts = array();
$formats_of   = array( 'Banana' => 'aside', 'Cherry' => 'quote' );
foreach ( $inserts as $insert ) {
	$id             = wp_insert_post( $insert + array( 'post_status' => 'publish', 'post_content' => '' ) );
	if ( isset( $formats_of[ $insert['post_title'] ] ) ) {
		set_post_format( $id, $formats_of[ $insert['post_title'] ] );
	}
	$row            = array_intersect_key( get_post( $id, ARRAY_A ), $columns );
	$row['terms']   = array();
	foreach ( array( 'category', 'post_tag', 'post_format' ) as $taxonomy ) {
		foreach ( wp_get_object_terms( $id, $taxonomy ) as $term ) {
			$row['terms'][] = array( 'taxonomy' => $taxonomy, 'term_id' => $term->term_id, 'name' => $term->name, 'slug' => $term->slug );
		}
	}
	$filter_posts[] = $row;
}
$post_args = array(
	'"orderBy":"title","order":"asc"', '"orderBy":"title"', '"orderBy":"date","order":"asc"', '"orderBy":"title date","order":"ASC"',
	'"orderBy":"nonsense"', '"orderBy":"","order":"asc"', '"orderBy":"ID","order":"asc"', '"orderBy":"author title","order":"asc"',
	'"orderBy":"post_title%20post_date"', '"orderBy":{"author":"asc","title":"desc"}', '"order":"sideways"',
	'"author":2', '"author":"2"', '"author":"1,2"', '"author":"-2"', '"author":[2]', '"author":[0,"2"]', '"author":0', '"author":""', '"author":-1',
	'"search":"an"', '"search":"zzz"', '"search":"an","orderBy":"relevance"', '"search":""',
	'"search":"banana"', '"search":"banana","orderBy":"relevance"', '"search":"banana","orderBy":"relev%61nce"', '"search":"banana","orderBy":""',
	'"search":"banana","orderBy":"0"', '"search":"banana","orderBy":false', '"search":"banana","orderBy":[]', '"search":"banana","orderBy":"title"', '"search":"banana","orderBy":{"title":"asc"}',
	'"search":"ban\\\\ana"', '"search":"ban\\nana"', '"search":"banana\\\\"', '"search":"\\\\0"',
	'"taxQuery":{"category":[' . $fruit . ']}', '"taxQuery":{"post_tag":[' . $red . ']}', '"taxQuery":{"category":[999]}', '"taxQuery":{"category":[]}',
	'"taxQuery":{"nav_menu":[1]}', '"taxQuery":{"category":[' . $fruit . '],"post_tag":[' . $red . ']}', '"taxQuery":{"category":[-' . $fruit . ']}',
	'"taxQuery":{"include":{"category":[' . $fruit . ']}}', '"taxQuery":{"exclude":{"post_tag":[' . $red . ']}}',
	'"taxQuery":{"include":{"category":[1]},"exclude":{"category":[' . $fruit . ']}}', '"taxQuery":{"include":{}}', '"taxQuery":{"exclude":{"category":[999]}}',
	'"taxQuery":{"include":{"category":[' . $fruit . ']},"post_tag":[' . $red . ']}', '"taxQuery":null',
	'"categoryIds":[' . $fruit . ']', '"tagIds":[' . $red . ']', '"categoryIds":[1],"tagIds":[' . $red . ']', '"categoryIds":[]', '"categoryIds":["abc"]',
	'"categoryIds":[' . $fruit . '],"taxQuery":{"exclude":{"post_tag":[' . $red . ']}}', '"parents":[2]',
	'"format":["aside"]', '"format":["standard"]', '"format":["standard","quote"]', '"format":["nonsense"]', '"format":[]', '"format":"aside"',
	'"format":["standard","standard"]', '"format":["quote","aside"]', '"format":["standard"],"categoryIds":[1]', '"format":["aside"],"taxQuery":{"exclude":{"category":[1]}}',
	'"orderBy":"rand","tagIds":[' . $red . ']', '"orderBy":{"rand":"asc"},"tagIds":[' . $red . ']', '"orderBy":"RAND(3)","tagIds":[' . $red . ']', '"orderBy":"xRaNd(12)y","tagIds":[' . $red . ']',
	'"orderBy":"post__in"', '"orderBy":"post%5F%5Fin"', '"orderBy":"post__in title"', '"orderBy":"post_name__in"', '"orderBy":"post__in","order":"desc"', '"orderBy":"rand(x)"', '"orderBy":"RAND"', '"orderBy":"%2574itle","order":"asc"', '"orderBy":"%74itle","order":"asc"',
);
$page_args = array( '"parents":[2]', '"parents":[0]', '"parents":[2,0]', '"parents":["-2"]', '"parents":[]', '"orderBy":"menu_order","order":"asc"', '"orderBy":"menu_order title"', '"orderBy":"parent menu_order","order":"asc"',
	'"parents":[2,0],"orderBy":"post_parent__in menu_order","order":"asc"', '"parents":[0,2],"orderBy":"post_parent__in menu_order","order":"asc"', '"parents":[0,2],"orderBy":"post_parent__in title"', '"orderBy":"post_parent__in"',
	'"parents":[2,0],"orderBy":{"post_parent__in":"desc","title":"asc"}', '"parents":[2,0],"orderBy":"post_parent%5F%5Fin menu_order"' );
$filter_cases = array();
foreach ( $post_args as $args ) {
	$filter_cases[] = '"postType":"post",' . $args;
}
foreach ( $page_args as $args ) {
	$filter_cases[] = '"postType":"page",' . $args;
}
$GLOBALS['wp'] = new WP();
wp( '' );
foreach ( $filter_cases as $args ) {
	$markup = '<!-- wp:query {"query":{' . $args . ',"perPage":20,"inherit":false}} --><div class="wp-block-query"><!-- wp:post-template --><!-- wp:post-title /--><!-- /wp:post-template --></div><!-- /wp:query -->';
	$state  = array( 'home' => home_url(), 'posts' => $filter_posts, 'sticky' => maybe_serialize( get_option( 'sticky_posts' ) ) );
	echo 'filter', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
}
// The same posts with Apple and Hello world sticky.
update_option( 'sticky_posts', array( $filter_posts[1]['ID'], 1 ) );
$sticky_args = array(
	'', '"perPage":1', '"perPage":2,"offset":2', '"sticky":"ignore"', '"sticky":"only"', '"sticky":"exclude"', '"exclude":[1]', '"search":"a"', '"search":" "',
	'"author":1', '"author":[1]', '"author":"1"', '"taxQuery":{"exclude":{"post_tag":[' . $red . ']}}', '"taxQuery":{"include":{"category":[1]}}', '"categoryIds":[]',
	'"format":["aside"]', '"order":"asc"', '"orderBy":"title","order":"asc"', '"postType":"page"', '"parents":[0]',
	'"format":["aside"],"categoryIds":[1]', '"format":["aside"],"taxQuery":{"exclude":{"post_tag":[' . $red . ']}}', '"perPage":2,"offset":10',
	'"sticky":"only","orderBy":"post__in"', '"sticky":"only","orderBy":"post__in","order":"desc"', '"sticky":"only","orderBy":"post__in title"', '"sticky":"only","orderBy":{"post__in":"desc"}',
	'"sticky":"only","orderBy":"ID post__in","order":"asc"',
);
foreach ( $sticky_args as $args ) {
	$args   = '"postType":"post"' . ( '' === $args ? '' : ',' ) . $args;
	$per    = str_contains( $args, 'perPage' ) ? '' : ',"perPage":20';
	$markup = '<!-- wp:query {"query":{' . $args . $per . ',"inherit":false}} --><div class="wp-block-query"><!-- wp:post-template --><!-- wp:post-title /--><!-- /wp:post-template --><!-- wp:query-total /--></div><!-- /wp:query -->';
	$state  = array( 'home' => home_url(), 'posts' => $filter_posts, 'sticky' => maybe_serialize( get_option( 'sticky_posts' ) ) );
	echo 'filter', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
}
update_option( 'sticky_posts', array() );
$wpdb->query( 'ROLLBACK' );
foreach ( $filter_posts as $row ) {
	clean_post_cache( $row['ID'] );
}
clean_term_cache( array( 1, $fruit ), 'category' );
clean_term_cache( array( $red ), 'post_tag' );
clean_taxonomy_cache( 'post_format' );

// Page 3 of lists, and a child page: one post fills no second page, so
// both are set on the query in memory.
foreach ( array( '', 'page_id=2', 'cat=1', 'm=2026', 's=hello', 'cat=1&page=1', 'page_id=2&child', 'cat=1&slug', 'p=1&book' ) as $case_query ) {
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
	3009 => array( '', '<!-- wp:paragraph --><p>Content</p><!-- /wp:paragraph -->' ),
	3011 => array( '', '<!-- wp:paragraph --><p>One</p><!-- /wp:paragraph --><!-- wp:nextpage --><!--nextpage--><!-- /wp:nextpage --><!-- wp:paragraph --><p>Two</p><!-- /wp:paragraph --><!-- wp:nextpage --><!--nextpage--><!-- /wp:nextpage --><!-- wp:paragraph --><p>Three</p><!-- /wp:paragraph -->' ),
	3013 => array( '', '<!-- wp:paragraph --><p>A</p><!-- /wp:paragraph --><!-- wp:nextpage --><!--nextpage--><!-- /wp:nextpage --><!-- wp:paragraph --><p>B</p><!-- /wp:paragraph -->' ),
	3012 => array( '', '<!-- wp:paragraph --><p>A</p><!-- /wp:paragraph --><!-- wp:nextpage /-->' ),
	3010 => array( '', '<!-- wp:paragraph --><p>w1 w2 w3 w4 w5 w6 w7 w8 w9 w10 w11 w12 w13 w14 w15 w16 w17 w18 w19 w20 w21 w22 w23 w24 w25 w26 w27 w28 w29 w30 w31 w32 w33 w34 w35 w36 w37 w38 w39 w40 w41 w42 w43 w44 w45 w46 w47 w48 w49 w50 w51 w52 w53 w54 w55 w56 w57 w58 w59 w60 w61 w62 w63 w64 w65 w66 w67 w68 w69 w70 w71 w72 w73 w74 w75 w76 w77 w78 w79 w80 w81 w82 w83 w84 w85 w86 w87 w88 w89 w90 w91 w92 w93 w94 w95 w96 w97 w98 w99 w100 w101 w102 w103 w104 w105 w106 w107 w108 w109 w110 w111 w112 w113 w114 w115 w116 w117 w118 w119 w120</p><!-- /wp:paragraph -->' ),
);
// Hand-written excerpts, for the post excerpt block.
$excerpts = array( 3009 => 'Hand <b>made</b> & "quoted"' );
// Classic content and the password form also go through wpautop(), not
// ported yet, so for those only get_the_content() is recorded.
$needs_autop = array( 3003, 3004, 3007 );
foreach ( $contents as $id => list( $password, $content ) ) {
	wp_cache_set( $id, (object) array( 'ID' => $id, 'post_author' => '1', 'post_date' => '2026-01-02 03:04:05', 'post_date_gmt' => '2026-01-02 03:04:05', 'post_content' => $content, 'post_title' => "Post $id", 'post_excerpt' => $excerpts[ $id ] ?? '', 'post_status' => 'publish', 'comment_status' => 'open', 'ping_status' => 'open', 'post_password' => $password, 'post_name' => "post-$id", 'to_ping' => '', 'pinged' => '', 'post_modified' => '2026-01-02 03:04:05', 'post_modified_gmt' => '2026-01-02 03:04:05', 'post_content_filtered' => '', 'post_parent' => 0, 'guid' => '', 'menu_order' => 0, 'post_type' => 'post', 'post_mime_type' => '', 'comment_count' => '0', 'filter' => 'raw' ), 'posts' );
}
$excerpt_blocks = array(
	'<!-- wp:post-excerpt /-->',
	'<!-- wp:post-excerpt {"moreText":"Read <em>more</em> <script>x</script>","excerptLength":10} /-->',
	'<!-- wp:post-excerpt {"moreText":"More","showMoreOnNewLine":false,"textAlign":"center"} /-->',
	'<!-- wp:post-excerpt {"excerptLength":3} /-->',
	'<!-- wp:post-excerpt {"excerptLength":-1} /-->',
	'<!-- wp:post-excerpt {"excerptLength":-2} /-->',
	'<!-- wp:post-excerpt {"moreText":"","excerptLength":200} /-->',
	'<!-- wp:post-excerpt {"excerptLength":101,"fontSize":"small","style":{"elements":{"link":{"color":{"text":"red"}}}}} /-->',
);
$content_blocks = array(
	'<!-- wp:post-content /-->',
	'<!-- wp:post-content {"tagName":"MAIN","align":"wide"} /-->',
	'<!-- wp:post-content {"tagName":"span"} /-->',
);
// Singular and not, and the second page of a singular request.
foreach ( array( array( true, 0 ), array( false, 0 ), array( true, 2 ), array( true, 5 ) ) as list( $singular, $page_var ) ) {
	$GLOBALS['wp'] = new WP();
	wp( $singular ? 'p=1' : '' );
	set_query_var( 'page', $page_var );
	foreach ( array_keys( $contents ) as $id ) {
		$post            = get_post( $id );
		$GLOBALS['post'] = $post;
		setup_postdata( $post );
		$GLOBALS['more'] = $singular ? 1 : 0;
		$state           = array( 'singular' => $singular, 'page' => $page_var, 'home' => home_url(), 'siteurl' => site_url() );
		echo 'teaser', "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( wp_json_encode( get_post( $id, ARRAY_A ) ) ), "\t", bin2hex( get_the_content() ), "\n";
		// With the elements support, through render_block_data as in
		// render_block(), and the elements counter recorded.
		add_filter( 'render_block', 'wp_render_elements_class_name', 10, 2 );
		add_filter( 'render_block_data', 'wp_render_elements_support_styles', 10, 1 );
		foreach ( $excerpt_blocks as $markup ) {
			foreach ( array( true, false ) as $with_post ) {
				$parsed  = parse_blocks( $markup )[0];
				$counter = (int) substr( wp_unique_prefixed_id( 'wp-elements-' ), 12 );
				$block   = new WP_Block( apply_filters( 'render_block_data', $parsed, $parsed, null ), $with_post ? array( 'postId' => $id, 'postType' => 'post' ) : array() );
				$excerpt = array( 'elements' => $counter, 'post' => $with_post ) + $state;
				echo 'excerpt', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $excerpt ) ), "\t", bin2hex( wp_json_encode( get_post( $id, ARRAY_A ) ) ), "\t", bin2hex( $block->render() ), "\n";
			}
		}
		remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
		remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );
		if ( in_array( $id, $needs_autop, true ) ) {
			continue;
		}
		foreach ( $content_blocks as $markup ) {
			$block = new WP_Block( parse_blocks( $markup )[0], array( 'postId' => $id, 'postType' => 'post' ) );
			echo 'content', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( wp_json_encode( get_post( $id, ARRAY_A ) ) ), "\t", bin2hex( $block->render() ), "\n";
		}
	}
}

// core/term-description on a category archive and the home page, with the
// description set on the term in memory. Lines are
//   term<TAB>hex(markup)<TAB>hex(state JSON)<TAB>hex(null)<TAB>hex(output)
$term_blocks = array(
	'<!-- wp:term-description /-->',
	'<!-- wp:term-description {"textAlign":"center","style":{"elements":{"link":{"color":{"text":"red"}}}}} /-->',
	'<!-- wp:term-description {"fontSize":"small"} /-->',
	'<!-- wp:term-description {"textAlign":"0"} /-->',
);
foreach ( array( '', '   ', 'Plain', "Line one\n\nLine \"two\" -- x", '<b>bold</b> & co (c)' ) as $description ) {
	$set_description = static function ( $term ) use ( $description ) {
		$term->description = $description;
		return $term;
	};
	add_filter( 'get_term', $set_description );
	foreach ( array( 'cat=1', '' ) as $case_query ) {
		$GLOBALS['wp'] = new WP();
		wp( $case_query );
		$state = array( 'description' => $description, 'query' => $case_query );
		foreach ( $term_blocks as $markup ) {
			$block = new WP_Block( parse_blocks( $markup )[0] );
			echo 'term', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( 'null' ), "\t", bin2hex( $block->render() ), "\n";
		}
	}
	remove_filter( 'get_term', $set_description );
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

// The comments wrapper and title over in-memory posts: comment count,
// comment status and password.
$comments_blocks = array(
	'<!-- wp:comments --><div class="wp-block-comments">saved</div><!-- /wp:comments -->',
	'<!-- wp:comments-title /-->',
	'<!-- wp:comments-title {"showPostTitle":false} /-->',
	'<!-- wp:comments-title {"showCommentsCount":false} /-->',
	'<!-- wp:comments-title {"showPostTitle":false,"showCommentsCount":false,"level":4,"textAlign":"center"} /-->',
	'<!-- wp:comments-title {"level":3,"fontSize":"large"} /-->',
);
foreach ( array( array( 0, 'open', '' ), array( 0, 'closed', '' ), array( 1, 'closed', '' ), array( 2, 'open', '' ), array( 1234, 'open', '' ), array( 1000000, 'open', '' ), array( 100, 'open', '' ), array( -5, 'open', '' ), array( 1, 'open', 'pw' ) ) as $n => list( $count, $status, $password ) ) {
	$id   = 1100 + $n;
	$copy = clone get_post( 1001 );
	$copy->ID             = $id;
	$copy->comment_count  = (string) $count;
	$copy->comment_status = $status;
	$copy->post_password  = $password;
	wp_cache_set( $id, $copy, 'posts' );
	$GLOBALS['post'] = get_post( $id );
	foreach ( $comments_blocks as $markup ) {
		$block = new WP_Block( parse_blocks( $markup )[0], array( 'postId' => $id, 'postType' => 'post' ) );
		echo 'comments', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( get_post( $id, ARRAY_A ) ) ), "\t", bin2hex( $block->render() ), "\n";
	}
}
foreach ( array( '<!-- wp:comments --><div>x</div><!-- /wp:comments -->', '<!-- wp:comments-title /-->' ) as $markup ) {
	$GLOBALS['post'] = null;
	$block           = new WP_Block( parse_blocks( $markup )[0], array() );
	echo 'comments', "\t", bin2hex( $markup ), "\t", bin2hex( 'null' ), "\t", bin2hex( $block->render() ), "\n";
}

// The post navigation link on single views. Adjacent posts come from the
// database; the in-memory posts sit before and after its one post.
$navigation_blocks = array(
	'<!-- wp:post-navigation-link /-->',
	'<!-- wp:post-navigation-link {"type":"previous"} /-->',
	'<!-- wp:post-navigation-link {"type":"previous","showTitle":true} /-->',
	'<!-- wp:post-navigation-link {"showTitle":true,"linkLabel":true} /-->',
	'<!-- wp:post-navigation-link {"type":"previous","showTitle":true,"linkLabel":true} /-->',
	'<!-- wp:post-navigation-link {"type":"previous","label":"Older <b>post</b> %date","showTitle":true} /-->',
	'<!-- wp:post-navigation-link {"label":"Newer","showTitle":true,"linkLabel":true,"arrow":"arrow","textAlign":"right"} /-->',
	'<!-- wp:post-navigation-link {"type":"previous","arrow":"chevron"} /-->',
	'<!-- wp:post-navigation-link {"type":"previous","arrow":"arrow"} /-->',
	'<!-- wp:post-navigation-link {"arrow":"chevron","label":"0"} /-->',
	'<!-- wp:post-navigation-link {"arrow":"none","label":"%title by %date"} /-->',
	'<!-- wp:post-navigation-link {"type":"sideways"} /-->',
	'<!-- wp:post-navigation-link {"type":"previous","taxonomy":"category"} /-->',
	'<!-- wp:post-navigation-link {"taxonomy":"post_tag"} /-->',
	'<!-- wp:post-navigation-link {"taxonomy":"no_such_taxonomy"} /-->',
	'<!-- wp:post-navigation-link {"taxonomy":"wp_theme"} /-->',
	'<!-- wp:post-navigation-link {"taxonomy":"nav_menu"} /-->',
	'<!-- wp:post-navigation-link {"taxonomy":"link_category"} /-->',
	'<!-- wp:post-navigation-link {"taxonomy":"wp_template_part_area"} /-->',
	'<!-- wp:post-navigation-link {"taxonomy":"wp_pattern_category"} /-->',
);
wp_cache_set( 1010, (object) array( 'ID' => 1010, 'post_author' => '1', 'post_date' => '2026-01-01 00:00:00', 'post_date_gmt' => '2026-01-01 00:00:00', 'post_content' => '', 'post_title' => 'Template', 'post_excerpt' => '', 'post_status' => 'publish', 'comment_status' => 'closed', 'ping_status' => 'closed', 'post_password' => '', 'post_name' => 'template', 'to_ping' => '', 'pinged' => '', 'post_modified' => '2026-01-01 00:00:00', 'post_modified_gmt' => '2026-01-01 00:00:00', 'post_content_filtered' => '', 'post_parent' => 0, 'guid' => '', 'menu_order' => 0, 'post_type' => 'wp_template', 'post_mime_type' => '', 'comment_count' => '0', 'filter' => 'raw' ), 'posts' );
foreach ( array( 1011 => 'nav_menu_item', 1012 => 'link', 1013 => 'wp_template_part' ) as $id => $type ) {
	$copy            = clone get_post( 1010 );
	$copy->ID        = $id;
	$copy->post_type = $type;
	wp_cache_set( $id, $copy, 'posts' );
}
// 'forced' rewrites the adjacent query to give 1006, whose title is empty,
// and gives every post term 1 in every taxonomy.
$force_empty = static fn( $sql ) => str_starts_with( $sql, 'SELECT p.ID FROM' ) ? 'SELECT 1006' : $sql;
$force_terms = static fn() => array( 1 );
foreach ( array( array( 1, '' ), array( 2, '' ), array( 1001, '' ), array( 1004, '' ), array( 1005, '' ), array( 1006, '' ), array( 1008, '' ), array( 1010, '' ), array( 9999, '' ), array( 1001, 'forced' ), array( 1, 'forced' ), array( 1005, 'forced' ), array( 1010, 'forced' ), array( 1011, 'forced' ), array( 1012, 'forced' ), array( 1013, 'forced' ) ) as list( $id, $mode ) ) {
	foreach ( array( true, false ) as $singular ) {
		wp_cache_flush_group( 'post-queries' );
		if ( 'forced' === $mode ) {
			add_filter( 'query', $force_empty );
			add_filter( 'wp_get_object_terms', $force_terms );
		}
		$GLOBALS['wp_query']              = new WP_Query();
		$GLOBALS['wp_query']->is_singular = $singular;
		$GLOBALS['post']                  = get_post( $id );
		foreach ( $navigation_blocks as $markup ) {
			$block = new WP_Block( parse_blocks( $markup )[0], array( 'postId' => $id, 'postType' => get_post_type( $id ) ) );
			$state = array( 'singular' => $singular, 'home' => home_url(), 'date_format' => get_option( 'date_format' ), 'forced' => 'forced' === $mode ? get_post( 1006, ARRAY_A ) : null );
			echo 'adjacent', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( wp_json_encode( get_post( $id, ARRAY_A ) ) ), "\t", bin2hex( $block->render() ), "\n";
		}
		remove_filter( 'query', $force_empty );
		remove_filter( 'wp_get_object_terms', $force_terms );
	}
}
wp_cache_flush_group( 'post-queries' );
// wp() works on $wp_the_query, which $wp_query must point at again.
$GLOBALS['wp_query'] = $GLOBALS['wp_the_query'];
// Read more and comment count blocks on posts of their own: 1021 to 1023
// with comments, 1024 with comments closed.
foreach ( array( 1021 => '1', 1022 => '2', 1023 => '1234', 1024 => '0' ) as $id => $count ) {
	$copy                 = clone get_post( 1001 );
	$copy->ID             = $id;
	$copy->post_title     = 1022 === $id ? '' : "Counted $id";
	$copy->comment_count  = $count;
	$copy->comment_status = 1024 === $id ? 'closed' : 'open';
	wp_cache_set( $id, $copy, 'posts' );
}
$comment_count_blocks = array(
	'<!-- wp:read-more /-->',
	'<!-- wp:read-more {"content":"Go <b>on</b> <script>x</script>","linkTarget":"_blank","justifyContent":"center"} /-->',
	'<!-- wp:read-more {"linkTarget":"a\"b","content":""} /-->',
	'<!-- wp:post-comments-count /-->',
	'<!-- wp:post-comments-count {"textAlign":"right","style":{"elements":{"link":{"color":{"text":"red"}}}}} /-->',
	'<!-- wp:post-comments-link /-->',
	'<!-- wp:post-comments-link {"textAlign":"center","style":{"elements":{"link":{"color":{"text":"red"}}}}} /-->',
);
foreach ( array( 0, 1001, 1003, 1004, 1021, 1022, 1023, 1024 ) as $id ) {
	$GLOBALS['post'] = $id ? get_post( $id ) : null;
	$context         = $id ? array( 'postId' => $id, 'postType' => 'post' ) : array();
	$state           = array( 'post' => $id, 'home' => home_url(), 'date_format' => get_option( 'date_format' ) );
	foreach ( $comment_count_blocks as $markup ) {
		$block = new WP_Block( parse_blocks( $markup )[0], $context );
		echo 'post', "\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( wp_json_encode( $id ? get_post( $id, ARRAY_A ) : null ) ), "\t", bin2hex( $block->render() ), "\n";
	}
}
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
