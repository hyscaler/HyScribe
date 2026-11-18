<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-render's duotone support: blocks with a duotone
// filter through do_blocks(), then what WP_Duotone collected, as lines of
//   phase<TAB>hex(markup)<TAB>hex(html)<TAB>hex(block supports CSS)<TAB>hex(footer SVGs)<TAB>hex(global styles preset CSS)
// Phase 0 is the theme as it is, phase 1 has the site's global styles set
// duotone presets for blocks. The ID counter of wp_unique_id() runs on from
// case to case, as the Rust test's does. Nothing is written to the database.
// Run through golden.sh inside the PHP WordPress oracle.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$reset = function () {
	foreach ( array( 'used_global_styles_presets', 'used_svg_filter_data', 'block_css_declarations' ) as $name ) {
		( new ReflectionProperty( 'WP_Duotone', $name ) )->setValue( null, array() );
	}
};

function duotone_cover( $attrs ) {
	return '<!-- wp:cover ' . wp_json_encode( $attrs ) . ' --><div class="wp-block-cover"><span aria-hidden="true" class="wp-block-cover__background has-background-dim"></span><div class="wp-block-cover__inner-container"><!-- wp:paragraph --><p>Text</p><!-- /wp:paragraph --></div></div><!-- /wp:cover -->';
}

function duotone_image( $attrs ) {
	return '<!-- wp:image ' . wp_json_encode( $attrs ) . ' --><figure class="wp-block-image"><img src="http://127.0.0.1:8081/x.jpg" alt=""/></figure><!-- /wp:image -->';
}

function duotone( $value ) {
	return array( 'style' => array( 'color' => array( 'duotone' => $value ) ) );
}

$cases = array(
	duotone_cover( duotone( array( '#8c00b7', '#fcff41' ) ) ),
	duotone_cover( duotone( array( '#000', '#FFF' ) ) ),
	duotone_cover( duotone( array( '#00000080', '#fff8' ) ) ),
	duotone_cover( duotone( array( 'rgb(255, 0, 128)', 'rgba(10%, 20%, 30%, 50%)' ) ) ),
	duotone_cover( duotone( array( 'rgb(1 2 3)', 'rgb(1 2 3 / 0.25)' ) ) ),
	duotone_cover( duotone( array( 'rgb(10%, 2, 3)', 'rgba(300, -20, 1.5, 7)' ) ) ),
	duotone_cover( duotone( array( 'hsl(120, 100%, 25%)', 'hsla(0.5turn, 50%, 50%, 0.5)' ) ) ),
	duotone_cover( duotone( array( 'hsl(200deg 40% 70%)', 'hsl(3.14rad 10% 90% / 20%)' ) ) ),
	duotone_cover( duotone( array( 'hsl(400, 100%, 50%)', 'hsl(-30, 20%, 20%)', 'hsl(1grad, 0%, 100%)' ) ) ),
	duotone_cover( duotone( array( 'HSL(10DEG, 50%, 50%)', 'RGB(1,2,3)', '#ABCDEF' ) ) ),
	duotone_cover( duotone( array( 'red', '#12', '#123456789', 'rgb(1,2)', 'rgb(1%, 2, 3)', '#fcff41' ) ) ),
	duotone_cover( duotone( array( '#8c00b7' ) ) ),
	duotone_cover( duotone( array() ) ),
	duotone_cover( duotone( array( '#8c00b7', '#fcff41' ) ) ) . duotone_cover( duotone( array( '#000000', '#ffffff' ) ) ),
	duotone_cover( duotone( 'unset' ) ),
	duotone_cover( duotone( 'drop-shadow(1px 2px red) Blur(2px)' ) ),
	duotone_cover( duotone( 'var:preset|duotone|grayscale' ) ),
	duotone_cover( duotone( 'var(--wp--preset--duotone--dark-grayscale)' ) ),
	duotone_cover( duotone( 'var:preset|duotone|grayscale' ) ) . duotone_cover( duotone( 'var:preset|duotone|grayscale' ) ) . duotone_cover( duotone( 'var:preset|duotone|purple-yellow' ) ),
	duotone_cover( duotone( 'var:preset|duotone|no-such-preset' ) ),
	duotone_cover( duotone( 'var(--wp--preset--duotone--blue-red)' ) ),
	duotone_cover( duotone( 'var(--wp--preset--duotone--' ) ),
	duotone_cover( duotone( 'a var:preset|duotone|grayscale b' ) ),
	duotone_cover( duotone( 'var:preset|duotone|0' ) ),
	duotone_cover( duotone( 'var:preset|duotone| x' ) ),
	duotone_cover( duotone( 'var(--wp--preset--duotone--x) var:preset|duotone|grayscale' ) ),
	duotone_cover( duotone( 'var:preset|duotone|grayscale var(--wp--preset--duotone--x)' ) ),
	duotone_cover( duotone( 'var(--wp--preset--duotone--grayscale)extra)' ) ),
	duotone_cover( duotone( 'var(--wp--preset--duotone--)' ) ),
	duotone_cover( duotone( 'var:preset|duotone|plain' ) ),
	duotone_cover( duotone( 'var:preset|duotone|my-preset' ) ) . duotone_cover( duotone( 'var:preset|duotone|My Preset' ) ),
	duotone_cover( duotone( 'var(--wp--preset--duotone--grayscale)' ) ) . duotone_cover( duotone( 'var:preset|duotone|plain' ) ),
	'<!-- wp:cover ' . wp_json_encode( duotone( array( '#8c00b7', '#fcff41' ) ) ) . ' -->no tags<!-- /wp:cover -->',
	duotone_cover( duotone( 5 ) ),
	duotone_cover( duotone( true ) ),
	duotone_cover( duotone( null ) ),
	duotone_cover( array() ),
	duotone_cover( array( 'style' => array( 'color' => array( 'background' => '#fff' ) ) ) ),
	duotone_image( duotone( array( '#8c00b7', '#fcff41' ) ) ),
	duotone_image( duotone( 'var:preset|duotone|midnight' ) ),
	duotone_image( array() ),
	'<!-- wp:paragraph ' . wp_json_encode( duotone( array( '#000', '#fff' ) ) ) . ' --><p>x</p><!-- /wp:paragraph -->',
	duotone_cover( duotone( array( '#8c00b7', '#fcff41' ) ) ) . duotone_image( duotone( array( '#8c00b7', '#fcff41' ) ) ),
	'<!-- wp:group --><div class="wp-block-group">' . duotone_cover( duotone( array( '#000', '#fff' ) ) ) . '</div><!-- /wp:group -->',
);

