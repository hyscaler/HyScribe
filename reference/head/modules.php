<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for the script modules hyscribe-render's wp_head() prints. Each
// line is hex(json scenario)<TAB>hex(output), where a scenario has the
// site URL, the modules registered (id, src, deps, version, in_footer,
// fetchpriority) and the queue, and the output is what print_import_map(),
// print_head_enqueued_script_modules() and print_script_module_preloads()
// print, in that order. Random scenarios use a fixed seed.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

mt_srand( 20261028 );

$ids        = array( 'a', 'b', 'c', 'd', 'e', '@x/f' );
$priorities = array( 'auto', 'low', 'high' );
$versions   = array( '1.0', false, null, '', '2 3' );
$scenarios  = array();
for ( $n = 0; $n < 400; $n++ ) {
	$modules = array();
	foreach ( $ids as $id ) {
		if ( mt_rand( 0, 5 ) === 0 ) {
			continue;
		}
		// Only earlier IDs, as PHP recurses forever on a cycle.
		$deps = array();
		foreach ( array_slice( $ids, 0, array_search( $id, $ids, true ) ) as $dep ) {
			if ( mt_rand( 0, 2 ) === 0 ) {
				$deps[] = array(
					'id'     => mt_rand( 0, 6 ) ? $dep : 'missing',
					'import' => mt_rand( 0, 2 ) ? 'static' : 'dynamic',
				);
			}
		}
		$modules[] = array(
			'id'            => $id,
			'src'           => mt_rand( 0, 8 ) ? ( mt_rand( 0, 9 ) ? "/js/$id.js" : "/js/<$id>\"q\".js" ) : '',
			'deps'          => $deps,
			'version'       => $versions[ mt_rand( 0, 4 ) ],
			'in_footer'     => (bool) mt_rand( 0, 1 ),
			'fetchpriority' => $priorities[ mt_rand( 0, 2 ) ],
		);
	}
	$queue = array();
	foreach ( $ids as $id ) {
		if ( mt_rand( 0, 2 ) === 0 ) {
			$queue[] = $id;
		}
	}
	$scenarios[] = array( 'modules' => $modules, 'queue' => $queue );
}
foreach ( $scenarios as $scenario ) {
	$registry = new WP_Script_Modules();
	foreach ( $scenario['modules'] as $module ) {
		$registry->register(
			$module['id'],
			'' === $module['src'] ? '' : site_url( $module['src'] ),
			$module['deps'],
			$module['version'],
			array(
				'in_footer'     => $module['in_footer'],
				'fetchpriority' => $module['fetchpriority'],
			)
		);
	}
	foreach ( $scenario['queue'] as $id ) {
		$registry->enqueue( $id );
	}
	ob_start();
	@$registry->print_import_map();
	@$registry->print_head_enqueued_script_modules();
	@$registry->print_script_module_preloads();
	$output = ob_get_clean();
	$scenario['siteurl'] = site_url();
	echo bin2hex( wp_json_encode( $scenario ) ), "\t", bin2hex( $output ), "\n";
}
