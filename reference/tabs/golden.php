<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints tabs blocks rendered in order in one request, as the tab panels'
// counters carry over between them. Lines are
//   counter<TAB>hex(markup)<TAB>hex(html)
// where counter is the last number wp_unique_id() gave before the case.
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$button = static fn( $label ) => '<button type="button" role="tab" class="wp-block-tab-list__tab">' . $label . '</button>';
$panel  = static fn( $attrs, $text, $id = '' ) => '<!-- wp:tab-panel ' . $attrs . ' --><section' . $id . ' role="tabpanel" class="wp-block-tab-panel"><!-- wp:paragraph --><p>' . $text . '</p><!-- /wp:paragraph --></section><!-- /wp:tab-panel -->';
$list   = static fn( $attrs, $buttons ) => '<!-- wp:tab-list ' . $attrs . ' --><div role="tablist" class="wp-block-tab-list">' . implode( '', $buttons ) . '</div><!-- /wp:tab-list -->';
$tabs   = static fn( $attrs, $inner ) => '<!-- wp:tabs ' . $attrs . ' --><div class="wp-block-tabs">' . $inner . '</div><!-- /wp:tabs -->';
$panels = static fn( $inner ) => '<!-- wp:tab-panels --><div class="wp-block-tab-panels">' . $inner . '</div><!-- /wp:tab-panels -->';

$two = $panels( $panel( '{"label":"One"}', 'A' ) . $panel( '{"label":"Two"}', 'B' ) );
$markups = array(
	$tabs( '{}', $list( '{}', array( $button( 'One' ), $button( 'Two' ) ) ) . $two ),
	$tabs( '{"anchor":"my","activeTabIndex":1}', $list( '{"ariaLabel":"<b>Pick</b> one"}', array( $button( 'One' ), $button( 'Two' ) ) ) . $panels( $panel( '{"anchor":"first"}', 'A', ' id="first"' ) . $panel( '{}', 'B', ' id="mine"' ) . $panel( '{}', 'C' ) ) ),
	$tabs( '{"anchor":"my"}', $list( '{}', array( $button( 'One' ) ) ) . $two ),
	$tabs( '{"activeTabIndex":"2","className":"is-style-x","backgroundColor":"accent-1"}', $list( '{"ariaLabel":""}', array( $button( 'One' ), $button( 'Two' ), $button( 'Three' ) ) ) . $two ),
	$tabs( '{}', $list( '{}', array( $button( 'One' ) ) ) ),
	$tabs( '{"anchor":""}', $list( '{}', array( $button( 'One' ) ) ) . $two ),
	$tabs( '{"anchor":"0"}', $two ),
	$tabs( '{"anchor":7}', $two ),
	$tabs( '{}', $two . $panels( $panel( '{}', 'Z' ) ) . $list( '{}', array( $button( 'One' ) ) ) ),
	$tabs( '{}', $list( '{}', array( $button( 'Outer' ) ) ) . $panels( $panel( '{}', 'X' ) . '<!-- wp:tab-panel --><section role="tabpanel" class="wp-block-tab-panel">' . $tabs( '{}', $list( '{}', array( $button( 'Inner' ) ) ) . $two ) . '</section><!-- /wp:tab-panel -->' ) ),
	$tabs( '{}', '<div class="other"></div>' . $list( '{}', array() ) . $two ),
	$list( '{}', array( $button( 'Alone' ) ) ),
	$panel( '{}', 'Alone' ),
	$panel( '{}', 'Again' ),
	'<!-- wp:tab-panel --><div>No class</div><!-- /wp:tab-panel -->',
	'<!-- wp:tab-list --><div>No class</div><!-- /wp:tab-list -->',
	'<!-- wp:tab-panel --><section id class="wp-block-tab-panel"></section><!-- /wp:tab-panel -->',
	'<!-- wp:tab-panel --><section id="0" class="wp-block-tab-panel"></section><!-- /wp:tab-panel -->',
);
foreach ( $markups as $markup ) {
	$counter = (int) wp_unique_id();
	echo $counter, "\t", bin2hex( $markup ), "\t", bin2hex( render_block( parse_blocks( $markup )[0] ) ), "\n";
}
