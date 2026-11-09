<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Renders the post author and post author biography blocks over the
// default install for a global post, the author's biography given by
// filter, without the elements support, which is checked on its own.
// Lines are
//   hex(markup)<TAB>postId<TAB>hex(description)<TAB>hex(html)
// The database is only read.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

function block( $name, $attrs ) {
	$json = $attrs ? ' ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES ) : '';
	return "<!-- wp:$name$json /-->";
}

$cases = array(
	block( 'post-author', array() ),
	block( 'post-author', array( 'showAvatar' => false ) ),
	block( 'post-author', array( 'avatarSize' => 24 ) ),
	block( 'post-author', array( 'avatarSize' => 0 ) ),
	block( 'post-author', array( 'showBio' => true ) ),
	block( 'post-author', array( 'byline' => 'Written <b>by</b> <script>x</script>' ) ),
	block( 'post-author', array( 'byline' => '0' ) ),
	block( 'post-author', array( 'isLink' => true ) ),
	block( 'post-author', array( 'isLink' => true, 'linkTarget' => '_blank' ) ),
	block( 'post-author', array( 'isLink' => true, 'linkTarget' => '' ) ),
	block( 'post-author', array( 'isLink' => false, 'linkTarget' => '_blank' ) ),
	block( 'post-author', array( 'textAlign' => 'center', 'itemsJustification' => 'right' ) ),
	block( 'post-author', array( 'style' => array( 'elements' => array( 'link' => array( 'color' => array( 'text' => '#f00' ) ) ) ), 'fontSize' => 'small' ) ),
	block( 'post-author-biography', array() ),
	block( 'post-author-biography', array( 'textAlign' => 'right' ) ),
	block( 'post-author-biography', array( 'textAlign' => '', 'className' => 'x' ) ),
);
$descriptions = array( '', 'Writes <b>things</b> & more.', '0' );

$GLOBALS['wp'] = new WP();
wp( '' );
foreach ( $descriptions as $description ) {
	remove_all_filters( 'get_user_metadata' );
	add_filter(
		'get_user_metadata',
		static fn( $value, $id, $key ) => 'description' === $key ? array( $description ) : $value,
		10,
		3
	);
	foreach ( array( 1, 2 ) as $post_id ) {
		foreach ( $cases as $markup ) {
			$GLOBALS['post'] = get_post( $post_id );
			echo bin2hex( $markup ), "\t", $post_id, "\t", bin2hex( $description ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
		}
	}
}
