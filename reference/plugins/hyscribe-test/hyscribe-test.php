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
 * - the REST routes of the namespace `ext/v1` (SDK 0.5), each callback
 *   answering what the extension's answers
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

// The admin pages and the settings page the extension adds (SDK 0.4): a
// top level page with a Tools page under it, a settings page under
// Settings saving `ext_title`, kept to letters, digits and spaces, and a
// top level page for administrators, where the extension prints `true`
// when the host refuses it another's settings group.
add_action(
	'admin_menu',
	function () {
		add_menu_page(
			'HyScribe Test',
			'HyScribe Test',
			'read',
			'hyscribe-test-top',
			function () {
				echo '<div class="wrap"><h1>HyScribe Test</h1><p id="ext-top">ext-top</p></div>';
			}
		);
		add_submenu_page(
			'hyscribe-test-top',
			'HyScribe Test Tools',
			'Tools',
			'manage_options',
			'hyscribe-test-sub',
			function () {
				echo '<div class="wrap"><h1>HyScribe Test Tools</h1><p id="ext-sub">ext-sub</p></div>';
			}
		);
		add_options_page(
			'HyScribe Test Settings',
			'HyScribe Test',
			'manage_options',
			'hyscribe-test',
			function () {
				echo '<div class="wrap"><h1>HyScribe Test Settings</h1><form action="options.php" method="post">';
				settings_fields( 'hyscribe_test' );
				do_settings_sections( 'hyscribe-test' );
				submit_button();
				echo '</form></div>';
			}
		);
		add_menu_page(
			'HyScribe Test Admin',
			'HyScribe Test Admin',
			'manage_options',
			'hyscribe-test-admin',
			function () {
				echo '<div class="wrap"><h1>HyScribe Test Admin</h1><p id="ext-admin">true</p></div>';
			}
		);
	}
);

add_action(
	'admin_init',
	function () {
		register_setting(
			'hyscribe_test',
			'ext_title',
			array(
				'type'              => 'string',
				'sanitize_callback' => function ( $value ) {
					return is_string( $value ) ? preg_replace( '/[^A-Za-z0-9 ]/', '', $value ) : '';
				},
			)
		);
		// Each added twice: the second takes the first one's place.
		foreach ( array( 1, 2 ) as $time ) {
			add_settings_section(
				'hyscribe_test_main',
				'Main',
				function () {
					echo '<p id="ext-section">ext-section</p>';
				},
				'hyscribe-test'
			);
			add_settings_field(
				'ext_title',
				'Title',
				function () {
					$value = get_option( 'ext_title' );
					printf(
						'<input name="ext_title" type="text" id="ext_title" value="%s" class="regular-text" />',
						esc_attr( is_string( $value ) ? $value : '' )
					);
				},
				'hyscribe-test',
				'hyscribe_test_main',
				array( 'label_for' => 'ext_title' )
			);
		}
		add_settings_section( 'hyscribe_test_more', '', null, 'hyscribe-test' );
		add_settings_field(
			'ext_more',
			'More',
			function () {
				echo '<span id="ext-more">ext-more</span>';
			},
			'hyscribe-test',
			'hyscribe_test_more',
			array( 'class' => 'ext-row' )
		);
		add_settings_section( 'hyscribe_test_empty', 'Empty', null, 'hyscribe-test' );
	}
);

add_filter(
	'admin_footer_text',
	function ( $text ) use ( $hyscribe_test_calls ) {
		$calls = $hyscribe_test_calls();
		return $calls ? $text . "<span id=\"ext-footer\">ext-n$calls</span>" : $text;
	}
);

// The REST routes the extension registers (SDK 0.5), in its order. A
// request is answered as the extension sees it: its method, route,
// parameters, content type and body.
$hyscribe_test_echo = function ( WP_REST_Request $request ) {
	return array(
		'got' => array(
			'method'       => $request->get_method(),
			'route'        => $request->get_route(),
			'params'       => $request->get_params(),
			'content_type' => $request->get_header( 'content_type' ),
			'body'         => $request->get_body(),
		),
	);
};

