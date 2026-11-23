<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// hs-admin's own look (ADR 0008 amendment, 2026-11-20) in PHP's wp-admin and
// wp-login.php, through WordPress's own filters only, so the goldens of
// admin and login screens stay differential. look.sh adds and removes it.

// The schemes hs-admin offers: `modern`, the near-black "Default", is left
// out, and a user without one of them is shown Coffee. Nothing is written.
const HYSCRIBE_LOOK_SCHEMES = array( 'blue', 'coffee', 'ectoplasm', 'fresh', 'light', 'midnight', 'ocean', 'sunrise' );

add_filter(
	'get_user_option_admin_color',
	function ( $color ) {
		return in_array( $color, HYSCRIBE_LOOK_SCHEMES, true ) ? $color : 'coffee';
	}
);

// The picker lists the others by name, as admin_color_scheme_picker() does
// once `modern` is not registered.
add_action(
	'admin_color_scheme_picker',
	function () {
		global $_wp_admin_css_colors;
		unset( $_wp_admin_css_colors['modern'] );
	},
	9
);

add_filter(
	'login_body_class',
	function ( $classes ) {
		return array_map(
			function ( $class ) {
				return 'admin-color-modern' === $class ? 'admin-color-coffee' : $class;
			},
			$classes
		);
	}
);

// A text mark in place of the WordPress logo, linked to the site.
add_filter(
	'login_headertext',
	function () {
		return 'HyScribe';
	}
);
add_filter(
	'login_headerurl',
	function () {
		return home_url( '/' );
	}
);
add_action(
	'login_head',
	function () {
		echo "<style>.login h1 a{background-image:none;width:auto;height:auto;text-indent:0;font-size:32px;font-weight:600;color:#382e27;}</style>\n";
	},
	100
);

// No WordPress logo menu in the toolbar.
add_action(
	'admin_bar_menu',
	function ( $bar ) {
		$bar->remove_node( 'wp-logo' );
	},
	11
);

// Core's own strings (the default text domain) on admin and login screens
// name HyScribe; content, themes and plugins never pass through here.
const HYSCRIBE_LOOK_TEXT = array(
	'Thank you for creating with <a href="%s">WordPress</a>.' => 'Thank you for creating with HyScribe.',
	'WordPress %1$s running %2$s theme.' => 'Running %2$s theme.',
	'<strong>At a Glance</strong> &mdash; Displays a summary of the content on your site and identifies which theme and version of WordPress you are using.' => '<strong>At a Glance</strong> &mdash; Displays a summary of the content on your site and identifies which theme you are using.',
	'WordPress Events and News' => 'Events and News',
	'<strong>WordPress Events and News</strong> &mdash; Upcoming events near you as well as the latest news from the official WordPress project and the <a href="%s">WordPress Planet</a>.' => '<strong>Events and News</strong> &mdash; Upcoming events near you as well as the latest news.',
	'There are no events scheduled near %1$s at the moment. Would you like to <a href="%2$s">organize a WordPress event</a>?' => 'There are no events scheduled near %1$s at the moment. Would you like to <a href="%2$s">organize an event</a>?',
	'There are no events scheduled near you at the moment. Would you like to <a href="%s">organize a WordPress event</a>?' => 'There are no events scheduled near you at the moment. Would you like to <a href="%s">organize an event</a>?',
	'There is a new kind of WordPress theme, called a block theme, that lets you build the site you&#8217;ve always wanted &#8212; with blocks and styles.' => 'There is a new kind of theme, called a block theme, that lets you build the site you&#8217;ve always wanted &#8212; with blocks and styles.',
	'When a comment contains any of these words in its content, author name, URL, email, IP address, or browser&#8217;s user agent string, it will be held in the <a href="edit-comments.php?comment_status=moderated">moderation queue</a>. One word or IP address per line. It will match inside words, so &#8220;press&#8221; will match &#8220;WordPress&#8221;.' => 'When a comment contains any of these words in its content, author name, URL, email, IP address, or browser&#8217;s user agent string, it will be held in the <a href="edit-comments.php?comment_status=moderated">moderation queue</a>. One word or IP address per line. It will match inside words, so &#8220;scribe&#8221; will match &#8220;HyScribe&#8221;.',
	'When a comment contains any of these words in its content, author name, URL, email, IP address, or browser&#8217;s user agent string, it will be put in the Trash. One word or IP address per line. It will match inside words, so &#8220;press&#8221; will match &#8220;WordPress&#8221;.' => 'When a comment contains any of these words in its content, author name, URL, email, IP address, or browser&#8217;s user agent string, it will be put in the Trash. One word or IP address per line. It will match inside words, so &#8220;scribe&#8221; will match &#8220;HyScribe&#8221;.',
	'In previous versions of WordPress, all importers were built-in. They have been turned into plugins since most people only use them once or infrequently.' => 'Importers are plugins, since most people only use them once or infrequently.',
	'This format, which is called WordPress eXtended RSS or WXR, will contain your posts, pages, comments, custom fields, categories, and tags.' => 'This format, which is called eXtended RSS or WXR, will contain your posts, pages, comments, custom fields, categories, and tags.',
	'If you would like to see more plugins to choose from, click on the &#8220;Add Plugin&#8221; button and you will be able to browse or search for additional plugins from the <a href="%s">WordPress Plugin Directory</a>. Plugins in the WordPress Plugin Directory are designed and developed by third parties, and are compatible with the license WordPress uses. Oh, and they are free!' => 'If you would like to see more plugins to choose from, click on the &#8220;Add Plugin&#8221; button and you will be able to browse or search for additional plugins from the <a href="%s">Plugin Directory</a>. Plugins in the Plugin Directory are designed and developed by third parties, and are compatible with the license HyScribe uses. Oh, and they are free!',
	'The Site Icon is what you see in browser tabs, bookmark bars, and within the WordPress mobile apps. It should be square and at least <strong>%1$s by %2$s</strong> pixels.' => 'The Site Icon is what you see in browser tabs, bookmark bars, and within mobile apps. It should be square and at least <strong>%1$s by %2$s</strong> pixels.',
	// Stored in the `settings_errors` transient, so kept as PHP writes it (ADR 0003).
	'The WordPress address you entered did not appear to be a valid URL. Please enter a valid URL.' => 'The WordPress address you entered did not appear to be a valid URL. Please enter a valid URL.',
);

