<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints the site logo block with the `site_logo` option set by filter to
// each of the fixture's first attachments and to a few non-images, on the
// front page. Prefixed IDs vary one thing: "mod:" gives the logo by the
// theme's `custom_logo` mod instead, "noalt:" empties its alt text and sets a
// blog name to texturize, "post:" renders on a single post. The fixture's content is not ours to commit, so lines hold a
// hash:
//   counter<TAB>case<TAB>hex(markup)<TAB>sha256(html)
// where counter is the last number wp_unique_id() gave before the case.
// Each case starts a fresh request's high priority image flag.
// With RAW=1 the html itself is printed instead. Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$raw     = getenv( 'RAW' );
$ids     = get_posts( array( 'post_type' => 'attachment', 'post_status' => 'any', 'numberposts' => 12, 'orderby' => 'ID', 'order' => 'ASC', 'fields' => 'ids' ) );
$ids     = array_merge( $ids, array( 1, 999999, 0, 'none', 'mod:611', 'noalt:611', 'post:611' ) );
$markups = array(
	'<!-- wp:site-logo /-->',
	'<!-- wp:site-logo {"isLink":false} /-->',
	'<!-- wp:site-logo {"width":120} /-->',
	'<!-- wp:site-logo {"width":"75","linkTarget":"_blank"} /-->',
	'<!-- wp:site-logo {"linkTarget":"_self","className":"is-style-rounded","align":"center"} /-->',
);

foreach ( $ids as $case ) {
	list( $kind, $id ) = str_contains( $case, ':' ) ? explode( ':', $case ) : array( '', $case );
	$filters = array(
		'pre_option_site_logo' => static fn() => 'none' === $id || 'mod' === $kind ? false : (string) $id,
	);
	if ( 'mod' === $kind ) {
		$filters['option_theme_mods_twentytwentyfive'] = static fn( $mods ) => array_merge( (array) $mods, array( 'custom_logo' => (int) $id ) );
	}
	if ( 'noalt' === $kind ) {
		$filters['get_post_metadata']    = static fn( $value, $object, $key ) => '_wp_attachment_image_alt' === $key ? array( '' ) : $value;
		$filters['pre_option_blogname'] = static fn() => 'Tom\'s "Logo" <b>&</b> co -- 1';
	}
	foreach ( $filters as $hook => $filter ) {
		add_filter( $hook, $filter, 10, 3 );
	}
	wp( 'post' === $kind ? 'p=1' : '' );
	foreach ( $markups as $markup ) {
		wp_high_priority_element_flag( true );
		$counter = (int) wp_unique_id();
		$html    = render_block( parse_blocks( $markup )[0] );
		echo $counter, "\t", $case, "\t", bin2hex( $markup ), "\t", $raw ? $html : hash( 'sha256', $html ), "\n";
	}
	foreach ( $filters as $hook => $filter ) {
		remove_filter( $hook, $filter, 10 );
	}
}
