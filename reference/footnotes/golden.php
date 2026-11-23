<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for the footnotes block on the default install's
// post, its footnotes meta and password check given by filters, without
// the elements support, which is checked on its own. Lines are
//   hex(markup)<TAB>hex(footnotes meta)<TAB>protected (0 or 1)<TAB>hex(html)
// The database is only read.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

$cases = array(
	'<!-- wp:footnotes /-->',
	'<!-- wp:footnotes {"className":"extra","fontSize":"small","style":{"spacing":{"padding":{"left":"1em"}}}} /-->',
);
$metas = array(
	'',
	'0',
	'[]',
	'{}',
	'not json',
	'5',
	'"text"',
	'[{"id":"a1","content":"Note <b>one</b><script>x</script>"},{"id":"x\"y","content":"Two"}]',
	'{"k":{"id":"k1","content":"keyed"},"j":{"id":"j1","content":"second"}}',
	'[{"content":"no id"},{"id":"only-id"}]',
	'[null,{"id":"after-null","content":"c"}]',
);

$GLOBALS['wp'] = new WP();
wp( 'p=1' );
$GLOBALS['post'] = get_post( 1 );
foreach ( array( false, true ) as $protected ) {
	remove_all_filters( 'post_password_required' );
	add_filter( 'post_password_required', static fn() => $protected );
	foreach ( $metas as $meta ) {
		remove_all_filters( 'get_post_metadata' );
		add_filter(
			'get_post_metadata',
			static fn( $value, $id, $key ) => 'footnotes' === $key ? array( $meta ) : $value,
			10,
			3
		);
		foreach ( $cases as $markup ) {
			echo bin2hex( $markup ), "\t", bin2hex( $meta ), "\t", (int) $protected, "\t", bin2hex( do_blocks( $markup ) ), "\n";
		}
	}
}
