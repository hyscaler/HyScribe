<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for navigation markup. Each case starts with the
// renderer's static state reset. Lines are
//   hex(markup)<TAB>wp_unique_id() counter<TAB>queried object<TAB>hex(html)
// where the queried object is empty, post:<ID> or term:<ID>. Read only.

require '/var/www/html/wp-load.php';

// Uploads under the fixture database's siteurl, which hyscribe's test reads.
$fixture_site = 'http://default-install.test';
add_filter(
	'upload_dir',
	function ( $dirs ) use ( $fixture_site ) {
		$dirs['url'] = "$fixture_site/wp-content/uploads";
		return $dirs;
	}
);

function nav( $attrs, $inner = '' ) {
	$json = $attrs ? ' ' . wp_json_encode( $attrs ) : '';
	return "<!-- wp:navigation$json -->$inner<!-- /wp:navigation -->";
}
function link_block( $attrs, $inner = '' ) {
	$json = ' ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES );
	return '' === $inner ? "<!-- wp:navigation-link$json /-->" : "<!-- wp:navigation-link$json -->$inner<!-- /wp:navigation-link -->";
}

$links  = link_block( array( 'label' => 'Blog', 'url' => '#' ) ) . link_block( array( 'label' => 'About', 'url' => '#' ) );
$never  = array( 'overlayMenu' => 'never' );
$cases  = array();
$add    = function ( $markup, $queried = '' ) use ( &$cases ) {
	$cases[] = array( $markup, $queried );
};

$add( nav( array( 'overlayMenu' => 'never', 'layout' => array( 'type' => 'flex', 'orientation' => 'vertical' ) ), $links ) );
$add( nav( $never, $links ) . nav( $never, $links ) . nav( $never + array( 'ariaLabel' => 'Main' ), $links ) . nav( $never + array( 'ariaLabel' => 'Main' ), $links ) );
$add( nav( array(), $links ) );
$add( nav( array(), $links ) . nav( array( 'overlayMenu' => 'always' ), $links ) );
$add( nav( array( 'overlayBackgroundColor' => 'base', 'overlayTextColor' => 'contrast', 'layout' => array( 'type' => 'flex', 'justifyContent' => 'right', 'flexWrap' => 'wrap' ) ), $links ) );
$add( nav( array( 'hasIcon' => false ), $links ) );
$add( nav( array( 'icon' => 'menu' ), $links ) );
$add( nav( array( 'customOverlayTextColor' => '#123456', 'customOverlayBackgroundColor' => 'red; x:expression(1)' ), $links ) );
$add( nav( $never + array( 'textColor' => 'contrast', 'backgroundColor' => 'base', 'fontSize' => 'small' ), $links ) );
$add( nav( $never + array( 'customTextColor' => '#abcdef', 'customBackgroundColor' => '#fedcba', 'customFontSize' => 13 ), $links ) );
$add( nav( $never + array( 'rgbTextColor' => '#111111', 'rgbBackgroundColor' => '#222222' ), $links ) );
$add( nav( $never + array( 'rgbTextColor' => '#111111', 'textColor' => 'contrast' ), $links ) );
$add( nav( $never + array( 'layout' => array( 'type' => 'flex', 'justifyContent' => 'space-between', 'flexWrap' => 'nowrap' ) ), $links ) );
$add( nav( $never + array( 'layout' => array( 'type' => 'flex', 'justifyContent' => 'stretch' ) ), $links ) );
$add( nav( $never + array( 'style' => array( 'typography' => array( 'textDecoration' => 'var:preset|text-decoration|strikethrough', 'fontStyle' => 'var:preset|font-style|italic', 'textTransform' => 'uppercase', 'fontWeight' => '' ) ) ), $links ) );
$add( nav( $never + array( 'style' => array( 'typography' => array( 'textDecoration' => 'underline' ) ) ), $links ) );
$add( nav( $never + array( 'ariaLabel' => 'Footer "menu" <b>' ), $links ) );
$add( nav( $never + array( '_isWithinOverlayTemplatePart' => true, 'ariaLabel' => 'Inner' ), $links ) );
$add( nav( $never + array( 'isResponsive' => true ), $links ) );
$add( nav( $never + array( 'ref' => 999 ), $links ) );
$add( nav( $never, '<!-- wp:site-title /-->' . $links . '<!-- wp:paragraph --><p>Text</p><!-- /wp:paragraph -->' . $links ) );
$add( nav( $never, '<!-- wp:paragraph --><p>Text</p><!-- /wp:paragraph -->' ) );
$add( nav( $never, $links . nav( $never, $links ) ) );
$add( nav( $never, '<!-- wp:group --><div class="wp-block-group">' . nav( $never, $links ) . '</div><!-- /wp:group -->' ) );
$add( nav( $never, link_block( array( 'label' => 'Parent', 'url' => '/p' ), $links ) ) );
$add( nav( $never + array( 'showSubmenuIcon' => false ), link_block( array( 'label' => 'Parent', 'url' => '/p' ), $links ) ) );
$add( link_block( array( 'label' => 'Alone', 'url' => '/a' ), $links ) );
$add( link_block( array( 'label' => 'Empty child', 'url' => '/a' ), '<!-- wp:paragraph --><p> </p><!-- /wp:paragraph -->' ) );

