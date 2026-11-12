<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints synced pattern blocks over patterns inserted into the fixture
// database inside a transaction that is rolled back. Lines are
//   post<TAB>ID<TAB>post_type<TAB>post_status<TAB>hex(post_password)<TAB>hex(post_content)
//   block<TAB>hex(markup)<TAB>hex(html)

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

global $wpdb;
$wpdb->query( 'START TRANSACTION' );

$insert = static function ( $content, $fields = array() ) {
	return wp_insert_post(
		$fields + array(
			'post_type'    => 'wp_block',
			'post_status'  => 'publish',
			'post_title'   => 'Pattern',
			'post_content' => $content,
		)
	);
};
$block = static fn( $ref ) => '<!-- wp:block {"ref":' . $ref . '} /-->';

$plain    = $insert( "<!-- wp:paragraph -->\n<p>Synced <em>text</em></p>\n<!-- /wp:paragraph -->\n\n<!-- wp:heading -->\n<h2 class=\"wp-block-heading\">Title</h2>\n<!-- /wp:heading -->" );
$draft    = $insert( '<!-- wp:paragraph --><p>Draft</p><!-- /wp:paragraph -->', array( 'post_status' => 'draft' ) );
$locked   = $insert( '<!-- wp:paragraph --><p>Locked</p><!-- /wp:paragraph -->', array( 'post_password' => 'secret' ) );
$looped   = $insert( '' );
wp_update_post( array( 'ID' => $looped, 'post_content' => '<!-- wp:paragraph --><p>Loop</p><!-- /wp:paragraph -->' . $block( $looped ) ) );
$nested   = $insert( '<!-- wp:group {"layout":{"type":"constrained"}} --><div class="wp-block-group">' . $block( $plain ) . '</div><!-- /wp:group -->' . $block( $plain ) );
$freeform = $insert( "Plain text\n\nand more" );
$page     = wp_insert_post( array( 'post_type' => 'page', 'post_status' => 'publish', 'post_title' => 'Page', 'post_content' => '<!-- wp:paragraph --><p>Page</p><!-- /wp:paragraph -->' ) );

foreach ( array( $plain, $draft, $locked, $looped, $nested, $freeform, $page ) as $id ) {
	$post = get_post( $id );
	echo 'post', "\t", $id, "\t", $post->post_type, "\t", $post->post_status, "\t", bin2hex( $post->post_password ), "\t", bin2hex( $post->post_content ), "\n";
}

$markups = array(
	$block( $plain ),
	$block( $draft ),
	$block( $locked ),
	$block( $looped ),
	$block( $nested ),
	$block( $freeform ),
	$block( $page ),
	$block( 0 ),
	'<!-- wp:block /-->',
	'<!-- wp:block {"ref":"' . $plain . '"} /-->',
	'<!-- wp:block {"ref":' . $plain . '.0,"className":"x"} /-->',
	'<!-- wp:block {"ref":99999999} /-->',
	'<!-- wp:group --><div class="wp-block-group">' . $block( $nested ) . '</div><!-- /wp:group -->',
);
foreach ( $markups as $markup ) {
	echo 'block', "\t", bin2hex( $markup ), "\t", bin2hex( render_block( parse_blocks( $markup )[0] ) ), "\n";
}

$wpdb->query( 'ROLLBACK' );
