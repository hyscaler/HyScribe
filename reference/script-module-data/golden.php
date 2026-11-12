<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Renders the blocks of case CASE in one request, then prints the script
// module data. One line:
//   counter<TAB>hex(markup)<TAB>hex(data)
// where counter is the last number wp_unique_id() gave before the blocks.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$accordion = '<!-- wp:accordion --><div class="wp-block-accordion"><!-- wp:accordion-item --><div class="wp-block-accordion-item"><!-- wp:accordion-heading --><h3 class="wp-block-accordion-heading"><button class="wp-block-accordion-heading__toggle">Q</button></h3><!-- /wp:accordion-heading --></div><!-- /wp:accordion-item --></div><!-- /wp:accordion -->';
$panels    = '<!-- wp:tab-panels --><div class="wp-block-tab-panels"><!-- wp:tab-panel --><section class="wp-block-tab-panel"></section><!-- /wp:tab-panel --><!-- wp:tab-panel {"anchor":"b<i>"} --><section class="wp-block-tab-panel"></section><!-- /wp:tab-panel --></div><!-- /wp:tab-panels -->';
$tabs      = static fn( $attrs ) => '<!-- wp:tabs ' . $attrs . ' --><div class="wp-block-tabs">' . $panels . '</div><!-- /wp:tabs -->';
$track     = static fn( $attrs ) => '<!-- wp:playlist-track ' . wp_json_encode( $attrs ) . ' /-->';
$playlist  = static fn( $inner ) => '<!-- wp:playlist --><figure class="wp-block-playlist">' . $inner . '</figure><!-- /wp:playlist -->';
$tracks    = $track( array( 'id' => 1, 'src' => 'https://x.test/a b.mp3', 'title' => '<b>Song</b> </p>', 'artist' => 'Band', 'album' => 'LP <i>2</i>', 'image' => 'javascript:alert(1)', 'imageAlt' => 'A < B' ) )
	. $track( array( 'id' => 2, 'title' => '0', 'artist' => 'Solo', 'album' => '0' ) )
	. $track( array( 'id' => 3, 'artist' => 'X', 'album' => 'Y', 'src' => '/rel.ogg' ) );

$cases = array(
	array( $accordion ),
	array( $tabs( '{}' ), $tabs( '{"anchor":"my"}' ), $tabs( '{"anchor":7}' ) ),
	array( $playlist( $tracks ) ),
	array( $accordion, $tabs( '{}' ), $playlist( $tracks ), $accordion, $playlist( $track( array( 'id' => 9 ) ) ) ),
	array( $tabs( '{"anchor":""}' ) ),
	array( $playlist( $track( array( 'title' => 'No id' ) ) ) ),
	array( '<!-- wp:paragraph --><p>Plain</p><!-- /wp:paragraph -->' ),
);

$markup  = implode( '', $cases[ (int) getenv( 'CASE' ) ] );
$counter = (int) wp_unique_id();
foreach ( parse_blocks( $markup ) as $block ) {
	render_block( $block );
}
ob_start();
wp_script_modules()->print_script_module_data();
$data = ob_get_clean();
echo $counter, "\t", bin2hex( $markup ), "\t", bin2hex( $data ), "\n";