add_action(
	'rest_api_init',
	function () use ( $hyscribe_test_echo ) {
		register_rest_route(
			'ext/v1',
			'/items',
			array(
				'methods'             => array( 'GET', 'POST' ),
				'callback'            => $hyscribe_test_echo,
				'permission_callback' => '__return_true',
				'args'                => array(
					'count' => array(
						'type'    => 'integer',
						'minimum' => 1,
						'maximum' => 10,
						'default' => 3,
					),
					'kind'  => array(
						'type' => 'string',
						'enum' => array( 'a', 'b' ),
					),
					'flag'  => array( 'type' => 'boolean' ),
				),
			)
		);
		$id = array( 'id' => array( 'type' => 'integer' ) );
		register_rest_route(
			'ext/v1',
			'/items/(?P<id>\\d+)',
			array(
				array(
					'methods'             => 'GET',
					'callback'            => $hyscribe_test_echo,
					'permission_callback' => '__return_true',
					'args'                => $id,
				),
				array(
					'methods'             => 'DELETE',
					'callback'            => $hyscribe_test_echo,
					'permission_callback' => function () {
						return current_user_can( 'edit_posts' );
					},
					'args'                => $id,
				),
			)
		);
		register_rest_route(
			'ext/v1',
			'/need',
			array(
				'methods'             => 'POST',
				'callback'            => $hyscribe_test_echo,
				'permission_callback' => '__return_true',
				'args'                => array(
					'name'  => array(
						'type'     => 'string',
						'required' => true,
					),
					'title' => array(
						'type'              => 'string',
						'sanitize_callback' => function ( $value ) {
							return strtoupper( (string) $value );
						},
					),
					'code'  => array(
						'validate_callback' => function ( $value ) {
							if ( 'ok' === $value ) {
								return true;
							}
							if ( 'no' === $value ) {
								return false;
							}
							return new WP_Error( 'ext_code', 'The code is not ok.', array( 'status' => 400 ) );
						},
					),
				),
			)
		);
		register_rest_route(
			'ext/v1',
			'/closed',
			array(
				'methods'             => 'GET',
				'callback'            => $hyscribe_test_echo,
				'permission_callback' => '__return_false',
			)
		);
		register_rest_route(
			'ext/v1',
			'/denied',
			array(
				'methods'             => 'GET',
				'callback'            => $hyscribe_test_echo,
				'permission_callback' => function () {
					return new WP_Error( 'ext_denied', 'Not for you.', array( 'status' => 403 ) );
				},
			)
		);
		register_rest_route(
			'ext/v1',
			'/fail',
			array(
				'methods'             => 'GET',
				'callback'            => function () {
					return new WP_Error( 'ext_failed', 'It failed.', array( 'status' => 409 ) );
				},
				'permission_callback' => '__return_true',
			)
		);
		// Saves `ext` through the options API and answers it as saved, with a
		// status and headers of its own.
		register_rest_route(
			'ext/v1',
			'/made',
			array(
				'methods'             => 'POST',
				'callback'            => function ( WP_REST_Request $request ) {
					update_option( 'ext', $request['value'] );
					$response = new WP_REST_Response( array( 'saved' => get_option( 'ext' ) ), 201 );
					$response->header( 'Location', 'http://example.test/made' );
					$response->header( 'X-WP-Total', '2' );
					return $response;
				},
				'permission_callback' => '__return_true',
				'args'                => array( 'value' => array( 'type' => 'string' ) ),
			)
		);
		// The extension traps here: a fatal error.
		register_rest_route(
			'ext/v1',
			'/trap',
			array(
				'methods'             => array( 'GET', 'POST' ),
				'callback'            => function () {
					hyscribe_no_such_function();
				},
				'permission_callback' => '__return_true',
			)
		);
	}
);
