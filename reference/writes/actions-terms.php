<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's term actions (MVP 2.4), loaded by actions.php. Calls that
// fail write nothing, so each action ends with a write the golden can show.

$tag = function ( $name, $args = array() ) {
	return wp_insert_term( $name, 'post_tag', $args );
};

$category = function ( $name, $parent = 0 ) {
	return wp_insert_term( $name, 'category', array( 'parent' => $parent ) );
};

return array(
	'insert-term-markup'           => array(
		'action' => function () use ( $tag ) {
			// Slashed, as the request gives them.
			$tag(
				"A <b>bold</b> & O\\'Brien\\\\s  tag",
				array( 'description' => "<p>Para</p><script>x()</script> O\\'Brien" )
			);
			$tag( '!!!' );
		},
	),
	'insert-child-category'        => array(
		'setup'  => function () use ( $category ) {
			return $category( 'Parent' )['term_id'];
		},
		'action' => function ( $parent ) use ( $category ) {
			$category( 'Child', $parent );
			$category( 'Second child', $parent );
		},
	),
	'insert-duplicate-tag'         => array(
		'setup'  => function () use ( $tag ) {
			$tag( 'Oracle' );
			$tag( 'Thing', array( 'slug' => 'custom' ) );
			// A tag keeps the parent it is given, but no tag is looked for under it.
			$tag( 'Kid', array( 'parent' => 1 ) );
		},
		'action' => function () use ( $tag ) {
			$tag( 'ORACLE' );
			$tag( 'thing' );
			$tag( 'Kid', array( 'parent' => 1 ) );
			$tag( '   ' );
			$tag( '<b></b>' );
			$tag( 'Missing parent', array( 'parent' => 999 ) );
			$tag( 'Oracle', array( 'slug' => 'oracle-other' ) );
			$tag( 'Another', array( 'slug' => 'oracle' ) );
			$tag( 'Yet another', array( 'slug' => 'oracle' ) );
		},
	),
	'insert-duplicate-category'    => array(
		'setup'  => function () use ( $category, $tag ) {
			$parent = $category( 'Parent' )['term_id'];
			$child  = $category( 'Child', $parent )['term_id'];
			$other  = $category( 'Other' )['term_id'];
			$stray  = $tag( 'Stray' )['term_id'];
			return array( $parent, $child, $other, $stray );
		},
		'action' => function ( $ids ) use ( $category ) {
			list( $parent, $child, $other, $stray ) = $ids;
			$category( 'child', $parent );
			$category( 'Child', $parent );
			wp_insert_term( 'Child', 'category', array( 'parent' => $parent, 'slug' => 'child' ) );
			wp_insert_term( 'Kid', 'category', array( 'parent' => $parent, 'slug' => 'kid' ) );
			wp_insert_term( 'Child', 'category', array( 'parent' => $parent, 'slug' => 'other' ) );
			$category( 'Child' );
			$category( 'Child', $other );
			wp_insert_term( 'Young', 'category', array( 'parent' => $other, 'slug' => 'child' ) );
			wp_insert_term( 'Tot', 'category', array( 'slug' => 'kid-child' ) );
			wp_insert_term( 'Grandchild', 'category', array( 'parent' => $child, 'slug' => 'kid' ) );
			wp_insert_term( 'Stray', 'category', array( 'parent' => $stray, 'slug' => 'kid' ) );
			wp_insert_term( 'Baby', 'category', array( 'parent' => $child, 'slug' => 'child' ) );
			wp_insert_term( 'Tyke', 'category', array( 'parent' => $parent, 'slug' => 'small' ) );
			$category( 'tyke', $parent );
		},
	),
	'insert-alias-tag'             => array(
		'setup'  => function () use ( $tag ) {
			$tag( 'Oracle' );
		},
		'action' => function () use ( $tag ) {
			$tag( 'Synonym', array( 'alias_of' => 'oracle' ) );
			$tag( 'Third', array( 'alias_of' => 'oracle' ) );
			$tag( 'Lonely', array( 'alias_of' => 'missing' ) );
		},
	),
	'insert-zero-slug-tag'         => array(
		'setup'  => function () use ( $tag ) {
			$tag( '0' );
		},
		'action' => function () use ( $tag ) {
			$term = $tag( '0!' );
			wp_set_post_terms( 1, array( $term['term_id'] ), 'post_tag' );
		},
	),
	'update-term'                  => array(
		'setup'  => function () use ( $category ) {
			return $category( 'News' )['term_id'];
		},
		'action' => function ( $id ) {
			wp_update_term( 999, 'category', array( 'name' => 'Nothing' ) );
			wp_update_term( $id, 'category', array( 'name' => ' ' ) );
			wp_update_term( $id, 'category', array( 'parent' => 999 ) );
			wp_update_term(
				$id,
				'category',
				array(
					'name'        => 'Renamed & <i>new</i>',
					'description' => '<em>What</em> is new.',
				)
			);
		},
	),
	'update-term-slug'             => array(
		'setup'  => function () use ( $tag ) {
			return array( $tag( 'One' )['term_id'], $tag( 'Two' )['term_id'], $tag( 'Three' )['term_id'] );
		},
		'action' => function ( $ids ) {
			list( , $two, $three ) = $ids;
			wp_update_term( $two, 'post_tag', array( 'slug' => 'one' ) );
			wp_update_term(
				$two,
				'post_tag',
				array(
					'name' => 'One',
					'slug' => '',
				)
			);
			wp_update_term(
				$three,
				'post_tag',
				array(
					'name' => '!!!',
					'slug' => '',
				)
			);
		},
	),
	'update-term-alias'            => array(
		'setup'  => function () use ( $tag ) {
			return array( $tag( 'Oracle' )['term_id'], $tag( 'Other' )['term_id'] );
		},
		'action' => function ( $ids ) {
			wp_update_term( $ids[1], 'post_tag', array( 'alias_of' => 'oracle' ) );
		},
	),
	'update-category-parent'       => array(
		'setup'  => function () use ( $category ) {
			$alpha = $category( 'Alpha' )['term_id'];
			$beta  = $category( 'Beta', $alpha )['term_id'];
			$gamma = $category( 'Gamma' )['term_id'];
			return array( $alpha, $beta, $gamma );
		},
		'action' => function ( $ids ) {
			list( $alpha, $beta, $gamma ) = $ids;
			wp_update_term( $alpha, 'category', array( 'parent' => $beta ) );
			wp_update_term( $gamma, 'category', array( 'parent' => $gamma ) );
			wp_update_term( $gamma, 'category', array( 'parent' => $beta ) );
			wp_update_term( $beta, 'category', array( 'slug' => 'gamma' ) );
		},
	),
	'break-category-loop'          => array(
		'setup'  => function () use ( $category ) {
			global $wpdb;
			$c = $category( 'Loop C' );
			$d = $category( 'Loop D', $c['term_id'] );
			$wpdb->update( $wpdb->term_taxonomy, array( 'parent' => $d['term_id'] ), array( 'term_taxonomy_id' => $c['term_taxonomy_id'] ) );
			// A new request reads the terms afresh.
			wp_cache_flush();
			$e = $category( 'Loop E' );
			return array( $c['term_id'], $e['term_id'] );
		},
		'action' => function ( $ids ) {
			wp_update_term( $ids[1], 'category', array( 'parent' => $ids[0] ) );
		},
	),
	'delete-tag'                   => array(
		'setup'  => function () use ( $tag ) {
			$id = $tag( 'Oracle' )['term_id'];
			wp_set_post_terms( 1, array( $id ), 'post_tag' );
			wp_set_object_terms( 2, array( $id ), 'post_tag' );
			add_term_meta( $id, 'color', 'red' );
			return $id;
		},
		'action' => function ( $id ) {
			wp_delete_term( 999, 'post_tag' );
			wp_delete_term( $id, 'post_tag' );
		},
	),
	'delete-shared-term'           => array(
		'setup'  => function () use ( $tag ) {
			global $wpdb;
			$id = $tag( 'Shared' )['term_id'];
			// A term two taxonomies share, as before WordPress 4.2.
			$wpdb->insert(
				$wpdb->term_taxonomy,
				array(
					'term_id'     => $id,
					'taxonomy'    => 'category',
					'description' => '',
					'parent'      => 0,
					'count'       => 0,
				)
			);
			return $id;
		},
		'action' => function ( $id ) {
			wp_delete_term( $id, 'post_tag' );
		},
	),
	'delete-category'              => array(
		'setup'  => function () use ( $category ) {
			$news  = $category( 'News' )['term_id'];
			$child = $category( 'Local', $news )['term_id'];
			$other = $category( 'Other' )['term_id'];
			wp_set_post_categories( 1, array( $news ) );
			wp_set_object_terms( 2, array( $news, $other ), 'category' );
			return $news;
		},
		'action' => function ( $news ) {
			wp_delete_term( 1, 'category' );
			wp_delete_term( $news, 'category' );
		},
	),
	'delete-category-no-default'   => array(
		'setup'  => function () use ( $category ) {
			$news = $category( 'News' )['term_id'];
			wp_set_post_categories( 1, array( $news ) );
			update_option( 'default_category', 999 );
			return $news;
		},
		'action' => function ( $news ) {
			wp_delete_term( $news, 'category' );
		},
	),
	'set-post-categories'          => array(
		'setup'  => function () use ( $category ) {
			global $wpdb;
			$news = $category( 'News' )['term_id'];
			wp_set_post_categories( 1, array( $news ) );
			$wpdb->insert(
				$wpdb->posts,
				array(
					'post_author'           => 1,
					'post_date'             => '2026-01-01 00:00:00',
					'post_date_gmt'         => '2026-01-01 00:00:00',
					'post_content'          => '',
					'post_content_filtered' => '',
					'post_title'            => 'Auto Draft',
					'post_excerpt'          => '',
					'post_status'           => 'auto-draft',
					'post_type'             => 'post',
					'comment_status'        => 'open',
					'ping_status'           => 'open',
					'post_password'         => '',
					'post_name'             => '',
					'to_ping'               => '',
					'pinged'                => '',
					'post_modified'         => '2026-01-01 00:00:00',
					'post_modified_gmt'     => '2026-01-01 00:00:00',
					'post_parent'           => 0,
					'menu_order'            => 0,
					'post_mime_type'        => '',
					'guid'                  => '',
				)
			);
			return array( $news, $wpdb->insert_id );
		},
		'action' => function ( $ids ) {
			list( $news, $draft ) = $ids;
			wp_set_post_categories( $draft, array() );
			wp_set_post_categories( 2, array() );
			wp_set_post_categories( 1, array() );
			wp_set_post_categories( 1, array( (string) $news, 'Missing', $news ), true );
			wp_set_post_categories( 1, array( '' ) );
		},
	),
	'set-post-tags'                => array(
		'action' => function () {
			wp_set_post_terms( 0, 'zero', 'post_tag' );
			wp_set_post_terms( 1, ' alpha, Beta,,', 'post_tag' );
			wp_set_post_terms( 1, array( 'alpha', 'gamma', 'beta ' ), 'post_tag' );
			wp_set_post_terms( 1, array( 'delta', 999, '' ), 'post_tag', true );
			wp_set_post_terms( 2, '0', 'post_tag' );
			wp_set_post_terms( 2, array( '???' ), 'post_tag' );
			wp_set_post_terms( 2, 'delta,gamma', 'post_tag', true );
		},
	),
	'remove-post-tags'             => array(
		'setup'  => function () {
			wp_set_post_terms( 1, 'alpha, beta, gamma', 'post_tag' );
		},
		'action' => function () {
			wp_remove_object_terms( 1, array( 'missing', '', '\\ ' ), 'post_tag' );
			wp_remove_object_terms( 1, array(), 'post_tag' );
			wp_remove_object_terms( 2, array( 'alpha' ), 'post_tag' );
			wp_remove_object_terms( 1, array( 'alpha', 'Beta', 'missing' ), 'post_tag' );
		},
	),
);
