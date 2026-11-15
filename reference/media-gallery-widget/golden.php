<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for the gallery widget in a legacy widget block (signed
// under the fixed salt `test-salt`), over attachments inserted for it and
// rolled back after, outside the loop and in a singular post's loop. Lines are
//   hex(markup)<TAB>hex(request)<TAB>hex(state JSON)<TAB>hex(html)
// where the state holds the attachments (ID, MIME type, excerpt, GUID, title,
// status and their meta) and the global post's ID. gallery_shortcode()
// numbers its galleries with a static counter, which runs on through every
// case of the run.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

global $wpdb;
$wpdb->query( 'START TRANSACTION' );
add_filter( 'salt', static fn() => 'test-salt' );

$attach = static function ( $file, $mime, $excerpt, $meta, $extra = array() ) {
	$id = wp_insert_attachment(
		array_merge(
			array(
				'post_title'     => 'Attached',
				'post_excerpt'   => $excerpt,
				'post_mime_type' => $mime,
				'guid'           => '' === $file ? '' : 'http://127.0.0.1:8081/wp-content/uploads/' . $file,
			),
			$extra
		),
		$file
	);
	if ( null !== $meta ) {
		update_post_meta( $id, '_wp_attachment_metadata', $meta );
	}
	return $id;
};
$size     = static fn( $file, $w, $h ) => array(
	'file'      => $file,
	'width'     => $w,
	'height'    => $h,
	'mime-type' => 'image/jpeg',
);
$photo    = $attach(
	'2026/01/photo.jpg',
	'image/jpeg',
	"It's a \"photo\" & <b>more</b>",
	array(
		'width'  => 1200,
		'height' => 800,
		'file'   => '2026/01/photo.jpg',
		'sizes'  => array(
			'thumbnail' => $size( 'photo-150x150.jpg', 150, 150 ),
			'medium'    => $size( 'photo-300x200.jpg', 300, 200 ),
		),
	)
);
$portrait = $attach(
	'2026/01/tall.jpg',
	'image/jpeg',
	'  ',
	array(
		'width'  => 600,
		'height' => 900,
		'file'   => '2026/01/tall.jpg',
		'sizes'  => array(
			'thumbnail' => $size( 'tall-150x150.jpg', 150, 150 ),
		),
	),
	array( 'post_title' => 'B tall' )
);
$square   = $attach( '2026/01/square.png', 'image/png', '', null, array( 'post_title' => 'A square' ) );
$untitled = $attach( '2026/01/untitled.gif', 'image/gif', '', null, array( 'post_title' => '' ) );
$missing  = $attach( '', 'image/jpeg', 'Gone', null );
$private  = $attach( '2026/01/private.jpg', 'image/jpeg', '', null, array( 'post_status' => 'private' ) );
$pdf      = $attach( '2026/01/doc.pdf', 'application/pdf', '', null );
// No file and no GUID: wp_get_attachment_url() has nothing to give.
$noguid = $attach( '', 'image/jpeg', '', null );
$wpdb->update( $wpdb->posts, array( 'guid' => '' ), array( 'ID' => $noguid ) );
clean_post_cache( $noguid );
// PHP's basename() splits a path at slashes only.
$backslash = $attach( '2026/01/a\\b.gif', 'image/gif', '', null, array( 'post_title' => '' ) );
$wpdb->update( $wpdb->postmeta, array( 'meta_value' => '2026/01/a\\b.gif' ), array( 'post_id' => $backslash, 'meta_key' => '_wp_attached_file' ) );
wp_cache_delete( $backslash, 'post_meta' );
// pathinfo() takes the file name of a path with no extension, or ending
// in a slash, whole.
$noext = $attach( '2026/01/noext', 'image/jpeg', '', null, array( 'post_title' => '' ) );
$trail = $attach( '2026/01/trail.d/', 'image/jpeg', '', null, array( 'post_title' => '' ) );
$nullh     = $attach( '2026/01/nullh.jpg', 'image/jpeg', '', array( 'width' => 100, 'height' => null ), array( 'post_title' => 'C nullh' ) );
$even      = $attach( '2026/01/even.jpg', 'image/jpeg', '', array( 'width' => 100, 'height' => 100 ), array( 'post_title' => 'D even' ) );

function widget( $instance ) {
	$serialized = serialize( $instance );
	$attrs      = array(
		'idBase'   => 'media_gallery',
		'instance' => array(
			'encoded' => base64_encode( $serialized ),
			'hash'    => wp_hash( $serialized ),
		),
	);
	return '<!-- wp:legacy-widget ' . wp_json_encode( $attrs ) . ' /-->';
}
$three = array( $photo, $portrait, $square );
$cases = array(
	widget( array() ),
	widget( array( 'ids' => array() ) ),
	widget( array( 'ids' => $three ) ),
	widget( array( 'ids' => "$portrait, $photo", 'link_type' => 'file', 'columns' => 2, 'size' => 'medium', 'title' => 'Gallery & <b>x</b>' ) ),
	widget( array( 'ids' => array( $photo ), 'link_type' => 'none', 'size' => 'full' ) ),
	widget( array( 'ids' => array( $photo, $pdf ) ) ),
	widget( array( 'ids' => array( $photo, 1 ) ) ),
	widget( array( 'ids' => array( $photo, 999999 ) ) ),
	widget( array( 'ids' => array( $private ), 'title' => 'Private' ) ),
	widget( array( 'ids' => array( $photo ), 'orderby_random' => true ) ),
	widget( array( 'ids' => $three, 'orderby' => 'ID', 'order' => 'DESC', 'itemtag' => 'li', 'icontag' => 'SPAN', 'captiontag' => 'bogus<x>' ) ),
	widget( array( 'ids' => $three, 'orderby' => 'title', 'order' => 'asc', 'itemtag' => 'nope', 'icontag' => 'nope', 'captiontag' => '' ) ),
	widget( array( 'ids' => array( $photo ), 'size' => 'custom' ) ),
	widget( array( 'ids' => array( $photo ), 'size' => array( 100, 100 ) ) ),
	widget( array( 'ids' => array( $photo, $square ), 'size' => '0' ) ),
	widget( array( 'ids' => array( $photo ), 'size' => 'none', 'link_type' => 'post' ) ),
	widget( array( 'ids' => array( $untitled ), 'size' => '' ) ),
	widget( array( 'ids' => array( $missing ) ) ),
	widget( array( 'ids' => array( $missing ), 'link_type' => 'none' ) ),
	widget( array( 'ids' => array( $photo ), 'id' => 42, 'columns' => 'abc' ) ),
	widget( array( 'ids' => array( $photo ), 'columns' => 0, 'size' => 'a b%20c' ) ),
	widget( array( 'ids' => array( $photo, $photo, $portrait ) ) ),
	widget( array( 'ids' => array( $photo, array( 1 ), (string) $portrait ) ) ),
	widget( array( 'ids' => " $square ,,\t$photo" ) ),
	widget( array( 'ids' => array( $photo ) ) ) . widget( array( 'ids' => array( $square ) ) ),
	widget( array( 'ids' => array( array( $photo ) ) ) ),
	widget( array( 'ids' => '0' ) ),
	widget( array( 'ids' => $three, 'orderby' => 'bogus' ) ),
	widget( array( 'ids' => $three, 'orderby' => 'title', 'order' => array( 'ASC' ) ) ),
	widget( array( 'ids' => array( $photo ), 'size' => array( '0', 300 ), 'link_type' => 'none' ) ),
	widget( array( 'ids' => array( 1.9, true, $photo ) ) ),
	widget( array( 'ids' => array( 0 ) ) ),
	widget( array( 'ids' => array( $photo ), 'link_type' => array( 'file' ) ) ),
	widget( array( 'ids' => array( $noguid ) ) ),
	widget( array( 'ids' => array( $photo ), 'size' => array( 100, 100, 1 ) ) ),
	widget( array( 'ids' => array( $photo ), 'size' => array( 0, 300 ) ) ),
	widget( array( 'ids' => array( $backslash ), 'size' => 'none' ) ),
	widget( array( 'ids' => ' , ' ) ),
	widget( array( 'ids' => array( $square, $photo ), 'orderby_random' => false, 'orderby' => 'title' ) ),
	widget( array( 'ids' => array( $nullh, $even ) ) ),
	widget( array( 'ids' => array( $noext, $trail ), 'size' => 'none' ) ),
);

$attachments = array();
foreach ( array( $photo, $portrait, $square, $untitled, $missing, $private, $pdf, $noguid, $backslash, $noext, $trail, $nullh, $even ) as $id ) {
	$a             = get_post( $id );
	$attachments[] = array( $id, $a->post_mime_type, $a->post_excerpt, $a->guid, $a->post_title, $a->post_status, get_post_meta( $id ) );
}

// Not $request: WordPress sets that global.
foreach ( array( '', 'p=1' ) as $query ) {
	$GLOBALS['wp'] = new WP();
	wp( $query );
	if ( is_singular() ) {
		// As get_the_block_template_html() renders a singular template.
		$GLOBALS['wp_query']->the_post();
	}
	$state = array(
		'attachments' => $attachments,
		'post'        => get_post() ? get_post()->ID : 0,
	);
	foreach ( $cases as $markup ) {
		wp_increase_content_media_count( -wp_increase_content_media_count( 0 ) );
		wp_high_priority_element_flag( true );
		echo bin2hex( $markup ), "\t", bin2hex( $query ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
	}
}
// With no IDs to include, WP_Query lists as many as `posts_per_page` says.
foreach ( array( -1, 0, -3 ) as $per_page ) {
	update_option( 'posts_per_page', $per_page );
	$state['posts_per_page'] = $per_page;
	$markup                  = widget( array( 'ids' => array( array( $photo ) ) ) );
	wp_increase_content_media_count( -wp_increase_content_media_count( 0 ) );
	wp_high_priority_element_flag( true );
	echo bin2hex( $markup ), "\t", bin2hex( $query ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
}
$wpdb->query( 'ROLLBACK' );
