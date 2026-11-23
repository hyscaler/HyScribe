<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for accordion markup on the home page. Lines are
//   hex(markup)<TAB>wp_unique_id() counter before<TAB>hex(html)
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

function heading( $title ) {
	return '<!-- wp:accordion-heading --><h3 class="wp-block-accordion-heading"><button class="wp-block-accordion-heading__toggle"><span class="wp-block-accordion-heading__toggle-title">' . $title . '</span><span class="wp-block-accordion-heading__toggle-icon" aria-hidden="true">+</span></button></h3><!-- /wp:accordion-heading -->';
}
function panel( $inner ) {
	return '<!-- wp:accordion-panel --><div role="region" class="wp-block-accordion-panel">' . $inner . '</div><!-- /wp:accordion-panel -->';
}
function item( $attrs, $inner ) {
	$json = $attrs ? ' ' . wp_json_encode( $attrs ) : '';
	return "<!-- wp:accordion-item$json --><div class=\"wp-block-accordion-item\">$inner</div><!-- /wp:accordion-item -->";
}
function accordion( $attrs, $inner ) {
	$json = $attrs ? ' ' . wp_json_encode( $attrs ) : '';
	return "<!-- wp:accordion$json --><div role=\"group\" class=\"wp-block-accordion\">$inner</div><!-- /wp:accordion -->";
}

$text  = '<!-- wp:paragraph --><p>Answer <img src="a.png" alt=""></p><!-- /wp:paragraph -->';
$one   = item( array(), heading( 'Question' ) . panel( $text ) );
$open  = item( array( 'openByDefault' => true ), heading( 'Open' ) . panel( $text ) );
$cases = array(
	accordion( array(), $one ),
	accordion( array( 'autoclose' => true ), $one . $open ),
	accordion( array(), $open . $one . $one ),
	accordion( array(), item( array(), heading( 'No panel' ) ) ),
	accordion( array(), item( array(), panel( $text ) ) ),
	accordion( array(), '<!-- wp:accordion-item --><section class="other">' . heading( 'Other' ) . panel( $text ) . '</section><!-- /wp:accordion-item -->' ),
	accordion( array(), '' ),
	'<!-- wp:accordion --><div class="other">' . $one . '</div><!-- /wp:accordion -->',
	'<!-- wp:accordion --><!-- /wp:accordion -->',
	'<!-- wp:accordion-item --><!-- /wp:accordion-item -->',
	$one,
	accordion( array( 'className' => 'is-style-x', 'showIcon' => false ), $open ),
);
foreach ( $cases as $markup ) {
	$counter = (int) wp_unique_id();
	echo bin2hex( $markup ), "\t", $counter, "\t", bin2hex( do_blocks( $markup ) ), "\n";
}
