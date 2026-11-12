<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for the rich text half of WP_Block::replace_html(): the processor
// WP_Block::get_block_bindings_processor() makes, walked to the first element
// matching a selector list and spliced by replace_rich_text(). The value goes in
// as given, wp_kses_post() having its own oracle. One case per line:
//   hex(content) <TAB> hex(selectors) <TAB> hex(value) <TAB> offsets <TAB> hex(result)
// with the inner block offsets comma-separated.
//   php rich-text-golden.php <wordpress> > crates/hyscribe-render/tests/golden/bindings-rich-text.tsv

ini_set( 'display_errors', 'stderr' );
error_reporting( E_ALL & ~E_WARNING & ~E_NOTICE & ~E_DEPRECATED );
define( 'ABSPATH', rtrim( $argv[1], '/' ) . '/' );
define( 'WPINC', 'wp-includes' );
define( 'WP_DEBUG', false );
function __( $text ) {
	return $text;
}
foreach ( array(
	'plugin',
	'functions',
	'formatting',
	'kses',
	'http',
	'utf8',
	'class-wp-token-map',
	'html-api/html5-named-character-references',
	'html-api/class-wp-html-attribute-token',
	'html-api/class-wp-html-span',
	'html-api/class-wp-html-text-replacement',
	'html-api/class-wp-html-decoder',
	'html-api/class-wp-html-tag-processor',
	'html-api/class-wp-html-doctype-info',
	'html-api/class-wp-html-open-elements',
	'html-api/class-wp-html-token',
	'html-api/class-wp-html-stack-event',
	'html-api/class-wp-html-processor-state',
	'html-api/class-wp-html-unsupported-exception',
	'html-api/class-wp-html-active-formatting-elements',
	'html-api/class-wp-html-processor',
	'class-wp-block',
) as $file ) {
	require_once ABSPATH . WPINC . "/$file.php";
}

$processor = new ReflectionMethod( 'WP_Block', 'get_block_bindings_processor' );

// The rich text branch of replace_html(), as WordPress 7.1 writes it.
function replace( $content, $selector_list, $value, $offsets ) {
	global $processor;
	$block_reader = $processor->invoke( null, $content );
	$selectors    = explode( ',', $selector_list );
	$block_reader->next_tag();
	$block_reader->set_bookmark( 'iterate-selectors' );
	foreach ( $selectors as $selector ) {
		if ( strcasecmp( (string) $block_reader->get_tag(), $selector ) === 0 || $block_reader->next_tag( array( 'tag_name' => $selector ) ) ) {
			$block_reader->release_bookmark( 'iterate-selectors' );
			$block_reader->replace_rich_text( $value, $offsets );
			return $block_reader->get_updated_html();
		} else {
			$block_reader->seek( 'iterate-selectors' );
		}
	}
	$block_reader->release_bookmark( 'iterate-selectors' );
	return $content;
}

function emit( $content, $selectors, $value, $offsets = array() ) {
	$result = replace( $content, $selectors, $value, $offsets );
	echo bin2hex( $content ), "\t", bin2hex( $selectors ), "\t", bin2hex( $value ), "\t", implode( ',', $offsets ), "\t", bin2hex( $result ), "\n";
}

