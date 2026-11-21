<?php
/**
 * Plugin Name: HyScribe Test
 * Plugin URI: https://hyscribe.com/plugins/hyscribe-test/
 * Description: Drives every call of the extension SDK for HyScribe's tests.
 * Version: 0.1.0
 * Author: HyScribe
 * Author URI: https://hyscribe.com/
 *
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * The PHP plugin the test extension stands in for
 * (crates/hyscribe-ext/guest/hyscribe.ext.toml): the same headers, and what
 * the extension does that PHP shows, so the oracle's Plugins screen, its
 * actions and its pages show what HyScribe shows with the extension:
 * - activation adds `ext_active`, and fails with a fatal error while the
 *   option `ext_hooks` is `trap` or `refuse`; deactivation deletes it
 * - while `ext_hooks` holds any other text, `body_class` gains two classes
 *   and `admin_footer_text` a span, each counting the request's calls
 */

register_activation_hook(
	__FILE__,
	function () {
		if ( in_array( get_option( 'ext_hooks' ), array( 'trap', 'refuse' ), true ) ) {
			throw new Error( 'HyScribe Test does not activate.' );
		}
		add_option( 'ext_active', true );
	}
);

register_deactivation_hook(
	__FILE__,
	function () {
		delete_option( 'ext_active' );
	}
);

// The count of the page hooks' calls in this request, when they are on: the
// extension traps instead while `ext_hooks` is `trap`, changing nothing.
$hyscribe_test_calls = function () {
	static $calls = 0;
	$on = get_option( 'ext_hooks' );
	return is_string( $on ) && 'trap' !== $on ? ++$calls : 0;
};

add_filter(
	'body_class',
	function ( $classes ) use ( $hyscribe_test_calls ) {
		$calls = $hyscribe_test_calls();
		return $calls ? array_merge( $classes, array( 'ext-body', "ext-n$calls" ) ) : $classes;
	}
);

add_filter(
	'admin_footer_text',
	function ( $text ) use ( $hyscribe_test_calls ) {
		$calls = $hyscribe_test_calls();
		return $calls ? $text . "<span id=\"ext-footer\">ext-n$calls</span>" : $text;
	}
);
