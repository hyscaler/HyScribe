<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// How the main query reads `category_name`, `tag`, `post_format`,
// `taxonomy` and `term`, over terms and posts inserted for it and rolled
// back after, for crates/hyscribe-http/tests/golden/tax-query.tsv. The first
// line is the data, `data`<TAB>hex(JSON): the category hierarchy, every
// term and each post's terms. Then one line per request,
//   hex(query string)<TAB>hex(state JSON)
// with the archive flags, the queried term, the posts found and the
// templates the template loader tries, up to the first the theme has.

require '/var/www/html/wp-load.php';

global $wp, $wpdb;
$wpdb->query( 'START TRANSACTION' );
// A category ID lower than the others' that no longer exists.
$gone = wp_insert_term( 'Gone', 'category' )['term_id'];
wp_delete_term( $gone, 'category' );
$parent = wp_insert_term( 'Parent', 'category' )['term_id'];
$child  = wp_insert_term( 'Child', 'category', array( 'parent' => $parent ) )['term_id'];
_get_term_hierarchy( 'category' );
$insert = function ( $title, $categories, $tags ) {
	return wp_insert_post(
		array(
			'post_title'    => $title,
			'post_status'   => 'publish',
			'post_category' => $categories,
			'tags_input'    => $tags,
		)
	);
};
wp_set_post_tags( 1, array( 'Foo', 'Bar' ) );
set_post_format( 1, 'aside' );
$second = $insert( 'Second', array( $child ), array( 'Foo', '10' ) );
$third  = $insert( 'Third', array( $parent ), array( '9' ) );
set_post_format( $third, 'chat' );
// Slugs that remove_accents() made.
wp_set_post_tags( $second, array( 'Café' ), true );
wp_insert_term( 'Été', 'category' );
// A tag sharing a category's slug, and a term of a taxonomy WordPress does
// not have, as a removed plugin leaves behind.
wp_set_post_tags( $third, array( 'Child' ), true );
$wpdb->insert( $wpdb->terms, array( 'name' => 'Fiction', 'slug' => 'fiction' ) );
$fiction = $wpdb->insert_id;
$wpdb->insert( $wpdb->term_taxonomy, array( 'term_id' => $fiction, 'taxonomy' => 'genre', 'description' => '' ) );
$wpdb->insert( $wpdb->term_relationships, array( 'object_id' => 1, 'term_taxonomy_id' => $wpdb->insert_id ) );

$terms = array();
foreach ( get_terms( array( 'hide_empty' => false, 'taxonomy' => array( 'category', 'post_tag', 'post_format' ) ) ) as $term ) {
	$terms[] = array( $term->taxonomy, $term->term_id, $term->slug, $term->parent );
}
$terms[] = array( 'genre', $fiction, 'fiction', 0 );
$posts = array();
foreach ( array( 1, $second, $third ) as $id ) {
	$posts[ $id ] = wp_get_object_terms( $id, array( 'category', 'post_tag', 'post_format' ), array( 'fields' => 'ids' ) );
}
$posts[1][] = $fiction;
$data = array(
	'hierarchy' => maybe_serialize( get_option( 'category_children' ) ),
	'terms'     => $terms,
	'posts'     => $posts,
);
echo 'data', "\t", bin2hex( wp_json_encode( $data ) ), "\n";

