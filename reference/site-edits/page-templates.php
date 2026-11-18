<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The template hierarchy get_single_template() and get_page_template() try
// and get_body_class() for post 1 and page 2 with each `_wp_page_template`
// below, each set in a transaction rolled back. Lines are
//   hex(value), - for none<TAB>post ID<TAB>templates, comma separated<TAB>body classes
// for crates/hyscribe-render/tests/golden/page-templates.tsv, written by
//   docker exec -i hyscribe-default-install-php php /dev/stdin < reference/site-edits/page-templates.php > crates/hyscribe-render/tests/golden/page-templates.tsv

require '/var/www/html/wp-load.php';

$values = array( null, '', '0', 'default', 'single', 'page', 'wp-custom-template-landing', 'page-no-title.php', 'page-no-title.html', 'templates/full.width.php', 'a.php/b.php', 'My Template!', '../x', 'x/../', '../', 'C:x', 'a/../b', 'x/../../' );
$hierarchy = array();
foreach ( array( 'single', 'page' ) as $type ) {
	add_filter(
		"{$type}_template_hierarchy",
		function ( $templates ) use ( &$hierarchy ) {
			$hierarchy = $templates;
			return $templates;
		}
	);
}
foreach ( array( 1 => 'get_single_template', 2 => 'get_page_template' ) as $id => $getter ) {
	foreach ( $values as $value ) {
		$wpdb->query( 'START TRANSACTION' );
		if ( null === $value ) {
			delete_post_meta( $id, '_wp_page_template' );
		} else {
			update_post_meta( $id, '_wp_page_template', $value );
		}
		$GLOBALS['wp_query']     = new WP_Query( 1 === $id ? array( 'p' => 1 ) : array( 'page_id' => 2 ) );
		$GLOBALS['wp_the_query'] = $GLOBALS['wp_query'];
		$GLOBALS['post']         = get_post( $id );
		$getter();
		echo null === $value ? '-' : bin2hex( $value ), "\t", $id, "\t", implode( ',', $hierarchy ), "\t", implode( ' ', get_body_class() ), "\n";
		$wpdb->query( 'ROLLBACK' );
		wp_cache_delete( $id, 'post_meta' );
	}
}
