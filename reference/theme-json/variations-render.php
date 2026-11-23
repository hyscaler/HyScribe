<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for blocks with style variation classes on Twenty
// Twenty-Five, with the synthetic variations of variations.json (read from
// stdin) as the site's global styles. Lines are
//   hex(markup)<TAB>wp_unique_id() counter<TAB>hex(html)<TAB>hex(block-style-variation-styles CSS)
// Only pure rendering runs; the database is only read.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$synthetic = json_decode( file_get_contents( 'php://stdin' ), true );
foreach ( $synthetic['styles'] as list( $block, $style ) ) {
	register_block_style( $block, array( 'name' => $style, 'label' => $style ) );
}
add_filter(
	'wp_theme_json_data_user',
	static function ( $data ) use ( $synthetic ) {
		return $data->update_with( $synthetic['theme'] );
	}
);
WP_Theme_JSON_Resolver::clean_cached_data();

$cases = array(
	'<!-- wp:paragraph {"className":"is-style-text-display"} --><p class="is-style-text-display">Hi</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"className":"is-style-unknown"} --><p class="is-style-unknown">Hi</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"className":"is-style-foo--3"} --><p>Hi</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"className":"is-style-foo--3x is-style-a-b--12"} --><p>Hi</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"className":5} --><p>Hi</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"className":""} --><p>Hi</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"className":"is-style-text-display"} --><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"className":"is-style-text-display"} -->Hi<!-- /wp:paragraph -->',
	'<!-- wp:group {"className":"is-style-section-1","layout":{"type":"constrained"}} --><div class="wp-block-group is-style-section-1"><!-- wp:paragraph {"className":"is-style-text-annotation"} --><p class="is-style-text-annotation">x</p><!-- /wp:paragraph --><!-- wp:paragraph {"className":"is-style-text-annotation"} --><p class="is-style-text-annotation">y</p><!-- /wp:paragraph --></div><!-- /wp:group -->',
	'<!-- wp:separator {"className":"is-style-wide"} --><hr class="wp-block-separator has-alpha-channel-opacity is-style-wide"/><!-- /wp:separator -->',
	'<!-- wp:heading {"className":"is-style-text-subtitle"} --><h2 class="wp-block-heading is-style-text-subtitle">T</h2><!-- /wp:heading -->',
	'<!-- wp:quote {"className":"is-style-hs-empty"} --><blockquote class="wp-block-quote is-style-hs-empty"><!-- wp:paragraph --><p>Q</p><!-- /wp:paragraph --></blockquote><!-- /wp:quote -->',
	'<!-- wp:group {"className":"is-style-hs-test"} --><div class="wp-block-group is-style-hs-test"><!-- wp:paragraph --><p>G</p><!-- /wp:paragraph --></div><!-- /wp:group -->',
);

foreach ( $cases as $markup ) {
	wp_styles()->remove( 'block-style-variation-styles' );
	$counter = (int) wp_unique_id();
	$html    = do_blocks( $markup );
	$css     = wp_styles()->get_data( 'block-style-variation-styles', 'after' );
	echo bin2hex( $markup ), "\t", $counter, "\t", bin2hex( $html ), "\t", bin2hex( $css ? implode( "\n", $css ) : '' ), "\n";
}
