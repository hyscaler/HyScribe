<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-render's font faces and relative CSS links. Lines are
//   fonts<TAB>hex(json fontFamilies setting)<TAB>hex(wp_print_font_faces() output)
//   css<TAB>hex(css)<TAB>hex(stylesheet url)<TAB>hex(_wp_normalize_relative_css_links())
//   style<TAB>hex(text)<TAB>hex(<style> with set_modifiable_text())
// Font sources resolve against the active theme. Random cases use a fixed
// seed, so the output is stable.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

mt_srand( 20261028 );

$parse = new ReflectionMethod( 'WP_Font_Face_Resolver', 'parse_settings' );
$pick  = static fn( $list ) => $list[ mt_rand( 0, count( $list ) - 1 ) ];

$families = array( 'Manrope', 'Fira Code', '"Quoted Name", serif', "'Single', sans", 'A, B', '', 'Na<b>me</b>', '0', 'Sp ace', '""', 7 );
$sources  = array(
	'file:./assets/a.woff2', 'file:./assets/a.woff', 'https://cdn.test/b.ttf', '/c.eot', 'd.otf', 'e.svg', 'data:font/woff2;base64,AAA',
	' data:x', 'f.woff2?v=1', 'g', '', 'h.WOFF2', 'dir.v/i',
);
for ( $n = 0; $n < 300; $n++ ) {
	$origins = array();
	foreach ( array( 'default', 'theme', 'custom' ) as $origin ) {
		if ( mt_rand( 0, 2 ) === 0 ) {
			continue;
		}
		$list = array();
		for ( $f = mt_rand( 0, 3 ); $f > 0; $f-- ) {
			$definition = array();
			if ( mt_rand( 0, 6 ) ) {
				$definition['fontFamily'] = $pick( $families );
			}
			if ( mt_rand( 0, 6 ) ) {
				$faces = array();
				for ( $c = mt_rand( 0, 3 ); $c > 0; $c-- ) {
					$face = array();
					if ( mt_rand( 0, 3 ) === 0 ) {
						$face['fontFamily'] = $pick( $families );
					}
					switch ( mt_rand( 0, 5 ) ) {
						case 0:
							break;
						case 1:
							$face['src'] = $pick( $sources );
							break;
						case 2:
							$face['src'] = array( 7 );
							break;
						default:
							$face['src'] = array();
							for ( $s = mt_rand( 1, 4 ); $s > 0; $s-- ) {
								$face['src'][] = $pick( $sources );
							}
					}
					$face['fontWeight']  = $pick( array( '400', 700, '200 800', array( 1 ), null, '0' ) );
					$face['fontStyle']   = $pick( array( 'normal', 'italic', '', null ) );
					$face['fontDisplay'] = $pick( array( 'swap', 'block', 'wrong', null ) );
					foreach ( array( 'unicodeRange' => 'U+0-FF', 'fontStretch' => '0', 'fontFeatureSettings' => '"liga" 1', 'ascentOverride' => '90%', 'bogus' => 'x', 'font-variant' => 'small-caps' ) as $key => $value ) {
						if ( mt_rand( 0, 3 ) === 0 ) {
							$face[ $key ] = $value;
						}
					}
					if ( mt_rand( 0, 4 ) === 0 ) {
						$face['fontVariationSettings'] = mt_rand( 0, 1 ) ? array( 'wght' => 400, 'slnt' => -10 ) : "'wght' 400";
					}
					$faces[] = array_filter( $face, static fn( $v ) => null !== $v );
				}
				$definition['fontFace'] = $faces;
			}
			$list[] = $definition;
		}
		$origins[ $origin ] = $list;
	}
	$fonts = $parse->invoke( null, array( 'typography' => array( 'fontFamilies' => $origins ) ) );
	ob_start();
	@( new WP_Font_Face() )->generate_and_print( $fonts );
	$printed = ob_get_clean();
	echo "fonts\t", bin2hex( wp_json_encode( $origins ) ), "\t", bin2hex( $printed ), "\n";
}

$css = array(
	'a{b:c}', 'a{background:url(img.png)}', 'a{background:url( "../img.png" )}', "a{background:url('./x/y.png')}", 'a{src:url(/abs.woff)}',
	'a{src:url(https://cdn.test/a)}', 'a{src:url(http:x)}', 'a{src:url(#frag)}', 'a{src:url(data:image/png;base64,AA)}', 'a{b:url()}',
	'a{b:url(  )}', 'a{b:urlx(y)} c{d:url (z.png)}', 'a{b:url(\'q"r\')}', 'url', 'url(', 'a{b:url(./../a/./b.png)}', 'a{b:URL(c.png)}',
	'a{b:url(a.png),url(b.png)}', "a{b:url(\t'c.png'\t)}", 'a{b:url(c d.png)}', 'a{b:url ()}', "a{b:url\t( '')}", "a{b:url(' )}",
);
$urls = array(
	'', '/', '///', '/root.css', 'http://127.0.0.1:8081/wp-includes/blocks/x/style.min.css', '/wp-includes/css/dist/block-library/common.min.css', 'https://cdn.test/a/b.css',
	'//cdn.test/c.css', 'relative/d.css', 'e.css', 'HTTP://Upper.test/f/g.css', 'https://h.test', 'https:///i/j.css',
);
foreach ( $css as $code ) {
	foreach ( $urls as $url ) {
		echo "css\t", bin2hex( $code ), "\t", bin2hex( $url ), "\t", bin2hex( _wp_normalize_relative_css_links( $code, $url ) ), "\n";
	}
}
foreach ( array( 'a{b:c}', 'a</style>b', '</STYLE x', '</styl', '</stylesheet', 'x</Style</style' ) as $code ) {
	$processor = new WP_HTML_Tag_Processor( '<style></style>' );
	$processor->next_tag();
	$processor->set_modifiable_text( $code );
	echo "style\t", bin2hex( $code ), "\t", bin2hex( $processor->get_updated_html() ), "\n";
}
