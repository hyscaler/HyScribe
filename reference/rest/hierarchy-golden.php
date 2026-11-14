<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Run inside the PHP WordPress container (reference/default-install/php.sh
// up) by reference/rest/golden.sh: prints get_template_hierarchy() of each
// case, which the templates' lookup route resolves, as
//   hex(slug)<TAB>is_custom<TAB>hex(template_prefix)<TAB>json(hierarchy)
// It only reads.

require '/var/www/html/wp-load.php';

$cases = array(
	array( 'index', false, '' ),
	array( 'custom', true, '' ),
	array( 'front-page', false, '' ),
	array( 'home', false, '' ),
	array( 'single-post-hello-world', false, '' ),
	array( 'single-page', false, '' ),
	array( 'single-postx', false, '' ),
	array( 'single-post-', false, '' ),
	array( 'single-wp_block-x', false, '' ),
	array( 'single', false, '' ),
	array( 'singular', false, '' ),
	array( 'page', false, '' ),
	array( 'page-about', false, '' ),
	array( 'pages', false, '' ),
	array( 'page-', false, '' ),
	array( "page-x\n", false, '' ),
	array( "page-\n", false, '' ),
	array( "page-x\ny", false, '' ),
	array( 'author-admin', false, '' ),
	array( 'author', false, '' ),
	array( 'category-news', false, '' ),
	array( 'category', false, '' ),
	array( 'tag-x', false, '' ),
	array( 'tags', false, '' ),
	array( 'archive-x', false, '' ),
	array( 'taxonomy-post_format-post-format-aside', false, '' ),
	array( 'taxonomy-wp_theme', false, '' ),
	array( 'taxonomy-nope-x', false, '' ),
	array( 'taxonomy-category-', false, '' ),
	array( 'taxonomy-categoryx', false, '' ),
	array( 'date', false, '' ),
	array( 'attachment', false, '' ),
	array( 'embed', false, '' ),
	array( '404', false, '' ),
	array( 'search', false, '' ),
	array( '', false, '' ),
	array( 'x', false, 'taxonomy-wp_theme' ),
	array( 'archive', false, 'archive-x' ),
	array( 'category', false, 'category' ),
	array( 'page-x', false, 'page' ),
	array( 'single-post-x', false, 'single-post' ),
	array( 'foo', false, 'bar-baz' ),
	array( 'bar', false, 'bar-baz' ),
	array( 'bar-baz', false, 'bar-baz' ),
);
foreach ( $cases as $case ) {
	list( $slug, $is_custom, $prefix ) = $case;
	printf(
		"%s\t%d\t%s\t%s\n",
		bin2hex( $slug ),
		$is_custom,
		bin2hex( $prefix ),
		wp_json_encode( get_template_hierarchy( $slug, $is_custom, $prefix ) )
	);
}
