<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Records the links WordPress builds under one permalink setup, as lines of
//   function and arguments<TAB>link
// Options come from filters set before WordPress loads, as in
// rules-golden.php, so nothing is written. Run in the PHP container:
//   docker exec -i hyscribe-default-install-php php /dev/stdin <structure> [category base] [tag base] [page on front] < reference/rewrite/links-golden.php
// reference/rewrite/golden.sh runs it for every setup.
$options = array(
	'permalink_structure' => $argv[1] ?? '',
	'category_base'       => $argv[2] ?? '',
	'tag_base'            => $argv[3] ?? '',
);
if ( ! empty( $argv[4] ) ) {
	$options['page_on_front'] = $argv[4];
	$options['show_on_front'] = 'page';
}
$GLOBALS['wp_filter'] = array();
foreach ( $options as $name => $value ) {
	$GLOBALS['wp_filter'][ "pre_option_$name" ][10][] = array(
		'function'      => fn() => $value,
		'accepted_args' => 1,
	);
}
require '/var/www/html/wp-load.php';

function out( $name, $link ) {
	echo $name, "\t", $link, "\n";
}

// Posts as they could be stored: the fixture's, and others in memory.
$post = get_post( 1 );
$page = get_post( 2 );
$draft = get_post( 3 );
$posts = array(
	'post 1'             => $post,
	'page 2'             => $page,
	'draft page 3'       => $draft,
	'future post'        => new WP_Post( (object) array( 'ID' => 7, 'post_type' => 'post', 'post_status' => 'future', 'post_name' => 'later', 'post_date' => '2027-01-02 03:04:05', 'post_author' => 1, 'filter' => 'raw' ) ),
	'draft post'         => new WP_Post( (object) array( 'ID' => 8, 'post_type' => 'post', 'post_status' => 'draft', 'post_name' => '', 'post_date' => '2026-01-02 03:04:05', 'post_author' => 1, 'filter' => 'raw' ) ),
	'private post'       => new WP_Post( (object) array( 'ID' => 9, 'post_type' => 'post', 'post_status' => 'private', 'post_name' => 'secret', 'post_date' => '2026-12-31 23:59:58', 'post_author' => 1, 'filter' => 'raw' ) ),
	'nameless published' => new WP_Post( (object) array( 'ID' => 10, 'post_type' => 'post', 'post_status' => 'publish', 'post_name' => '', 'post_date' => '2026-05-06 07:08:09', 'post_author' => 1, 'filter' => 'raw' ) ),
	'unicode post'       => new WP_Post( (object) array( 'ID' => 11, 'post_type' => 'post', 'post_status' => 'publish', 'post_name' => '%e2%82%ac-price', 'post_date' => '2026-05-06 07:08:09', 'post_author' => 1, 'filter' => 'raw' ) ),
	'child page'         => new WP_Post( (object) array( 'ID' => 12, 'post_type' => 'page', 'post_status' => 'publish', 'post_name' => 'child', 'post_parent' => 2, 'post_date' => '2026-05-06 07:08:09', 'post_author' => 1, 'filter' => 'raw' ) ),
);
foreach ( $posts as $name => $p ) {
	out( "permalink $name", get_permalink( $p ) );
}
out( 'post_comments_feed_link post 1', get_post_comments_feed_link( 1 ) );
out( 'post_comments_feed_link post 1 atom', get_post_comments_feed_link( 1, 'atom' ) );
out( 'post_comments_feed_link page 2', get_post_comments_feed_link( 2 ) );
out( 'post_comments_feed_link page 2 rss2', get_post_comments_feed_link( 2, 'rss2' ) );
$category = get_term( 1, 'category' );
out( 'term_link category 1', get_term_link( $category ) );
out( 'term_link tag', get_term_link( new WP_Term( (object) array( 'term_id' => 5, 'slug' => 'a-tag', 'taxonomy' => 'post_tag', 'parent' => 0 ) ) ) );
out( 'term_link format', get_term_link( new WP_Term( (object) array( 'term_id' => 6, 'slug' => 'post-format-aside', 'taxonomy' => 'post_format', 'parent' => 0 ) ) ) );
out( 'term_feed_link category 1', get_term_feed_link( 1, 'category' ) );
out( 'term_feed_link category 1 atom', get_term_feed_link( 1, 'category', 'atom' ) );
out( 'year_link 2026', get_year_link( 2026 ) );
out( 'month_link 2026 3', get_month_link( 2026, 3 ) );
out( 'day_link 2026 3 7', get_day_link( 2026, 3, 7 ) );
foreach ( array( '', 'rss2', 'atom', 'rdf', 'comments_rss2', 'comments_atom' ) as $feed ) {
	out( "feed_link $feed", get_feed_link( $feed ) );
}
foreach ( array( 'hello', 'a b/c', 'é&x' ) as $search ) {
	out( "search_link $search", get_search_link( $search ) );
	out( "search_feed_link $search", get_search_feed_link( $search ) );
	out( "search_feed_link $search atom", get_search_feed_link( $search, 'atom' ) );
	out( "search_comments_feed_link $search atom", get_search_comments_feed_link( $search, 'atom' ) );
}
out( 'author_posts_url 1', get_author_posts_url( 1 ) );
out( 'author_feed_link 1', get_author_feed_link( 1 ) );
out( 'post_embed_url post 1', get_post_embed_url( 1 ) );
out( 'post_embed_url page 2', get_post_embed_url( 2 ) );
out( 'trackback_url post 1', ( function () { $GLOBALS['post'] = get_post( 1 ); return get_trackback_url(); } )() );
out( 'sitemap_url index', get_sitemap_url( 'index' ) );
out( 'sitemap_url posts post 2', get_sitemap_url( 'posts', 'post', 2 ) );
out( 'sitemap_url taxonomies category', get_sitemap_url( 'taxonomies', 'category' ) );
out( 'sitemap_url users', get_sitemap_url( 'users' ) );
out( 'rest_url /', get_rest_url( null, '/' ) );
out( 'rest_url /wp/v2/posts/1', get_rest_url( null, '/wp/v2/posts/1' ) );
foreach ( array( '/', '/hello-world/', '/category/uncategorized/', '/category/uncategorized/page/3/', '/?s=a&paged=2', '/2026/?x=1', '/index.php/hello-world/' ) as $uri ) {
	$_SERVER['REQUEST_URI'] = $uri;
	foreach ( array( 1, 2 ) as $n ) {
		out( "pagenum_link $uri $n", get_pagenum_link( $n, false ) );
	}
}
