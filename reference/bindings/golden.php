<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for block bindings, over posts, meta and terms inserted into the
// fixture database inside a transaction that is rolled back. Lines are
//   post<TAB>ID<TAB>post_type<TAB>post_status<TAB>hex(post_password)<TAB>hex(post_content)<TAB>post_date<TAB>post_modified<TAB>hex(post_name)
//   meta<TAB>post ID<TAB>hex(key)<TAB>hex(value)
//   term<TAB>term ID<TAB>taxonomy<TAB>hex(name)<TAB>hex(slug)<TAB>hex(description)<TAB>parent<TAB>count
//   render<TAB>global post ID<TAB>hex(markup)<TAB>hex(do_blocks( markup ))
//   source<TAB>hex(source)<TAB>hex(block JSON)<TAB>hex(args JSON)<TAB>hex(value JSON)
// where the block JSON holds a block's name, attributes and context.
define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';
global $wpdb;
$wpdb->query( 'START TRANSACTION' );
remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

$insert = static fn( $fields ) => wp_insert_post(
	$fields + array(
		'post_status'  => 'publish',
		'post_title'   => 'Bound',
		'post_content' => '',
		'post_date'    => '2000-01-02 03:04:05',
	)
);
$public   = $insert( array( 'post_type' => 'post' ) );
$page     = $insert( array( 'post_type' => 'page' ) );
$draft    = $insert( array( 'post_type' => 'post', 'post_status' => 'draft' ) );
$locked   = $insert( array( 'post_type' => 'post', 'post_password' => 'secret' ) );
$edited   = $insert( array( 'post_type' => 'post' ) );
$wpdb->update( $wpdb->posts, array( 'post_modified' => '2026-03-04 05:06:07', 'post_modified_gmt' => '2026-03-04 05:06:07' ), array( 'ID' => $edited ) );
clean_post_cache( $edited );
foreach ( array( $public, $page, $draft, $locked ) as $id ) {
	update_post_meta( $id, 'footnotes', '[{"content":"Note <b>one</b>","id":"n1"}]' );
	update_post_meta( $id, '_hidden', 'Hidden' );
	update_post_meta( $id, 'plain', 'Plain' );
}
$tag   = wp_insert_term( 'Bound & <tag>', 'post_tag', array( 'description' => 'About <em>it</em><script>x</script>' ) );
$child = wp_insert_term( 'Child', 'category', array( 'parent' => 1, 'slug' => 'kid' ) );
$menu  = wp_insert_term( 'Menu', 'nav_menu' );

$block = static fn( $ref, $attrs = array() ) => '<!-- wp:block ' . wp_json_encode( array( 'ref' => $ref ) + $attrs ) . ' /-->';
$paragraph = static fn( $attrs, $html = '<p>Static</p>' ) => '<!-- wp:paragraph ' . wp_json_encode( $attrs ) . ' -->' . $html . '<!-- /wp:paragraph -->';
$override  = static fn( $name ) => array( 'metadata' => array( 'name' => $name, 'bindings' => array( '__default' => array( 'source' => 'core/pattern-overrides' ) ) ) );
$pattern   = $insert(
	array(
		'post_type'    => 'wp_block',
		'post_content' => $paragraph( $override( 'Intro' ) )
			. '<!-- wp:heading ' . wp_json_encode( $override( 'Head' ) ) . ' --><h2 class="wp-block-heading">Heading</h2><!-- /wp:heading -->'
			. '<!-- wp:buttons --><div class="wp-block-buttons"><!-- wp:button ' . wp_json_encode( $override( 'Go' ) ) . ' --><div class="wp-block-button"><a class="wp-block-button__link wp-element-button" href="http://old.test">Old</a></div><!-- /wp:button --></div><!-- /wp:buttons -->'
			. $paragraph( array( 'metadata' => array( 'name' => 'Mixed', 'bindings' => array( '__default' => array( 'source' => 'core/pattern-overrides' ), 'content' => array( 'source' => 'core/post-data', 'args' => array( 'field' => 'date' ) ) ) ) ) )
			. $paragraph( array( 'metadata' => array( 'name' => 'Only', 'bindings' => array( 'content' => array( 'source' => 'core/pattern-overrides' ) ) ) ) )
			. $paragraph( array( 'metadata' => array( 'bindings' => array( '__default' => array( 'source' => 'core/pattern-overrides' ) ) ) ) )
			. $paragraph( array( 'metadata' => array( 'name' => 7, 'bindings' => array( 'content' => array( 'source' => 'core/pattern-overrides' ) ) ) ) )
			. $paragraph( array( 'metadata' => array( 'name' => true, 'bindings' => array( 'content' => array( 'source' => 'core/pattern-overrides' ) ) ) ) )
			. $paragraph( array( 'metadata' => array( 'name' => 1.5, 'bindings' => array( 'content' => array( 'source' => 'core/pattern-overrides' ) ) ) ) )
			. '<!-- wp:list --><ul class="wp-block-list"><!-- wp:list-item ' . wp_json_encode( $override( 'Item' ) ) . ' --><li>Item<!-- wp:list --><ul class="wp-block-list"><!-- wp:list-item --><li>Inner</li><!-- /wp:list-item --></ul><!-- /wp:list --></li><!-- /wp:list-item --></ul><!-- /wp:list -->',
	)
);
$looping = $insert(
	array(
		'post_type'    => 'wp_block',
		'post_content' => '<!-- wp:query {"queryId":1,"query":{"perPage":1,"postType":"post","inherit":false}} --><div class="wp-block-query"><!-- wp:post-template -->' . $paragraph( $override( 'Intro' ), '<p>Looped</p>' ) . '<!-- /wp:post-template --></div><!-- /wp:query -->' . $paragraph( $override( 'Intro' ) ),
	)
);
$nested = $insert(
	array(
		'post_type'    => 'wp_block',
		'post_content' => '<!-- wp:group --><div class="wp-block-group">' . $block( $pattern ) . '</div><!-- /wp:group -->',
	)
);

