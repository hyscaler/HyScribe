<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for the legacy widget block, over widget options set inside a
// transaction that is rolled back, with wp_hash() signing under the fixed
// salt `test-salt`. Lines are
//   option<TAB>name<TAB>hex(stored value)
//   render<TAB>hex(search query)<TAB>hex(markup)<TAB>hex(do_blocks( markup ))
define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';
global $wpdb, $wp_widget_factory;
$wpdb->query( 'START TRANSACTION' );
add_filter( 'salt', static fn() => 'test-salt' );

$options = array(
	'widget_search'    => array(
		2              => array( 'title' => 'Find & <b>it</b>' ),
		3              => array(),
		4              => array( 'title' => '0' ),
		5              => array( 'title' => 'Elsewhere' ),
		6              => null,
		7              => false,
		'1.5'          => array( 'title' => 'Half' ),
		'_multiwidget' => 1,
	),
	'widget_tag_cloud' => 'plain',
	'widget_custom_html' => array(
		2              => array( 'title' => 'Stored', 'content' => '<p>Stored <em>HTML</em></p>' ),
		'_multiwidget' => 1,
	),
	'users_can_register' => '1',
	'sidebars_widgets' => array(
		'sidebar-1'           => array( 'search-5', 'block-2' ),
		'wp_inactive_widgets' => array( 'search-2', 'search-3', 'search-4', 'search-5', 'search-9', 'search-02', 'custom_html-2', 'search-6', 'search-7', 'search-1.5' ),
		'array_version'       => 3,
	),
);
foreach ( $options as $name => $value ) {
	update_option( $name, $value );
	echo 'option', "\t", $name, "\t", bin2hex( maybe_serialize( $value ) ), "\n";
}
// A request reads these fresh: the widgets register their instances on
// init, and the sidebars were cached before the update.
foreach ( $wp_widget_factory->widgets as $widget ) {
	$widget->_register();
}
$GLOBALS['_wp_sidebars_widgets'] = array();

$signed = static function ( $serialized, $hash = null ) {
	return array(
		'encoded' => base64_encode( $serialized ),
		'hash'    => $hash ?? wp_hash( $serialized ),
	);
};
$block  = static fn( $attrs ) => '<!-- wp:legacy-widget ' . wp_json_encode( $attrs ) . ' /-->';
$cases  = array(
	$block( array( 'id' => 'search-2' ) ),
	$block( array( 'id' => 'search-3' ) ),
	$block( array( 'id' => 'search-4' ) ),
	$block( array( 'id' => 'search-5' ) ),
	$block( array( 'id' => 'search-9' ) ),
	$block( array( 'id' => 'search-02' ) ),
	$block( array( 'id' => 'block-2' ) ),
	$block( array( 'id' => 'custom_html-2' ) ),
	$block( array( 'id' => 'search-6' ) ),
	$block( array( 'id' => 'search-7' ) ),
	$block( array( 'id' => 'search-1.5' ) ),
	$block( array( 'id' => 'nowhere-1' ) ),
	$block( array() ),
	$block( array( 'idBase' => 'search' ) ),
	$block( array( 'idBase' => 'nope' ) ),
	$block( array( 'idBase' => 'rss' ) ),
	$block( array( 'idBase' => 'custom_html' ) ),
	$block( array( 'idBase' => 'custom_html', 'instance' => $signed( serialize( array( 'title' => 'Some "HTML"', 'content' => '<p>Hi <b>there</p>' ) ) ) ) ),
	$block( array( 'idBase' => 'custom_html', 'instance' => $signed( serialize( array( 'title' => '0', 'content' => 'x' ) ) ) ) ),
	$block( array( 'idBase' => 'meta' ) ),
	$block( array( 'idBase' => 'meta', 'instance' => $signed( serialize( array( 'title' => 'Links & <i>more</i>' ) ) ) ) ),
	$block( array( 'idBase' => 'text' ) ),
	$block( array( 'idBase' => 'text', 'instance' => $signed( serialize( array( 'title' => 'Plain', 'text' => "a\n\nb 'quoted'" ) ) ) ) ),
	$block( array( 'idBase' => 'text', 'instance' => $signed( serialize( array( 'text' => "a\n\nb <b>bold", 'filter' => true ) ) ) ) ),
	$block( array( 'idBase' => 'text', 'instance' => $signed( serialize( array( 'text' => "\"Quotes\" (c) wordpress :)\n\nline", 'filter' => true, 'visual' => true ) ) ) ) ),
	$block( array( 'idBase' => 'text', 'instance' => $signed( serialize( array( 'text' => "one\n\ntwo", 'filter' => 'content' ) ) ) ) ),
	$block( array( 'idBase' => 'text', 'instance' => $signed( serialize( array( 'text' => "one\n\ntwo :)", 'visual' => true, 'filter' => false ) ) ) ) ),
	$block( array( 'idBase' => 'text', 'instance' => $signed( serialize( array( 'text' => '<span  width="5">x</span><iframe width="" style="width:px;max-width: 7px" src="y">', 'filter' => false ) ) ) ) ),
	$block( array( 'idBase' => 'text', 'instance' => $signed( serialize( array( 'text' => '<iframe width="560" height="315" style="width: 560px;" src="x"></iframe><VIDEO class="v" width="1">', 'filter' => false ) ) ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( serialize( array( 'title' => 'Signed "title"' ) ) ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( serialize( array( 'title' => 'Forged' ) ), 'bad' ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( serialize( 'title=From+string' ) ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( serialize( 7 ) ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( serialize( null ) ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( serialize( false ) ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( 'not serialized' ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( serialize( array( 'title' => 'Trailing' ) ) . 'junk' ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( 'O:8:"stdClass":1:{s:5:"title";s:6:"Object";}' ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => array( 'encoded' => base64_encode( serialize( array( 'title' => 'Unsigned' ) ) ) ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => array( 'encoded' => base64_encode( serialize( array( 'title' => 'Null hash' ) ) ), 'hash' => null ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => array( 'encoded' => null, 'hash' => wp_hash( '' ) ) ) ),
);
foreach ( array( '', 'x"y <z>' ) as $search ) {
	set_query_var( 's', $search );
	foreach ( $cases as $markup ) {
		echo 'render', "\t", bin2hex( $search ), "\t", bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
	}
}
// Registration closed and tags balanced.
set_query_var( 's', '' );
foreach ( array( 'users_can_register' => '0', 'use_balanceTags' => '1' ) as $name => $value ) {
	update_option( $name, $value );
	echo 'option', "\t", $name, "\t", bin2hex( $value ), "\n";
}
$later = array(
	$block( array( 'idBase' => 'meta' ) ),
	$block( array( 'idBase' => 'custom_html', 'instance' => $signed( serialize( array( 'content' => '<p>Hi <b>there</p>' ) ) ) ) ),
	$block( array( 'idBase' => 'text', 'instance' => $signed( serialize( array( 'text' => '<div>open <em>x', 'filter' => false ) ) ) ) ),
	$block( array( 'idBase' => 'text', 'instance' => $signed( serialize( array( 'text' => '<embed width="5" style="width: 5em; max-width:600px;height:9px">', 'filter' => false ) ) ) ) ),
);
foreach ( $later as $markup ) {
	echo 'render', "\t", '', "\t", bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
}
$wpdb->query( 'ROLLBACK' );
