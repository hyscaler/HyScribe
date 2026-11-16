<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints the text widget in a legacy widget block (signed under the fixed
// salt `test-salt`), over an attachment inserted for it and rolled back
// after. Lines are
//   hex(markup)<TAB>hex(mode)<TAB>hex(state JSON)<TAB>hex(html)
// where the mode is `` (do_blocks() outside the loop), `loop` (do_blocks()
// in a singular post's loop), `content` (the_content of that post) or
// `attachment` (the_content on the attachment's own page, which
// prepend_attachment() adds to), and the state holds the attachment (ID,
// MIME type, excerpt, GUID and its meta). Each case starts with the content
// media count and the high priority flag reset.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

global $wpdb;
$wpdb->query( 'START TRANSACTION' );
add_filter( 'salt', static fn() => 'test-salt' );

$size  = static fn( $file, $w, $h ) => array(
	'file'      => $file,
	'width'     => $w,
	'height'    => $h,
	'mime-type' => 'image/jpeg',
);
$photo = wp_insert_attachment(
	array(
		'post_title'     => 'Attached',
		'post_mime_type' => 'image/jpeg',
		'guid'           => 'http://127.0.0.1:8081/wp-content/uploads/2026/01/photo.jpg',
	),
	'2026/01/photo.jpg'
);
update_post_meta(
	$photo,
	'_wp_attachment_metadata',
	array(
		'width'  => 1200,
		'height' => 800,
		'file'   => '2026/01/photo.jpg',
		'sizes'  => array(
			'medium' => $size( 'photo-300x200.jpg', 300, 200 ),
			'large'  => $size( 'photo-1024x683.jpg', 1024, 683 ),
		),
	)
);

function widget( $instance, $id_base = 'text' ) {
	$serialized = serialize( $instance );
	$attrs      = array(
		'idBase'   => $id_base,
		'instance' => array(
			'encoded' => base64_encode( $serialized ),
			'hash'    => wp_hash( $serialized ),
		),
	);
	return '<!-- wp:legacy-widget ' . wp_json_encode( $attrs ) . ' /-->';
}
$img    = '<img src="http://127.0.0.1:8081/wp-content/uploads/2026/01/photo-1024x683.jpg" width="1024" height="683" class="wp-image-' . $photo . '">';
$visual = array(
	'text'   => "It's \"wordpress\" :)\n\n$img",
	'filter' => true,
	'visual' => true,
);
$image  = "<!-- wp:image {\"id\":$photo,\"sizeSlug\":\"large\"} -->\n<figure class=\"wp-block-image size-large\"><img src=\"http://127.0.0.1:8081/wp-content/uploads/2026/01/photo-1024x683.jpg\" alt=\"\" class=\"wp-image-$photo\"/></figure>\n<!-- /wp:image -->";
$cases  = array(
	widget( $visual ),
	widget( array( 'text' => "one\n\ntwo $img", 'filter' => 'content' ) ),
	widget( array( 'text' => "one\n\ntwo $img", 'filter' => true ) ),
	widget( array( 'text' => "one $img" ) ),
	// An image block before the widget takes the high priority.
	$image . widget( $visual ),
	widget( $visual ) . $image,
	// Inside a block widget, whose own pass gives the image its attributes.
	widget( array( 'content' => widget( $visual ) ), 'block' ),
);

$state = array( 'attachments' => array( array( $photo, 'image/jpeg', '', get_post( $photo )->guid, get_post_meta( $photo ) ) ) );

// Not $request: WordPress sets that global.
foreach ( array( '', 'loop', 'content', 'attachment' ) as $mode ) {
	$GLOBALS['wp'] = new WP();
	wp( 'attachment' === $mode ? "attachment_id=$photo" : ( '' === $mode ? '' : 'p=1' ) );
	if ( is_singular() ) {
		// As get_the_block_template_html() renders a singular template.
		$GLOBALS['wp_query']->the_post();
	}
	foreach ( $cases as $markup ) {
		wp_increase_content_media_count( -wp_increase_content_media_count( 0 ) );
		wp_high_priority_element_flag( true );
		$html = in_array( $mode, array( 'content', 'attachment' ), true ) ? apply_filters( 'the_content', $markup ) : do_blocks( $markup );
		echo bin2hex( $markup ), "\t", bin2hex( $mode ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( $html ), "\n";
	}
}
$wpdb->query( 'ROLLBACK' );