foreach ( array( $public, $page, $draft, $locked, $edited, $pattern, $looping, $nested ) as $id ) {
	$post = get_post( $id );
	echo 'post', "\t", $id, "\t", $post->post_type, "\t", $post->post_status, "\t", bin2hex( $post->post_password ), "\t", bin2hex( $post->post_content ), "\t", $post->post_date, "\t", $post->post_modified, "\t", bin2hex( $post->post_name ), "\n";
}
foreach ( array( $public, $page, $draft, $locked ) as $id ) {
	foreach ( array( 'footnotes', '_hidden', 'plain' ) as $key ) {
		echo 'meta', "\t", $id, "\t", bin2hex( $key ), "\t", bin2hex( get_post_meta( $id, $key, true ) ), "\n";
	}
}
$terms = array( array( 1, 'category' ), array( $tag['term_id'], 'post_tag' ), array( $child['term_id'], 'category' ), array( $menu['term_id'], 'nav_menu' ) );
foreach ( $terms as list( $id, $taxonomy ) ) {
	$term = get_term( $id, $taxonomy );
	echo 'term', "\t", $id, "\t", $taxonomy, "\t", bin2hex( $term->name ), "\t", bin2hex( $term->slug ), "\t", bin2hex( $term->description ), "\t", $term->parent, "\t", $term->count, "\n";
}