$cases = array(
	'category_name=uncategorized', 'category_name=Uncategorized', 'category_name=nope', 'category_name=nope/uncategorized',
	'category_name=uncategorized/', 'category_name=child', 'category_name=parent', 'category_name=parent,child',
	'category_name=child,parent', 'category_name=uncategorized,nope', 'category_name=nope,uncategorized',
	'category_name=uncategorized+nope', 'category_name=uncategorized%20parent', 'category_name=parent+child', 'category_name=,',
	'category_name=0', 'category_name=%20child%20,parent', 'category_name=child,,parent', 'category_name=a%5Cchild',
	'category_name=10,9,parent', 'category_name=', 'category_name=Child%20', 'category_name=%2B', 'category_name=/', 'category_name=0/',
	'taxonomy=category&term=uncategorized', 'taxonomy=category&term=parent', 'taxonomy=category&term=nope',
	'taxonomy=category&term=parent,child', 'taxonomy=post_tag&term=foo', 'taxonomy=post_tag&term=nope',
	'taxonomy=post_format&term=post-format-aside', 'taxonomy=post_format&term=aside', 'taxonomy=nav_menu&term=x',
	'taxonomy=wp_theme&term=twentytwentyfive', 'taxonomy=foo&term=x', 'taxonomy=category', 'term=foo', 'taxonomy=0&term=x',
	'taxonomy=category&term=0', 'taxonomy=category&term=child&category_name=uncategorized',
	'taxonomy=post_tag&term=foo&tag=bar', 'taxonomy=post_format&term=post-format-chat&post_format=aside',
	'tag=foo', 'tag=Foo', 'tag=nope', 'tag=foo,bar', 'tag=bar,foo', 'tag=foo+bar', 'tag=foo+nope', 'tag=nope,foo',
	'tag=foo,nope', 'tag=0', 'tag=10,9', 'tag=9,10', 'tag=foo%09bar', 'tag=foo&cat=1', 'tag=foo&cat=-1', 'tag=foo,bar&cat=1',
	'tag=Foo%20Bar', 'tag=foo,,bar', 'tag=,foo', 'tag=foo++bar', 'tag=10', 'tag=%20', 'tag=foo&category_name=child',
	'tag=10&category_name=parent', 'tag=foo%2Cbar', 'tag=foo%0Abar', 'tag=foo,bar+10', 'tag=FOO+bar', 'tag=caf%C3%A9',
	'post_format=aside', 'post_format=chat', 'post_format=standard', 'post_format=aside,chat', 'post_format=post-format-aside',
	'post_format=post-format-aside,post-format-chat', 'post_format=post-format-chat+post-format-aside', 'post_format=',
	'post_format=0', 'post_format=Aside', 'post_format=aside&cat=1', 'post_format=chat&category_name=parent',
	's=sample&post_format=', 's=hello&tag=foo', 'm=2026&tag=foo', 'p=1&tag=nope', 'cat=1&category_name=nope',
	'cat=1&category_name=child', 'cat=-1&category_name=parent', 'cat=1&taxonomy=category&term=child', "cat=$gone,$parent",
	'taxonomy=category&term=nope&cat=1,,2', 'taxonomy=genre&term=fiction', 'tag=child&category_name=child', 'tag=child', 'post_format=chat&tag=child',
	'tag=Caf%C3%A9', 'tag=cafe%CC%81', 'tag=caf%C3%A9,foo', 'tag=caf%C3%A9+foo', 'category_name=%C3%89t%C3%A9',
	'taxonomy=post_tag&term=caf%C3%A9', 'taxonomy=category&term=%C3%A9t%C3%A9', 'tag=%C3%A9t%C3%A9&category_name=caf%C3%A9',
);
foreach ( $cases as $case ) {
	$_GET = array();
	parse_str( $case, $_GET );
	$_SERVER['QUERY_STRING'] = $case;
	$_SERVER['REQUEST_URI']  = '/?' . $case;
	$wp->parse_request();
	// The constructor runs no query for no vars, where the main query does.
	$query = new WP_Query();
	$query->query( $wp->query_vars );
	$object = $query->get_queried_object();
	$GLOBALS['wp_query']     = $query;
	$GLOBALS['wp_the_query'] = $query;
	$templates               = array();
	$record                  = function ( $hierarchy ) use ( &$templates ) {
		$templates = array_merge( $templates, $hierarchy );
		return $hierarchy;
	};
	// template-loader.php's order, for the conditions a list can meet.
	$getters = array(
		'is_search'   => 'search',
		'is_tax'      => 'taxonomy',
		'is_category' => 'category',
		'is_tag'      => 'tag',
		'is_date'     => 'date',
		'is_archive'  => 'archive',
	);
	foreach ( $getters as $condition => $type ) {
		add_filter( "{$type}_template_hierarchy", $record );
		$found = $query->$condition() && call_user_func( "get_{$type}_template" );
		remove_filter( "{$type}_template_hierarchy", $record );
		if ( $found ) {
			break;
		}
	}
	$state  = array(
		'category' => $query->is_category,
		'tag'      => $query->is_tag,
		'tax'      => $query->is_tax,
		'queried'  => $object instanceof WP_Term ? array( $object->taxonomy, $object->term_id ) : null,
		'posts'     => array_map( 'intval', wp_list_pluck( $query->posts, 'ID' ) ),
		'templates' => array_map( fn( $t ) => basename( $t, '.php' ), $templates ),
	);
	echo bin2hex( $case ), "\t", bin2hex( wp_json_encode( $state ) ), "\n";
}
$wpdb->query( 'ROLLBACK' );
