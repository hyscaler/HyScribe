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
		'_multiwidget' => 1,
	),
	'widget_tag_cloud' => 'plain',
	'sidebars_widgets' => array(
		'sidebar-1'           => array( 'search-5', 'block-2' ),
		'wp_inactive_widgets' => array( 'search-2', 'search-3', 'search-4', 'search-5', 'search-9', 'search-02' ),
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
	$block( array( 'id' => 'nowhere-1' ) ),
	$block( array() ),
	$block( array( 'idBase' => 'search' ) ),
	$block( array( 'idBase' => 'nope' ) ),
	$block( array( 'idBase' => 'rss' ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( serialize( array( 'title' => 'Signed "title"' ) ) ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( serialize( array( 'title' => 'Forged' ) ), 'bad' ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( serialize( 'title=From+string' ) ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( serialize( 7 ) ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( serialize( null ) ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( serialize( false ) ) ) ),
	$block( array( 'idBase' => 'search', 'instance' => $signed( 'not serialized' ) ) ),
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
$wpdb->query( 'ROLLBACK' );