$content = static fn( $source, $args = null ) => array( 'metadata' => array( 'bindings' => array( 'content' => array( 'source' => $source ) + ( null === $args ? array() : array( 'args' => $args ) ) ) ) );
$renders = array(
	array( $public, $paragraph( $content( 'core/post-data', array( 'field' => 'date' ) ) ) ),
	array( $public, $paragraph( $content( 'core/post-data', array( 'key' => 'date' ) ) ) ),
	array( $public, $paragraph( $content( 'core/post-data', array( 'field' => 'modified' ) ) ) ),
	array( $edited, $paragraph( $content( 'core/post-data', array( 'field' => 'modified' ) ) ) ),
	array( $public, $paragraph( $content( 'core/post-data', array( 'field' => 'link' ) ) ) ),
	array( $public, $paragraph( $content( 'core/post-data', array( 'field' => 'title' ) ) ) ),
	array( $public, $paragraph( $content( 'core/post-data', array( 'field' => '' ) ) ) ),
	array( $public, $paragraph( $content( 'core/post-data' ) ) ),
	array( $draft, $paragraph( $content( 'core/post-data', array( 'field' => 'date' ) ) ) ),
	array( $locked, $paragraph( $content( 'core/post-data', array( 'field' => 'date' ) ) ) ),
	array( 0, $paragraph( $content( 'core/post-data', array( 'field' => 'date' ) ) ) ),
	array( $public, $paragraph( $content( 'core/post-meta', array( 'key' => 'footnotes' ) ) ) ),
	array( $page, $paragraph( $content( 'core/post-meta', array( 'key' => 'footnotes' ) ) ) ),
	array( $pattern, $paragraph( $content( 'core/post-meta', array( 'key' => 'footnotes' ) ) ) ),
	array( $draft, $paragraph( $content( 'core/post-meta', array( 'key' => 'footnotes' ) ) ) ),
	array( $locked, $paragraph( $content( 'core/post-meta', array( 'key' => 'footnotes' ) ) ) ),
	array( $public, $paragraph( $content( 'core/post-meta', array( 'key' => '_hidden' ) ) ) ),
	array( $public, $paragraph( $content( 'core/post-meta', array( 'key' => 'plain' ) ) ) ),
	array( $public, $paragraph( $content( 'core/post-meta', array( 'key' => '' ) ) ) ),
	array( $public, $paragraph( $content( 'core/post-meta' ) ) ),
	array( 0, $paragraph( $content( 'core/post-meta', array( 'key' => 'footnotes' ) ) ) ),
	array( $public, $paragraph( $content( 'core/unknown', array( 'key' => 'x' ) ) ) ),
	array( $public, $paragraph( $content( 'core/term-data', array( 'field' => 'name' ) ) ) ),
	array( $public, $paragraph( array( 'metadata' => array( 'bindings' => array( 'content' => array( 'source' => 7 ) ) ) ) ) ),
	array( $public, $paragraph( array( 'metadata' => array( 'bindings' => array( 'content' => array( 'args' => array( 'field' => 'date' ) ) ) ) ) ) ),
	array( $public, $paragraph( array( 'metadata' => array( 'bindings' => array( 'content' => array( 'source' => 'core/post-data', 'args' => 'date' ) ) ) ) ) ),
	array( $public, $paragraph( array( 'metadata' => array( 'bindings' => array( 'align' => array( 'source' => 'core/post-data', 'args' => array( 'field' => 'date' ) ) ) ) ) ) ),
	array( $public, $paragraph( array( 'metadata' => array( 'bindings' => 'content' ) ) ) ),
	array( $public, $paragraph( array( 'metadata' => array( 'bindings' => array( array( 'source' => 'core/post-data', 'args' => array( 'field' => 'date' ) ) ) ) ) ) ),
	array( $public, '<!-- wp:buttons --><div class="wp-block-buttons"><!-- wp:button {"tagName":"button","metadata":{"bindings":{"url":{"source":"core/post-data","args":{"field":"link"}}}}} --><div class="wp-block-button"><button type="button" class="wp-block-button__link wp-element-button">B</button></div><!-- /wp:button --></div><!-- /wp:buttons -->' ),
	array( $public, $paragraph( array( 'metadata' => array( 'bindings' => array() ) ) ) ),
	array( $public, $paragraph( $content( 'core/post-data', array( 'field' => 'date' ) ), '' ) ),
	array( $public, $paragraph( $content( 'core/post-data', array( 'field' => 'date' ) ), '<div>No paragraph</div>' ) ),
	array( $public, '<!-- wp:heading ' . wp_json_encode( $content( 'core/post-data', array( 'field' => 'link' ) ) ) . ' --><h3 class="wp-block-heading">H</h3><!-- /wp:heading -->' ),
	array( $public, '<!-- wp:group --><div class="wp-block-group">' . $paragraph( $content( 'core/post-data', array( 'field' => 'date' ) ) ) . '</div><!-- /wp:group -->' ),
	array( $public, '<!-- wp:buttons --><div class="wp-block-buttons"><!-- wp:button {"metadata":{"bindings":{"url":{"source":"core/post-data","args":{"field":"link"}},"text":{"source":"core/post-data","args":{"field":"date"}}}}} --><div class="wp-block-button"><a class="wp-block-button__link wp-element-button" href="http://old.test">Old</a></div><!-- /wp:button --></div><!-- /wp:buttons -->' ),
	array( $public, '<!-- wp:list --><ul class="wp-block-list"><!-- wp:list-item ' . wp_json_encode( $content( 'core/post-data', array( 'field' => 'date' ) ) ) . ' --><li>Item<!-- wp:list --><ul class="wp-block-list"><!-- wp:list-item --><li>Inner</li><!-- /wp:list-item --></ul><!-- /wp:list --></li><!-- /wp:list-item --></ul><!-- /wp:list -->' ),
	array( $public, '<!-- wp:post-date {"metadata":{"bindings":{"datetime":{"source":"core/post-data","args":{"field":"date"}}}}} /-->' ),
	array( $public, '<!-- wp:post-date {"metadata":{"bindings":{"datetime":{"source":"core/post-meta","args":{"key":"footnotes"}}}}} /-->' ),
	array( $public, '<!-- wp:post-date {"metadata":{"bindings":{"datetime":{"source":"core/post-meta","args":{"key":"plain"}}}}} /-->' ),
	array( $public, '<!-- wp:post-date {"metadata":{"bindings":{"datetime":{"source":"core/post-data"}}}} /-->' ),
	array( $public, '<!-- wp:post-date {"datetime":"2020-05-06T07:08:09+00:00","metadata":{"bindings":{"datetime":{"source":"core/post-data","args":{"field":"modified"}}}}} /-->' ),
	array( $public, '<!-- wp:image {"metadata":{"bindings":{"url":{"source":"core/post-data","args":{"field":"link"}},"alt":{"source":"core/post-data","args":{"field":"date"}},"title":{"source":"core/post-data","args":{"field":"date"}}}}} --><figure class="wp-block-image"><img src="a.jpg" alt=""/></figure><!-- /wp:image -->' ),
	array( $public, $paragraph( array( 'metadata' => array( 'name' => 'Intro', 'bindings' => array( 'content' => array( 'source' => 'core/pattern-overrides' ) ) ) ) ) ),
	array( $public, $block( $pattern ) ),
	array( $public, $block( $pattern, array( 'content' => array( 'Intro' => array( 'content' => 'New <em>intro</em><script>x</script>' ), 'Head' => array( 'content' => 'New heading' ), 'Go' => array( 'url' => 'http://new.test', 'text' => 'Go now', 'linkTarget' => '_blank', 'rel' => 'nofollow' ), 'Mixed' => array( 'content' => 'Mixed override' ), 'Only' => array( 'content' => 'Only override' ), '7' => array( 'content' => 'Seven' ), '1' => array( 'content' => 'One' ), '1.5' => array( 'content' => 'Half' ), 'Item' => array( 'content' => 'New item' ) ) ) ) ),
	array( $public, $block( $pattern, array( 'content' => array( 'Intro' => array( 'values' => array( 'content' => 'Values format' ) ) ) ) ) ),
	array( $public, $block( $pattern, array( 'content' => array( 'Intro' => array( 'content' => null ), 'Go' => array( 'linkTarget' => true, 'rel' => false ) ) ) ) ),
	array( $public, $block( $pattern, array( 'overrides' => array( 'Intro' => array( 'content' => 'Old format' ) ) ) ) ),
	array( $public, $block( $nested, array( 'content' => array( 'Intro' => array( 'content' => 'Outer' ) ) ) ) ),
	array( $public, $block( $looping, array( 'content' => array( 'Intro' => array( 'content' => 'Overridden' ) ) ) ) ),
);
foreach ( $renders as list( $id, $markup ) ) {
	$GLOBALS['post'] = $id ? get_post( $id ) : null;
	echo 'render', "\t", $id, "\t", bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
}
$GLOBALS['post'] = null;

