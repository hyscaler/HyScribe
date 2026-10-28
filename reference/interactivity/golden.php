<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints wp_interactivity_process_directives() for directive markup, with
// no server state registered. Lines are
//   hex(markup)<TAB>hex(processed)
// Read only.

require '/var/www/html/wp-load.php';

$ctx = function ( $context, $inner ) {
	return "<div data-wp-interactive=\"test\" data-wp-context='" . wp_json_encode( $context ) . "'>$inner</div>";
};
$context = array(
	'yes'    => true,
	'no'     => false,
	'zero'   => 0,
	'one'    => 1,
	'half'   => 1.5,
	'text'   => 'hello "world"',
	'empty'  => '',
	'list'   => array( 1, 2, 3 ),
	'map'    => array( 'a' => array( 'b' => 'deep' ) ),
	'nil'    => null,
	'color'  => 'red',
	'style'  => ' color: blue ;; ',
);

$cases = array(
	'plain text',
	'<p>no directives</p>',
	'<div data-wp-bind--hidden="state.missing" hidden>x</div>',
	'<div data-wp-interactive="test"><button data-wp-bind--aria-expanded="state.open" aria-expanded="false">b</button></div>',
	'<div data-wp-interactive="test"><button data-wp-bind--aria-expanded="!state.open">b</button></div>',
	'<div data-wp-interactive="test"><span data-wp-class--active="state.on" class="x active">s</span></div>',
	'<div data-wp-interactive="test"><span data-wp-class--active="!state.on" class="x">s</span></div>',
	'<div data-wp-interactive="test"><span data-wp-class--0="!state.on" data-wp-class--a---b="!state.on" data-wp-class--c----d="!state.on">s</span></div>',
	'<div data-wp-interactive="test"><span data-wp-style--color="state.c" style="color:red;margin:0">s</span></div>',
	'<div data-wp-interactive="test"><span data-wp-style--color="!state.c">s</span></div>',
	'<div data-wp-interactive="test"><span data-wp-bind--onclick="!state.x" data-wp-bind---id="!state.x" data-wp-bind--title---u="!state.x">s</span></div>',
	'<div data-wp-interactive=\'{"namespace":"test"}\'><b data-wp-bind--title="!state.x">j</b></div>',
	'<div data-wp-interactive="other::x"><b data-wp-bind--title="test::!state.x">n</b></div>',
	'<div data-wp-interactive="%bad"><b data-wp-bind--title="!state.x">n</b></div>',
	'<div data-wp-interactive><b data-wp-bind--title="!state.x">n</b></div>',
	'<b data-wp-bind--title="!state.x">no namespace</b>',
	'<div data-wp-interactive="test"><b data-wp-bind--title="">e</b><b data-wp-bind--title>e</b><b data-wp-bind--title="1">n</b><b data-wp-bind--title="null">n</b></div>',
	'<div data-wp-interactive="test"><div>unclosed</div>',
	'<div data-wp-interactive="test"></span><b data-wp-bind--title="!state.x">u</b></div>',
	'<div data-wp-interactive="test"><img data-wp-bind--alt="!state.x"><br data-wp-class--z="!state.x"><b data-wp-bind--title="!state.x">v</b></div>',
	'<div data-wp-interactive="test"><svg data-wp-bind--x="!state.x"><svg/><path data-wp-bind--d="!state.x"></path></svg><b data-wp-bind--title="!state.x">after</b></div>',
	'<div data-wp-interactive="test"><math><mi data-wp-bind--x="!state.x">x</mi></math></div>',
	'<div data-wp-interactive="test"><textarea data-wp-bind--title="!state.x"><b></textarea></div>',
	'<div data-wp-interactive="test" data-wp-bind--data-x="!state.x" data-wp-bind--DATA-Y="state.x" data-wp-bind--abcd-e="!state.x" data-wp-bind--abcde="!state.x">d</div>',
	'<div data-wp--x="!state.x" data-wp-bind--="!state.x" data-wp-b.ad="!state.x" data-wp-="1">names</div>',
);
foreach ( array( 'yes', 'no', 'zero', 'one', 'half', 'text', 'empty', 'list', 'map', 'map.a', 'map.a.b', 'map.a.b.c', 'nil', 'missing', 'list.length', 'text.length', 'map.length', 'list.1', '!yes', '!empty', '!list' ) as $path ) {
	foreach ( array( 'aria-label', 'title', 'hidden' ) as $attribute ) {
		$cases[] = $ctx( $context, "<i data-wp-bind--$attribute=\"context.$path\" $attribute=\"old\">p</i>" );
	}
	$cases[] = $ctx( $context, "<i data-wp-class--on=\"context.$path\" class=\"on\">c</i>" );
	// trim() of an array is a fatal error in PHP.
	if ( ! in_array( $path, array( 'list', 'map', 'map.a' ), true ) ) {
		$cases[] = $ctx( $context, "<i data-wp-style--color=\"context.$path\" style=\"margin: 0; color: green\">s</i>" );
	}
}
$cases[] = $ctx( $context, '<i data-wp-style--color="context.color" style="background:url(http://x); color: green">s</i>' );
$cases[] = $ctx( $context, '<i data-wp-style--color="context.style">s</i>' );
$cases[] = $ctx( $context, '<i data-wp-style--color="context.missing" style="color: green">s</i>' );
$cases[] = $ctx( $context, '<i data-wp-style--color="context.missing" style=";;">s</i>' );
$cases[] = $ctx( $context, '<i data-wp-style--color="context.missing" style="color">s</i>' );
$cases[] = $ctx( array( 'a' => array( 'x' => 1, 'y' => 2 ) ), $ctx( array( 'a' => array( 'y' => 3 ), 'b' => true ), '<i data-wp-bind--title="context.a.x" data-wp-bind--lang="context.a.y" data-wp-bind--dir="context.b">n</i>' ) . '<i data-wp-bind--lang="context.a.y" data-wp-bind--dir="context.b">after</i>' );
$cases[] = '<div data-wp-interactive="test" data-wp-context=\'{"a":1}\' data-wp-context--x=\'{"a":2}\' data-wp-context---u=\'{"b":3}\'><i data-wp-bind--title="context.a" data-wp-bind--lang="context.b">c</i></div>';
$cases[] = '<div data-wp-interactive="test" data-wp-context=\'other::{"a":1}\'><i data-wp-bind--title="other::context.a">o</i></div>';
$cases[] = '<div data-wp-interactive="test" data-wp-context=\'[1,2]\'><i data-wp-bind--title="context.0">l</i></div>';
$cases[] = '<div data-wp-interactive="test" data-wp-context=\'"str"\'><i data-wp-bind--title="context.0">s</i></div>';
$cases[] = '<div data-wp-interactive="test" data-wp-context=\'{"a":1}\'><img data-wp-context=\'{"a":2}\'><i data-wp-bind--title="context.a">void</i></div>';
$cases[] = '<div data-wp-interactive="a"><div data-wp-interactive="b"></div><i data-wp-bind--title="!state.x">pop</i></div>';
$cases[] = '<div data-wp-interactive="test"><span data-wp-style="!state.x" data-wp-style--0="!state.x" data-wp-style--color---u="!state.x" data-wp-style--margin="state.x">s</span></div>';
$cases[] = '<div data-wp-interactive="test"><span data-wp-bind.x="!state.x" data-wp-bindx--title="!state.x" data-wp-bind--lang="!state.x">p</span></div>';
$cases[] = '<div data-wp-interactive="test"><svg><g><svg><path/></svg></g></svg><b data-wp-bind--title="!state.x">nested</b></div>';
$cases[] = '<div data-wp-interactive="test"><span><b></span></b><i data-wp-bind--title="!state.x">rebalanced</i></div>';
$cases[] = '<div data-wp-interactive="a" data-wp-context=\'{"v":"A"}\'><div data-wp-interactive="b"></div><i data-wp-bind--title="context.v">popped</i></div>';
$cases[] = $ctx( $context, '<i data-wp-bind--abcd-="context.no" data-wp-bind--a.b="context.yes" data-wp-bind--a---b-="context.yes">suffixes</i>' );
$cases[] = "<div data-wp-interactive=\"test\" data-wp-context='test::\n{\"a\":1}'><i data-wp-bind--title=\"context.a\">newline</i></div>";
$cases[] = "<div data-wp-interactive=\"test\" data-wp-context='test::{\"a\":1}'><i data-wp-bind--title=\"context.a\">explicit</i></div>";
foreach ( $cases as $markup ) {
	echo bin2hex( $markup ), "\t", bin2hex( wp_interactivity_process_directives( $markup ) ), "\n";
}