$link_attrs = array(
	array( 'label' => '' ),
	array( 'label' => '0', 'url' => '#' ),
	array( 'label' => 'No URL' ),
	array( 'label' => '<strong>Bold</strong><script>x</script>', 'url' => '#', 'description' => 'About <em>us</em><img src=x onerror=1>' ),
	array( 'label' => 'Encoded', 'url' => 'https://example.com/?q=%2541&r=1' ),
	array( 'label' => 'Encoded once', 'url' => 'https://example.com/?q=%41' ),
	array( 'label' => 'Encoded array', 'url' => 'https://example.com/?q[]=%2541' ),
	array( 'label' => 'Encoded then plain', 'url' => 'https://example.com/?q=%2541&q=x' ),
	array( 'label' => 'Plus', 'url' => 'https://example.com/?q=a+b%252B' ),
	array( 'label' => 'Fragment', 'url' => 'https://example.com/#x?q=%2541' ),
	array( 'label' => 'Flag', 'url' => 'https://example.com/?flag' ),
	array( 'label' => 'No name', 'url' => 'https://example.com/?=%2541' ),
	array( 'label' => 'Dotted', 'url' => 'https://example.com/?a.b=%2541&a_b=x' ),
	array( 'label' => 'Open bracket', 'url' => 'https://example.com/?a[b=%2541&a_b=x' ),
	array( 'label' => 'Spaced name', 'url' => 'https://example.com/?+a+b=%2541&a_b=x' ),
	array( 'label' => 'Closing bracket', 'url' => 'https://example.com/?a]=%2541' ),
	array( 'label' => 'Lower hex', 'url' => 'https://example.com/?q=%2a%2' ),
	array( 'label' => 'Bad', 'url' => 'javascript:alert(1)' ),
	array( 'label' => 'PDF <object data="' . $fixture_site . '/wp-content/uploads/a.pdf" type="application/pdf"></object>', 'url' => '#' ),
	array( 'label' => 'Other PDF <object data="http://example.com/a.pdf" type="application/pdf"></object>', 'url' => '#' ),
	array( 'label' => 'Spaces', 'url' => ' /a b?c=d e' ),
	array( 'label' => 'New tab', 'url' => '#', 'opensInNewTab' => true ),
	array( 'label' => 'Rel', 'url' => '#', 'rel' => 'noopener "x"', 'nofollow' => true ),
	array( 'label' => 'Nofollow', 'url' => '#', 'nofollow' => true ),
	array( 'label' => 'Title', 'url' => '#', 'title' => 'The "title" & more' ),
	array( 'label' => 'Post', 'url' => '#', 'kind' => 'post-type', 'type' => 'post', 'id' => 1 ),
	array( 'label' => 'Page', 'url' => '#', 'type' => 'page', 'id' => 2 ),
	array( 'label' => 'Missing', 'url' => '#', 'type' => 'page', 'id' => 999 ),
	array( 'label' => 'Custom id', 'url' => '#', 'kind' => 'custom', 'id' => 999 ),
	array( 'label' => 'Category', 'url' => '#', 'kind' => 'taxonomy', 'type' => 'category', 'id' => 1 ),
);
foreach ( $link_attrs as $attrs ) {
	$add( nav( $never, link_block( $attrs ) ) );
}
$add( nav( $never, link_block( array( 'label' => 'Page', 'url' => '#', 'kind' => 'post-type', 'type' => 'page', 'id' => 2 ) ) ), 'post:2' );
$add( nav( $never, link_block( array( 'label' => 'Page', 'url' => '#', 'type' => 'page', 'id' => 2 ) ) ), 'post:2' );
$add( nav( $never, link_block( array( 'label' => 'Page', 'url' => '#', 'kind' => 'taxonomy', 'id' => 2 ) ) ), 'post:2' );
$add( nav( $never, link_block( array( 'label' => 'Other', 'url' => '#', 'kind' => 'post-type', 'id' => 1 ) ) ), 'post:2' );
$add( nav( $never, link_block( array( 'label' => 'Category', 'url' => '#', 'kind' => 'taxonomy', 'type' => 'category', 'id' => 1 ) ) ), 'term:1' );
$add( nav( $never, link_block( array( 'label' => 'Category', 'url' => '#', 'kind' => 'post-type', 'id' => 1 ) ) ), 'term:1' );

$renderer = new ReflectionClass( 'WP_Navigation_Block_Renderer' );
foreach ( $cases as list( $markup, $queried ) ) {
	$renderer->getProperty( 'seen_menu_names' )->setValue( null, array() );
	$renderer->getProperty( 'has_submenus' )->setValue( null, false );
	$GLOBALS['wp_query'] = new WP_Query();
	if ( $queried ) {
		list( $kind, $id ) = explode( ':', $queried );
		$GLOBALS['wp_query']->queried_object    = 'post' === $kind ? get_post( (int) $id ) : get_term( (int) $id );
		$GLOBALS['wp_query']->queried_object_id = (int) $id;
	}
	$counter = (int) wp_unique_id();
	echo bin2hex( $markup ), "\t", $counter, "\t", $queried, "\t", bin2hex( do_blocks( $markup ) ), "\n";
}