$run = function ( $phase ) use ( $cases, $reset ) {
	foreach ( $cases as $markup ) {
		$reset();
		WP_Style_Engine_CSS_Rules_Store::remove_all_stores();
		$html = do_blocks( $markup );
		WP_Duotone::output_block_styles();
		$css = wp_style_engine_get_stylesheet_from_context( 'block-supports', array( 'prettify' => false ) );
		ob_start();
		WP_Duotone::output_footer_assets();
		$footer = ob_get_clean();
		$used   = ( new ReflectionProperty( 'WP_Duotone', 'used_global_styles_presets' ) )->getValue();
		$presets = '';
		if ( ! empty( $used ) ) {
			$presets = ( new ReflectionMethod( 'WP_Duotone', 'get_global_styles_presets' ) )->invoke( null, $used );
		}
		echo $phase, "\t", bin2hex( $markup ), "\t", bin2hex( $html ), "\t", bin2hex( $css ), "\t", bin2hex( $footer ), "\t", bin2hex( $presets ), "\n";
	}
};

$run( 0 );

// The site's global styles set a preset on blocks.
$user = array(
	'version'  => 3,
	'settings' => array(
		'color' => array(
			'duotone' => array(
				array( 'name' => 'Plain', 'slug' => 'plain', 'colors' => 'unset' ),
				array( 'name' => 'Gray', 'slug' => 'Grayscale', 'colors' => array( '#111111', '#eeeeee' ) ),
				array( 'name' => 'Mine', 'slug' => 'My Preset', 'colors' => array( '#123456', '#abcdef' ) ),
			),
		),
	),
	'styles'   => array(
		'blocks' => array(
			'core/cover' => array( 'filter' => array( 'duotone' => 'var:preset|duotone|purple-yellow' ) ),
			'core/image' => array( 'filter' => array( 'duotone' => 'var(--wp--preset--duotone--grayscale)' ) ),
			'core/site-logo' => array( 'filter' => array( 'duotone' => 'var:preset|duotone|no-such-preset' ) ),
			'core/avatar' => array( 'filter' => array( 'duotone' => '#ff0000' ) ),
			'core/post-author' => array( 'filter' => array( 'duotone' => array( '#000', '#fff' ) ) ),
		),
	),
);
add_filter(
	'wp_theme_json_data_user',
	function ( $data ) use ( $user ) {
		return $data->update_with( $user );
	}
);
WP_Theme_JSON_Resolver::clean_cached_data();
wp_cache_flush_group( 'theme_json' );
foreach ( array( 'global_styles_block_names', 'global_styles_presets' ) as $name ) {
	( new ReflectionProperty( 'WP_Duotone', $name ) )->setValue( null, null );
}
echo "user\t", bin2hex( wp_json_encode( $user + array( 'isGlobalStylesUserThemeJSON' => true ) ) ), "\n";
$run( 1 );

// A block whose global styles name a preset nobody defined.
$user['styles']['blocks']['core/image'] = array( 'filter' => array( 'duotone' => 'var:preset|duotone|no-such-preset' ) );
add_filter(
	'wp_theme_json_data_user',
	function ( $data ) use ( $user ) {
		return $data->update_with( $user );
	}
);
WP_Theme_JSON_Resolver::clean_cached_data();
wp_cache_flush_group( 'theme_json' );
foreach ( array( 'global_styles_block_names', 'global_styles_presets' ) as $name ) {
	( new ReflectionProperty( 'WP_Duotone', $name ) )->setValue( null, null );
}
echo "user\t", bin2hex( wp_json_encode( $user + array( 'isGlobalStylesUserThemeJSON' => true ) ) ), "\n";
$run( 2 );
