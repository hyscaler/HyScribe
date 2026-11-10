<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for social link markup on the home page, and
// is_email() over addresses. Lines are
//   block<TAB>hex(markup)<TAB>hex(html)
//   email<TAB>hex(text)<TAB>1 or 0
// An address's link is drawn at random by antispambot(). Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

function link_block( $attrs ) {
	return '<!-- wp:social-link ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES ) . ' /-->';
}
function links( $attrs, $inner ) {
	$json = $attrs ? ' ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES ) : '';
	return "<!-- wp:social-links$json --><ul class=\"wp-block-social-links\">" . implode( '', $inner ) . '</ul><!-- /wp:social-links -->';
}

$markups = array();
// The theme's own social links.
foreach ( WP_Block_Patterns_Registry::get_instance()->get_all_registered() as $pattern ) {
	if ( preg_match_all( '/<!-- wp:social-links.*?<!-- \/wp:social-links -->/s', $pattern['content'], $found ) ) {
		$markups = array_merge( $markups, $found[0] );
	}
}
$urls = array( 'https://x.test/a', 'example.com', 'example.com/p?q=1', '//cdn.test', '#top', 'mailto:me@example.com', 'me@example.com', 'tel:123', 'javascript:alert(1)', 'localhost:8080', '0', '', 'a b' );
foreach ( $urls as $url ) {
	$markups[] = links( array(), array( link_block( array( 'url' => $url, 'service' => 'github' ) ) ) );
}
$singles = array(
	array( 'url' => '#', 'service' => 'wordpress' ),
	array( 'url' => '#', 'service' => 'nope' ),
	array( 'url' => '#' ),
	array( 'url' => '#', 'service' => 'x', 'label' => '  Follow me  ' ),
	array( 'url' => '#', 'service' => 'x', 'label' => '0' ),
	array( 'url' => '#', 'service' => 'x', 'label' => '   ' ),
	array( 'url' => '#', 'service' => 'x', 'label' => 'A & <b>B</b>' ),
	array( 'url' => '#', 'service' => 'x', 'rel' => ' me ' ),
	array( 'url' => '#', 'service' => 'mastodon', 'rel' => 'me', 'className' => 'extra' ),
);
foreach ( $singles as $attrs ) {
	$markups[] = links( array(), array( link_block( $attrs ) ) );
}
$contexts = array(
	array( 'openInNewTab' => true ),
	array( 'openInNewTab' => true, 'showLabels' => true ),
	array( 'iconColor' => 'base', 'iconColorValue' => '#fff' ),
	array( 'iconBackgroundColor' => 'contrast', 'iconBackgroundColorValue' => '#111', 'customIconColor' => '#f00' ),
	array( 'iconColor' => 'base', 'iconColorValue' => '#fff', 'iconBackgroundColor' => 'contrast', 'iconBackgroundColorValue' => '#111', 'className' => 'is-style-logos-only' ),
);
foreach ( $contexts as $attrs ) {
	$markups[] = links( $attrs, array( link_block( array( 'url' => '#', 'service' => 'bluesky' ) ), link_block( array( 'url' => '#', 'service' => 'chain', 'rel' => 'me' ) ) ) );
}
// Outside a social links block there is no context.
$markups[] = link_block( array( 'url' => '#', 'service' => 'feed' ) );

foreach ( $markups as $markup ) {
	echo "block\t", bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
}

$emails = array( 'me@example.com', 'a@b.co', 'a@b.c', '@example.com', 'me@@example.com', "me\n@example.com", "m e@example.com", 'me@example', 'me@ex..com', 'me@.example.com', 'me@example.com.', 'me@-ex.com', 'me@ex-.com', 'me@ex_ample.com', "me@example.com\n", "!#$%&'*+/=?^_`{|}~.-@x.io", 'me@exa mple.com', 'mé@example.com', 'me@exämple.com', '12345' );
foreach ( $emails as $email ) {
	echo "email\t", bin2hex( $email ), "\t", is_email( $email ) ? 1 : 0, "\n";
}
