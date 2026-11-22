<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/theme-tags/golden.sh: where the template
// loader has picked the classic theme's template, the template tags the
// host answers for a theme (ADR 0023) are printed instead of the page, one
// line each, hyscribe-tag<TAB>name<TAB>hex(value), and nothing is rendered.
// It only reads.
add_filter(
	'template_include',
	function () {
		$out  = array();
		$bool = fn( $value ) => $value ? 'true' : 'false';
		$put  = function ( $name, $value ) use ( &$out ) {
			$out[] = "hyscribe-tag\t" . $name . "\t" . bin2hex( (string) $value );
		};
		$conditionals = array( 'is_home', 'is_front_page', 'is_singular', 'is_single', 'is_page', 'is_attachment', 'is_archive', 'is_category', 'is_tag', 'is_tax', 'is_author', 'is_date', 'is_year', 'is_month', 'is_day', 'is_time', 'is_search', 'is_404', 'is_paged', 'is_preview', 'is_feed', 'is_embed', 'is_privacy_policy', 'is_page_template', 'is_admin', 'is_rtl', 'in_the_loop' );
		foreach ( $conditionals as $tag ) {
			$put( $tag, $bool( $tag() ) );
		}
		$put( 'is_singular:page', $bool( is_singular( 'page' ) ) );
		$put( 'is_singular:post', $bool( is_singular( 'post' ) ) );
		$ids = array();
		while ( have_posts() ) {
			the_post();
			$ids[] = get_the_ID() . ( in_the_loop() ? '' : '!' );
		}
		$put( 'loop', implode( ',', $ids ) );
		$put( 'in_the_loop:after', $bool( in_the_loop() ) );
		rewind_posts();
		$put( 'have_posts:rewound', $bool( have_posts() ) );
		wp_reset_postdata();
		ob_start();
		language_attributes();
		$put( 'language_attributes', ob_get_clean() );
		ob_start();
		body_class();
		$put( 'body_class', ob_get_clean() );
		ob_start();
		wp_body_open();
		$put( 'wp_body_open', ob_get_clean() );
		$put( 'has_custom_logo', $bool( has_custom_logo() ) );
		ob_start();
		the_custom_logo();
		$put( 'the_custom_logo', ob_get_clean() );
		foreach ( array( 'menu-1', 'menu-2' ) as $location ) {
			$menu = wp_nav_menu(
				array(
					'theme_location' => $location,
					'fallback_cb'    => false,
					'container'      => false,
					'echo'           => false,
				)
			);
			$put( "wp_nav_menu:$location", false === $menu ? 'false' : 'markup:' . $menu );
		}
		// The logo printed again in the header and the footer: its high
		// priority is taken once.
		foreach ( array( 'header', 'footer' ) as $part ) {
			do_action( "get_$part", null, array() );
			ob_start();
			the_custom_logo();
			$put( "the_custom_logo:$part", ob_get_clean() );
		}
		echo implode( "\n", $out ), "\n";
		exit;
	},
	PHP_INT_MAX
);
