<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for latest posts block markup over the default
// install, on its home page. Lines are
//   hex(markup)<TAB>hex(html)
// The database is only read.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

function latest( $attrs ) {
	$json = $attrs ? ' ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES ) : '';
	return "<!-- wp:latest-posts$json /-->";
}

$cases = array(
	latest( array() ),
	latest( array( 'displayPostContent' => true ) ),
	latest( array( 'displayPostContent' => true, 'excerptLength' => 5 ) ),
	latest( array( 'displayPostContent' => true, 'excerptLength' => 14 ) ),
	latest( array( 'displayPostContent' => true, 'excerptLength' => 13 ) ),
	latest( array( 'displayPostContent' => true, 'displayPostContentRadio' => 'full_post' ) ),
	latest( array( 'displayPostContent' => false, 'displayPostContentRadio' => 'full_post' ) ),
	latest( array( 'displayPostContent' => true, 'displayPostContentRadio' => 'other' ) ),
	latest( array( 'displayAuthor' => true ) ),
	latest( array( 'displayPostDate' => true ) ),
	latest( array( 'displayAuthor' => true, 'displayPostDate' => true, 'displayPostContent' => true, 'excerptLength' => 3 ) ),
	latest( array( 'order' => 'asc', 'orderBy' => 'title' ) ),
	latest( array( 'order' => 'ASC', 'orderBy' => 'nope' ) ),
	latest( array( 'categories' => array( array( 'id' => 1 ) ) ) ),
	latest( array( 'categories' => array( array( 'id' => 99 ) ) ) ),
	latest( array( 'categories' => array( array( 'name' => 'x' ) ) ) ),
	latest( array( 'categories' => '1' ) ),
	latest( array( 'categories' => '99' ) ),
	latest( array( 'selectedAuthor' => 1 ) ),
	latest( array( 'selectedAuthor' => 2 ) ),
	latest( array( 'postsToShow' => 0 ) ),
	latest( array( 'postsToShow' => -1 ) ),
	latest( array( 'postsToShow' => -3 ) ),
	latest( array( 'postsToShow' => 1 ) ),
	latest( array( 'layout' => array( 'type' => 'grid', 'columnCount' => 3 ) ) ),
	latest( array( 'layout' => array( 'type' => 'grid', 'columnCount' => 3, 'minimumColumnWidth' => '12rem' ) ) ),
	latest( array( 'layout' => array( 'type' => 'grid' ) ) ),
	latest( array( 'layout' => array( 'type' => 'flex' ), 'postLayout' => 'grid' ) ),
	latest( array( 'postLayout' => 'grid', 'columns' => 2 ) ),
	latest( array( 'postLayout' => 'grid', 'columns' => 2.5 ) ),
	latest( array( 'postLayout' => 'list', 'columns' => 2 ) ),
	latest( array( 'style' => array( 'elements' => array( 'link' => array( 'color' => array( 'text' => '#f00' ) ) ) ) ) ),
	latest( array( 'align' => 'wide', 'className' => 'extra', 'fontSize' => 'small', 'textColor' => 'contrast' ) ),
	latest( array( 'displayFeaturedImage' => true ) ),
);

$GLOBALS['wp'] = new WP();
wp( '' );
foreach ( $cases as $markup ) {
	echo bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
}