add_filter(
	'gettext',
	function ( $translation, $text, $domain ) {
		if ( 'default' !== $domain || false === strpos( $translation, 'WordPress' ) ) {
			return $translation;
		}
		if ( ! is_admin() && 'wp-login.php' !== ( $GLOBALS['pagenow'] ?? '' ) ) {
			return $translation;
		}
		return HYSCRIBE_LOOK_TEXT[ $text ] ?? preg_replace( '/WordPress(?!\.org)/', 'HyScribe', $translation );
	},
	10,
	3
);

// The footer's version line names HyScribe's version; look.sh writes it in.
add_filter(
	'update_footer',
	function () {
		return 'HyScribe @HYSCRIBE_VERSION@';
	},
	11
);

// Block descriptions core translates with their context, as the editor's
// server-side block definitions carry them.
const HYSCRIBE_LOOK_BLOCK_TEXT = array(
	'Use the classic WordPress editor.' => 'Use the classic editor.',
	'Insert additional custom elements with a WordPress shortcode.' => 'Insert additional custom elements with a shortcode.',
	'Displays a link to edit the comment in the WordPress Dashboard. This link is only visible to users with the edit comment capability.' => 'Displays a link to edit the comment in the Dashboard. This link is only visible to users with the edit comment capability.',
);

add_filter(
	'gettext_with_context',
	function ( $translation, $text, $context, $domain ) {
		if ( 'default' !== $domain || 'block description' !== $context || ! is_admin() ) {
			return $translation;
		}
		return HYSCRIBE_LOOK_BLOCK_TEXT[ $text ] ?? $translation;
	},
	10,
	4
);

// Strings of WordPress's script bundles, given to wp.i18n as the locale data
// WordPress prints for a script's translations, so the bundles stay as released.
const HYSCRIBE_LOOK_SCRIPT_TEXT = array(
	'wp-block-editor'  => array(
		'Patterns are available from the <Link>WordPress.org Pattern Directory</Link>, bundled in the active theme, or created by users on this site. Only patterns created on this site can be synced.' => 'Patterns are available from the <Link>Pattern Directory</Link>, bundled in the active theme, or created by users on this site. Only patterns created on this site can be synced.',
	),
	'wp-block-library' => array(
		'A WordPress Commenter'          => 'A Commenter',
		'Reply to A WordPress Commenter' => 'Reply to A Commenter',
		'Site Icons are what you see in browser tabs, bookmark bars, and within the WordPress mobile apps. To use a custom icon that is different from your site logo, use the <a>Site Icon settings</a>.' => 'Site Icons are what you see in browser tabs, bookmark bars, and within mobile apps. To use a custom icon that is different from your site logo, use the <a>Site Icon settings</a>.',
	),
	'wp-edit-post'     => array(
		'In the WordPress editor, each paragraph, image, or video is presented as a distinct “block” of content.' => 'In the editor, each paragraph, image, or video is presented as a distinct “block” of content.',
	),
	'wp-edit-site'     => array(
		'%1$s ‹ %2$s ‹ Editor — WordPress' => '%1$s ‹ %2$s ‹ Editor — HyScribe',
	),
);

add_filter(
	'pre_load_script_translations',
	function ( $translations, $file, $handle, $domain ) {
		if ( 'default' !== $domain || ! is_admin() || ! isset( HYSCRIBE_LOOK_SCRIPT_TEXT[ $handle ] ) ) {
			return $translations;
		}
		$messages = array( '' => array( 'domain' => 'messages' ) );
		foreach ( HYSCRIBE_LOOK_SCRIPT_TEXT[ $handle ] as $text => $translation ) {
			$messages[ $text ] = array( $translation );
		}
		return wp_json_encode( array( 'locale_data' => array( 'messages' => $messages ) ) );
	},
	10,
	4
);
