<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's Plugins screen actions (MVP 11.3), loaded beside
// actions.php: plugins.php's `activate` and `deactivate` cases, taken from
// WordPress's own source and run over the request hs-admin's handlers get,
// past their checks, on the plugin the test extension stands in for
// (reference/plugins/hyscribe-test), which each case puts in the plugins
// folder and takes out after, loaded as wp-settings.php loads it when it
// is active. `active_plugins`, `recently_activated` and the plugin's own
// `ext_hooks` are set as each case starts from; an activation whose hook
// ends in a fatal error writes what it wrote before it.

$plugins_file = 'hyscribe-test/hyscribe-test.php';

// plugins.php's case `$action` on `$plugin`, as the administrator's request,
// with the variables plugins.php sets before it; activate_plugin()'s early
// redirect for a fatal error, a header only, is left out, as the oracle's
// redirects end the case.
$plugins_run = function ( $action, $plugin ) use ( $admin_code, $admin_run ) {
	set_current_screen( 'plugins' );
	$_GET     = array(
		'action'        => $action,
		'plugin'        => $plugin,
		'plugin_status' => 'all',
		'paged'         => '1',
		's'             => '',
		'_wpnonce'      => wp_create_nonce( "$action-plugin_$plugin" ),
	);
	$_REQUEST = $_GET;
	$code     = $admin_code( 'plugins.php', "/\t\tcase '$action':\n(.*?)\n\t\tcase '/s" );
	$code     = str_replace( "self_admin_url( 'plugins.php?error=true&plugin=' . urlencode( \$plugin ) )", "''", $code );
	$vars     = '$plugin = wp_unslash( $_REQUEST["plugin"] ); $status = "all"; $page = 1; $s = ""; ';
	$admin_run( fn() => eval( $vars . $code ) );
};

// A case: the plugin installed, the options as given, then the action; the
// plugin's folder removed once the case has run.
$plugins_case = function ( $active, $recent, $action, $hooks = null ) use ( $plugins_file, $plugins_run ) {
	return array(
		'setup'  => function () use ( $active, $recent, $hooks ) {
			$folder = WP_PLUGIN_DIR . '/hyscribe-test';
			wp_mkdir_p( $folder );
			copy( __DIR__ . '/plugins/hyscribe-test.php', "$folder/hyscribe-test.php" );
			wp_clean_plugins_cache();
			update_option( 'active_plugins', $active );
			if ( null !== $recent ) {
				update_option( 'recently_activated', $recent, false );
			}
			if ( null !== $hooks ) {
				update_option( 'ext_hooks', $hooks );
			}
		},
		'action' => function () use ( $action, $active, $plugins_file, $plugins_run ) {
			if ( in_array( $plugins_file, $active, true ) ) {
				include_once WP_PLUGIN_DIR . "/$plugins_file";
			}
			try {
				$plugins_run( $action, $plugins_file );
			} catch ( Error $fatal ) {
				unset( $fatal );
			}
			unlink( WP_PLUGIN_DIR . '/hyscribe-test/hyscribe-test.php' );
			rmdir( WP_PLUGIN_DIR . '/hyscribe-test' );
		},
	);
};

// A view of the Plugins screen: its list's prepare_items(), which keeps
// what recently_activated holds of the last week.
$plugins_view = function () {
	set_current_screen( 'plugins' );
	_get_list_table( 'WP_Plugins_List_Table', array( 'screen' => 'plugins' ) )->prepare_items();
};

$plugins_recent = array(
	$plugins_file => 4102444800,
	'old/old.php' => 1000000000,
);

return array(
	'admin-plugins-activate'            => $plugins_case( array(), null, 'activate' ),
	'admin-plugins-activate-sorted'     => $plugins_case( array( 'wordfence/wordfence.php' ), null, 'activate' ),
	'admin-plugins-activate-recent'     => $plugins_case( array(), $plugins_recent, 'activate' ),
	'admin-plugins-deactivate'          => $plugins_case( array( $plugins_file ), null, 'deactivate' ),
	'admin-plugins-deactivate-kept'     => $plugins_case( array( $plugins_file, 'wordfence/wordfence.php' ), null, 'deactivate' ),
	'admin-plugins-deactivate-inactive' => $plugins_case( array(), $plugins_recent, 'deactivate' ),
	'admin-plugins-view'                => array( 'action' => $plugins_view ),
	'admin-plugins-view-recent'         => array(
		'setup'  => function () use ( $plugins_recent ) {
			update_option( 'recently_activated', $plugins_recent + array( 'x/x.php' => 'soon' ), false );
		},
		'action' => $plugins_view,
	),
);