$sources = array();
foreach ( $terms as list( $id, $taxonomy ) ) {
	foreach ( array( 'id', 'name', 'link', 'slug', 'description', 'parent', 'count', 'other', '' ) as $field ) {
		$sources[] = array( 'core/term-data', array( 'name' => 'core/paragraph', 'context' => array( 'termId' => $id, 'taxonomy' => $taxonomy ) ), array( 'field' => $field ) );
	}
}
$sources[] = array( 'core/term-data', array( 'name' => 'core/paragraph', 'context' => array( 'termId' => 999999, 'taxonomy' => 'category' ) ), array( 'field' => 'name' ) );
$sources[] = array( 'core/term-data', array( 'name' => 'core/paragraph', 'context' => array() ), array( 'field' => 'name' ) );
foreach ( array( 'navigation-link', 'navigation-submenu' ) as $nav ) {
	$sources[] = array( 'core/term-data', array( 'name' => "core/$nav", 'attributes' => array( 'id' => $tag['term_id'], 'type' => 'tag' ) ), array( 'field' => 'name' ) );
	$sources[] = array( 'core/term-data', array( 'name' => "core/$nav", 'attributes' => array( 'id' => 1, 'type' => 'category' ) ), array( 'field' => 'link' ) );
	$sources[] = array( 'core/term-data', array( 'name' => "core/$nav", 'attributes' => array( 'id' => 1 ) ), array( 'field' => 'name' ) );
	$sources[] = array( 'core/post-data', array( 'name' => "core/$nav", 'attributes' => array( 'id' => $public ) ), array( 'field' => 'link' ) );
	$sources[] = array( 'core/post-data', array( 'name' => "core/$nav", 'attributes' => array( 'id' => $draft ) ), array( 'field' => 'date' ) );
	$sources[] = array( 'core/post-data', array( 'name' => "core/$nav", 'attributes' => array() ), array( 'field' => 'date' ) );
}
foreach ( $sources as list( $source, $instance, $args ) ) {
	$block  = (object) ( $instance + array( 'attributes' => array(), 'context' => array() ) );
	$value  = get_block_bindings_source( $source )->get_value( $args, $block, 'content' );
	echo 'source', "\t", bin2hex( $source ), "\t", bin2hex( wp_json_encode( $instance ) ), "\t", bin2hex( wp_json_encode( $args ) ), "\t", bin2hex( wp_json_encode( $value ) ), "\n";
}
$wpdb->query( 'ROLLBACK' );
