<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for home link blocks, alone and in navigation blocks
// (with no overlay menu), on several requests, without the elements
// support, which is checked on its own. Lines are
//   hex(markup)<TAB>hex(request)<TAB>is_front_page()<TAB>is_home()<TAB>get_queried_object_id()<TAB>page_for_posts<TAB>hex(html)
// show_on_front and page_for_posts are set by filter; the database is
// only read.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

function home( $attrs ) {
	$json = $attrs ? ' ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES ) : '';
	return "<!-- wp:home-link$json /-->";
}
function nav( $attrs, $inner ) {
	$attrs['overlayMenu'] = 'never';
	return '<!-- wp:navigation ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES ) . ' -->' . $inner . '<!-- /wp:navigation -->';
}

$cases = array(
	home( array() ),
	home( array( 'label' => 'Start <b>here</b><script>x</script>' ) ),
	home( array( 'label' => '0' ) ),
	home( array( 'opensInNewTab' => true, 'description' => 'The <em>home</em> page' ) ),
	home( array( 'opensInNewTab' => 'yes', 'className' => 'extra' ) ),
	nav( array(), home( array() ) ),
	nav( array( 'textColor' => 'accent-1', 'backgroundColor' => 'base' ), home( array() ) ),
	nav( array( 'style' => array( 'color' => array( 'text' => '#123', 'background' => '#fed' ) ) ), home( array() ) ),
	nav( array( 'customTextColor' => '#123', 'customBackgroundColor' => '#fed' ), home( array() ) ),
	nav( array( 'textColor' => 'contrast', 'style' => array( 'color' => array( 'background' => '#fed' ) ) ), home( array( 'label' => 'Back' ) ) ),
);
// Posts on the front with no page for posts, or the sample page; and a
// page on the front that is not set, with the sample page for posts.
foreach ( array( array( 'posts', '0' ), array( 'posts', '2' ), array( 'page', '2' ) ) as list( $show_on_front, $posts_page ) ) {
	remove_all_filters( 'pre_option_show_on_front' );
	remove_all_filters( 'pre_option_page_for_posts' );
	add_filter( 'pre_option_show_on_front', static fn() => $show_on_front );
	add_filter( 'pre_option_page_for_posts', static fn() => $posts_page );
	foreach ( array( '', 'p=1', 'page_id=2' ) as $query ) {
		$GLOBALS['wp'] = new WP();
		wp( $query );
		foreach ( $cases as $markup ) {
			echo bin2hex( $markup ), "\t", bin2hex( $query ), "\t", (int) is_front_page(), "\t", (int) is_home(), "\t", get_queried_object_id(), "\t", $posts_page, "\t", bin2hex( do_blocks( $markup ) ), "\n";
		}
	}
}