$headings = 'h1,h2,h3,h4,h5,h6';
$cases    = array(
	array( '<p>Hello</p>', 'p' ),
	array( '<p class="x">Hello <strong>w</strong></p>', 'p' ),
	array( "\n<p>Hi</p>\n", 'p' ),
	array( '<P>x</P>', 'p' ),
	array( '<p>', 'p' ),
	array( '<p></p>', 'p' ),
	array( '<p>text', 'p' ),
	array( '<p>a<div>b</div>c</p>', 'p' ),
	array( '<p><p>x</p>', 'p' ),
	array( '<div>x</div>', 'p' ),
	array( 'plain text', 'p' ),
	array( '<!-- c --><p>x</p>', 'p' ),
	array( '<div><p>x</p></div>', 'p' ),
	array( '<h2 class="wp-block-heading">Old</h2>', $headings ),
	array( '<h4>Old</h4>', $headings ),
	array( '<h3>a</h3><h2>b</h2>', $headings ),
	array( '<li>Item</li>', 'li' ),
	array( '<li>Item<ul><li>inner</li></ul></li>', 'li', array( 8 ) ),
	array( '<li><ul><li>inner</li></ul></li>', 'li', array( 4 ) ),
	array( '<li>A<ul><li>x</li></ul>B</li>', 'li', array( 5 ) ),
	array( '<li>A</li>', 'li', array( 2, 100 ) ),
	array( '<li>Item<ol><li>x</li></ol></li>', 'li', array( 3, 8 ) ),
	array( '<figure class="wp-block-image"><img src="a.jpg"/><figcaption class="wp-element-caption">Cap</figcaption></figure>', 'figcaption' ),
	array( '<figure class="wp-block-image"><img src="a.jpg"/></figure>', 'figcaption' ),
	array( '<div class="wp-block-button"><a class="wp-block-button__link">Go</a></div>', 'a,button' ),
	array( '<div class="wp-block-button"><button type="button">Go</button></div>', 'a,button' ),
	array( '<div class="wp-block-button"><button>Go</button></div>', 'a, button' ),
	array( '<a href="x">Go</a>', 'a,button' ),
	array( '<button>Go</button><a>x</a>', 'a,button' ),
	array( '<img><p>a</p>', 'img' ),
	array( '<br><p>a</p>', 'br,p' ),
	array( '<table><tr><td>x</td></tr></table><p>y</p>', 'p' ),
	array( '<table>x</table><p>y</p>', 'p' ),
	array( '<a>x</a><table>y</table>', 'button,a' ),
	array( '<p>a</p><table>y</table>', 'button,p' ),
	array( '<p><b>x</p>y', 'p' ),
	array( '<svg><p>x</p></svg>', 'p' ),
	array( '<svg><a>x</a></svg>', 'a' ),
	array( '<svg><a/>x</svg>', 'a' ),
	array( '<math><mi>x</mi></math><p>y</p>', 'p' ),
	array( '<textarea>x</textarea>', 'textarea' ),
	array( '<template><p>x</p></template>', 'p' ),
	array( '<p>x</p>', '' ),
	array( '<p>x</p>', 'p,' ),
	array( '<p>x</p>', ',p' ),
	array( '<image>x', 'img' ),
	array( '<ul><li>a<li>b</ul>', 'li' ),
	array( '<li>a<li>b', 'li', array( 5 ) ),
	array( '<dl><dt>a<dd>b</dl>', 'dd' ),
);
foreach ( $cases as $case ) {
	foreach ( array( 'New', '', '<em>x</em> & y' ) as $value ) {
		emit( $case[0], $case[1], $value, $case[2] ?? array() );
	}
}

// Random fragments with offsets, from a fixed seed.
mt_srand( 20261112 );
$atoms     = array(
	'<p>', '</p>', '<li>', '</li>', '<ul>', '</ul>', '<a href=x>', '</a>', '<button>', '</button>', '<div>', '</div>', '<b>', '</b>',
	'<h2>', '</h2>', '<figcaption>', '</figcaption>', '<img>', '<br>', 'text', ' ', '<!--c-->', '<table>', '<td>', '<svg>', '</svg>',
);
$selectors = array( 'p', 'li', 'a,button', $headings, 'figcaption', 'div' );
for ( $i = 0; $i < 600; $i++ ) {
	$html = '';
	$n    = mt_rand( 1, 10 );
	for ( $j = 0; $j < $n; $j++ ) {
		$html .= $atoms[ mt_rand( 0, count( $atoms ) - 1 ) ];
	}
	$offsets = array();
	if ( mt_rand( 0, 1 ) ) {
		$offsets[] = mt_rand( 0, strlen( $html ) );
		$offsets[] = mt_rand( 0, strlen( $html ) );
		sort( $offsets );
	}
	emit( $html, $selectors[ mt_rand( 0, count( $selectors ) - 1 ) ], 'V', $offsets );
}
