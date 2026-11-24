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
		// The other tags a classic theme's templates call, after the
		// above, so their answers stay as they were.
		$print = function ( $name, $callback ) use ( $put ) {
			ob_start();
			$callback();
			$put( $name, ob_get_clean() );
		};
		$shows = array( 'name', 'description', 'charset', 'url', 'wpurl', 'html_type', 'language', 'rss2_url', 'atom_url', 'comments_rss2_url', 'pingback_url', 'nope' );
		foreach ( $shows as $show ) {
			$put( "get_bloginfo:$show", get_bloginfo( $show ) );
			$put( "get_bloginfo:$show:display", get_bloginfo( $show, 'display' ) );
		}
		$print( 'bloginfo:charset', fn() => bloginfo( 'charset' ) );
		$print( 'bloginfo:name', fn() => bloginfo( 'name' ) );
		$put( 'home_url', home_url() );
		$put( 'home_url:/', home_url( '/' ) );
		$samples = array( '#content', 'http://127.0.0.1:8081/?p=1&a=b', 'example.test/a b', 'javascript:alert(1)', "a\"b<c>&amp;'d", '<p onclick="x">Hi <script>x</script><a href="https://w.test/">w</a></p>' );
		foreach ( $samples as $at => $sample ) {
			$put( "esc_url:$at", esc_url( $sample ) );
			$put( "esc_attr:$at", esc_attr( $sample ) );
			$put( "esc_html:$at", esc_html( $sample ) );
			$put( "wp_kses_post:$at", wp_kses_post( $sample ) );
		}
		$print( 'the_archive_title', fn() => the_archive_title( '<h1 class="entry-title">', '</h1>' ) );
		$print( 'the_archive_description', fn() => the_archive_description( '<p class="archive-description">', '</p>' ) );
		$put( 'get_search_query', get_search_query() );
		$put( 'get_search_query:raw', get_search_query( false ) );
		global $wp_query;
		$put( 'max_num_pages', $wp_query->max_num_pages );
		$print( 'previous_posts_link', fn() => previous_posts_link( '<span class="meta-nav">&larr;</span> Previous' ) );
		$print( 'next_posts_link', fn() => next_posts_link( 'Next <span class="meta-nav">&rarr;</span>' ) );
		while ( have_posts() ) {
			the_post();
			$at = 'post:' . get_the_ID() . ':';
			$print( $at . 'the_title', fn() => the_title( '<h1 class="entry-title">', '</h1>' ) );
			$put( $at . 'get_the_title', get_the_title() );
			$put( $at . 'get_permalink', get_permalink() );
			$print( $at . 'the_permalink', fn() => the_permalink() );
			$print( $at . 'post_class', fn() => post_class( 'site-main' ) );
			$put( $at . 'get_post_type', get_post_type() );
			$put( $at . 'post_type_supports:comments', $bool( post_type_supports( get_post_type(), 'comments' ) ) );
			$put( $at . 'has_post_thumbnail', $bool( has_post_thumbnail() ) );
			$put( $at . 'get_the_post_thumbnail', get_the_post_thumbnail( null, 'large' ) );
			$print( $at . 'the_excerpt', fn() => the_excerpt() );
			$print( $at . 'the_content', fn() => the_content() );
			$print( $at . 'wp_link_pages', fn() => wp_link_pages() );
			$put( $at . 'has_tag', $bool( has_tag() ) );
			$print( $at . 'the_tags', fn() => the_tags( '<span class="tag-links">Tagged ', ', ', '</span>' ) );
			$put( $at . 'comments_open', $bool( comments_open() ) );
			$print( $at . 'comments_template', fn() => comments_template() );
			$put( $at . 'have_comments', $bool( have_comments() ) );
			$put( $at . 'get_comments_number', get_comments_number() );
			$print( $at . 'the_comments_navigation', fn() => the_comments_navigation() );
			$print( $at . 'wp_list_comments', fn() => wp_list_comments( array( 'style' => 'ol', 'short_ping' => true, 'avatar_size' => 42 ) ) );
			$print( $at . 'comment_form', fn() => comment_form( array( 'title_reply_before' => '<h2 id="reply-title" class="comment-reply-title">', 'title_reply_after' => '</h2>' ) ) );
		}
		echo implode( "\n", $out ), "\n";
		exit;
	},
	PHP_INT_MAX
);
