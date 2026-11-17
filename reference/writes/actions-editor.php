<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's block editor views (MVP 5.1), loaded beside
// actions.php: the writes a view of post-new.php or post.php?action=edit
// makes, through the functions that make them, in the page's order. The
// theme's pattern cache and the admin's update checks are left out.

// What the editor's page writes once its post is known: the theme's global
// styles post (its preloads), the edit lock, the custom CSS lookup.
$editor_view = function ( $id ) {
	WP_Theme_JSON_Resolver::get_user_global_styles_post_id();
	wp_set_post_lock( $id );
	wp_get_custom_css();
};

// The writes of a site whose editor was viewed once already.
$editor_viewed = function () {
	WP_Theme_JSON_Resolver::get_user_global_styles_post_id();
	wp_get_custom_css();
};

$editor_new = function ( $type ) use ( $editor_view ) {
	return function () use ( $type, $editor_view ) {
		$editor_view( get_default_post_to_edit( $type, true )->ID );
	};
};

// The theme mods php.sh's first page view wrote, which the fixture lacks.
$editor_fresh = function () {
	delete_option( 'theme_mods_' . get_stylesheet() );
};

return array(
	// post-new.php's first view.
	'admin-editor-new'                 => array(
		'setup'  => $editor_fresh,
		'action' => $editor_new( 'post' ),
	),
	// The same with unfiltered HTML, which keeps the global styles as written.
	'admin-editor-new-unfiltered-html' => array(
		'setup'  => $editor_fresh,
		'action' => $editor_new( 'post' ),
	),
	// post-new.php?post_type=page once the editor was viewed.
	'admin-editor-new-page'            => array(
		'setup'  => $editor_viewed,
		'action' => $editor_new( 'page' ),
	),
	// post.php?post=1&action=edit once the editor was viewed.
	'admin-editor-edit'                => array(
		'setup'  => $editor_viewed,
		'action' => function () use ( $editor_view ) {
			$editor_view( 1 );
		},
	),
);
