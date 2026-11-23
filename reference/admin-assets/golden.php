<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-admin's style and script printing: print_admin_styles(),
// print_head_scripts() and print_footer_scripts() over sets of enqueued
// handles, with concatenation on and off, on the registry an admin request has.
// URLs and nonces are replaced by registry.json's placeholders. One case per
// line:
//   1 or 0 (concatenate) <TAB> styles, comma-separated <TAB> scripts <TAB> hex(output)
define( 'DISABLE_WP_CRON', true );
define( 'WP_ADMIN', true );
require '/var/www/html/wp-load.php';

function placeholders( $text ) {
	$text = str_replace( array( '"user_id":"0"', '"uid":"0"' ), array( '"user_id":"%user_id%"', '"uid":"%user_id%"' ), $text );
	$text = preg_replace( '/"time":"\d+"/', '"time":"%time%"', $text );
	$text = str_replace( wp_create_nonce( 'reset-password-for-0' ), '%nonce:reset-password-for-user%', $text );
	return str_replace(
		array( rest_url(), admin_url(), '"' . admin_url( '', 'relative' ), site_url(), wp_create_nonce( 'wp_rest' ) ),
		array( '%rest_url%', '%admin_url%', '"%admin_path%', '%site_url%', '%nonce:wp_rest%' ),
		$text
	);
}

$cases = array(
	array( 'login', 'user-profile,wp-tooltip' ),
	array( 'dashicons,buttons', 'jquery' ),
	array( 'wp-block-library', 'wp-util,heartbeat' ),
	array( 'common,forms,admin-menu', 'common,utils,svg-painter' ),
	array( 'nope', 'nope,jquery-core' ),
	array( 'colors', 'wp-api-fetch' ),
	array( 'login,wp-edit-blocks', 'wp-edit-post' ),
	array( '', 'comment-reply,admin-bar' ),
	array( 'thickbox,media-views', 'media-editor,thickbox' ),
	array( 'customize-controls', 'customize-controls' ),
	array( 'wp-block-paragraph,wp-block-button', 'wp-tooltip' ),
	array( 'wp-embed-template-ie,wp-view-transitions-admin,open-sans', 'json2,wp-embed,mediaelement' ),
	array( 'common', 'postbox,mediaelement,comment-reply' ),
	array( 'common', 'underscore,code-editor' ),
);
foreach ( $cases as list( $styles, $scripts ) ) {
	foreach ( array( true, false ) as $concatenate ) {
		foreach ( array( wp_styles(), wp_scripts() ) as $registry ) {
			$registry->queue = $registry->to_do = $registry->done = $registry->groups = $registry->args = array();
			$registry->concat = $registry->concat_version = $registry->print_code = $registry->print_html = '';
			$registry->ext_handles = $registry->ext_version = '';
		}
		wp_scripts()->in_footer = array();
		$GLOBALS['concatenate_scripts'] = $concatenate;
		foreach ( array_filter( explode( ',', $styles ) ) as $handle ) {
			wp_enqueue_style( $handle );
		}
		foreach ( array_filter( explode( ',', $scripts ) ) as $handle ) {
			wp_enqueue_script( $handle );
		}
		ob_start();
		print_admin_styles();
		print_head_scripts();
		echo "<!-- footer -->\n";
		print_footer_scripts();
		$output = ob_get_clean();
		echo $concatenate ? 1 : 0, "\t", $styles, "\t", $scripts, "\t", bin2hex( placeholders( $output ) ), "\n";
	}
}
