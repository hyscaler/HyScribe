<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints playlist blocks. Lines are
//   counter<TAB>hex(markup)<TAB>hex(html)
// where counter is the last number wp_unique_id() gave before the case.
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$track    = static fn( $attrs ) => '<!-- wp:playlist-track ' . wp_json_encode( $attrs ) . ' /-->';
$playlist = static fn( $attrs, $inner, $figure = '<figure class="wp-block-playlist">' ) => '<!-- wp:playlist ' . wp_json_encode( $attrs ) . ' -->' . $figure . '<ol class="wp-block-playlist__tracklist">' . $inner . '</ol></figure><!-- /wp:playlist -->';
$full     = array( 'id' => 7, 'src' => 'https://x.test/a.mp3', 'title' => 'Song <b>1</b>', 'artist' => 'Band & co', 'album' => 'LP', 'image' => 'https://x.test/a.jpg', 'imageAlt' => 'Cover "A"', 'length' => '3:05' );
$bare     = array( 'id' => 8 );
$tracks   = $track( $full ) . $track( $bare ) . $track( array( 'title' => 'No id' ) ) . $track( array( 'id' => 9, 'title' => '0', 'artist' => '0', 'length' => '0', 'image' => '0' ) );

$markups = array(
	$playlist( new stdClass(), $tracks ),
	$playlist( array( 'waveformColor' => '${1x${' ), $track( $bare ) ),
	$playlist( array( 'waveformColor' => '$01|$10' ), $track( $bare ) ),
	'<!-- wp:playlist --><ol>' . $track( $bare ) . '</ol><figure<!-- /wp:playlist -->',
	$playlist( array( 'showImages' => false, 'showPlayButtonArtwork' => true, 'waveformStyle' => 'line' ), $tracks ),
	$playlist( array( 'waveformColor' => '#f00', 'waveformGradient' => 'linear-gradient(red,"blue")', 'waveformBackgroundColor' => '0', 'waveformBackgroundGradient' => 'x', 'waveformStyle' => 'bogus' ), $track( $bare ) ),
	$playlist( array( 'waveformColor' => 'a$1b\1c${1}d\$1e\\\\f$9g${12}h$' ), $track( $bare ) ),
	$playlist( new stdClass(), $track( array( 'title' => 'No id' ) ) ),
	$playlist( new stdClass(), $track( $bare ), '<div class="no-figure">' ),
	$playlist( new stdClass(), $track( $bare ) . $track( $full ), "<figure\nclass=\"wp-block-playlist x\" data-a='>'>" ),
	$playlist( new stdClass(), '<p>no tracks</p>' ),
	$track( $full ),
	$track( array( 'id' => 0, 'title' => 'Zero' ) ),
	$track( array( 'id' => 3, 'className' => 'c', 'style' => array( 'color' => array( 'text' => '#123' ) ) ) ),
);
foreach ( $markups as $markup ) {
	$counter = (int) wp_unique_id();
	echo $counter, "\t", bin2hex( $markup ), "\t", bin2hex( render_block( parse_blocks( $markup )[0] ) ), "\n";
}
