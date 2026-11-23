<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for the image widget in a legacy widget block (signed
// under the fixed salt `test-salt`), over attachments inserted for it and
// rolled back after, outside the loop and in a singular post's loop. Lines are
//   hex(markup)<TAB>hex(request)<TAB>hex(state JSON)<TAB>hex(html)
// where the state holds the attachments (ID, MIME type, excerpt, GUID and
// their meta) and the intermediate image sizes. Each case starts with the
// content media count and the high priority flag reset, as on a new request.
// A case PHP stops with an error on prints the error's class as its html.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

global $wpdb;
$wpdb->query( 'START TRANSACTION' );
add_filter( 'salt', static fn() => 'test-salt' );

$attach = static function ( $file, $mime, $excerpt, $meta, $alt = null ) {
	$id = wp_insert_attachment(
		array(
			'post_title'     => 'Attached',
			'post_excerpt'   => $excerpt,
			'post_mime_type' => $mime,
			'guid'           => 'http://127.0.0.1:8081/wp-content/uploads/' . $file,
		),
		$file
	);
	if ( null !== $meta ) {
		update_post_meta( $id, '_wp_attachment_metadata', $meta );
	}
	if ( null !== $alt ) {
		update_post_meta( $id, '_wp_attachment_image_alt', $alt );
	}
	return $id;
};
$size  = static fn( $file, $w, $h ) => array(
	'file'      => $file,
	'width'     => $w,
	'height'    => $h,
	'mime-type' => 'image/jpeg',
);
$photo = $attach(
	'2026/01/photo.jpg',
	'image/jpeg',
	'Stored caption',
	array(
		'width'  => 1200,
		'height' => 800,
		'file'   => '2026/01/photo.jpg',
		'sizes'  => array(
			'thumbnail'    => $size( 'photo-150x150.jpg', 150, 150 ),
			'medium'       => $size( 'photo-300x200.jpg', 300, 200 ),
			'medium_large' => $size( 'photo-768x512.jpg', 768, 512 ),
			'large'        => $size( 'photo-1024x683.jpg', 1024, 683 ),
		),
	),
	'Stored alt'
);
// No `file` in the metadata: a zero side compares with the `full` size's
// ratio; two sizes alike, the first wins an exact match.
$nofile = $attach(
	'2026/01/nofile.jpg',
	'image/jpeg',
	'',
	array(
		'sizes' => array(
			'a'      => $size( 'nofile-a.jpg', 600, 400 ),
			'b'      => $size( 'nofile-b.jpg', 600, 400 ),
			'full'   => $size( 'nofile-full.jpg', 1200, 600 ),
			'medium' => $size( 'nofile-300x150.jpg', 300, 150 ),
		),
	)
);
// A `file` and a `full` size: a zero side compares with the original's
// ratio, not the `full` size's.
$withfull = $attach(
	'2026/01/withfull.jpg',
	'image/jpeg',
	'',
	array(
		'width'  => 1000,
		'height' => 1000,
		'file'   => '2026/01/withfull.jpg',
		'sizes'  => array(
			'full'   => $size( 'withfull-full.jpg', 1200, 600 ),
			'medium' => $size( 'withfull-300x150.jpg', 300, 150 ),
			'square' => $size( 'withfull-200x200.jpg', 200, 200 ),
		),
	)
);
$plain = $attach( '2026/01/plain.png', 'image/png', '', null );
$pdf   = $attach( '2026/01/doc.pdf', 'application/pdf', 'A document', null );

function widget( $instance ) {
	$serialized = serialize( $instance );
	$attrs      = array(
		'idBase'   => 'media_image',
		'instance' => array(
			'encoded' => base64_encode( $serialized ),
			'hash'    => wp_hash( $serialized ),
		),
	);
	return '<!-- wp:legacy-widget ' . wp_json_encode( $attrs ) . ' /-->';
}
$url   = 'https://example.org/a.jpg?x=1&y=2';
$cases = array(
	widget( array() ),
	widget( array( 'attachment_id' => $photo ) ),
	widget( array( 'attachment_id' => $photo, 'caption' => null ) ),
	widget( array( 'attachment_id' => $photo, 'caption' => '  ' ) ),
	widget( array( 'attachment_id' => (string) $photo, 'size' => 'full', 'alt' => 'Given & alt', 'image_title' => 'T"t', 'image_classes' => 'one two', 'link_type' => 'file' ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'thumbnail', 'link_type' => 'post', 'link_classes' => 'lc', 'link_rel' => 'noopener', 'link_target_blank' => true, 'title' => 'Photo <b>x</b> & "q"' ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'medium_large', 'caption' => 'Given <script>x</script><em>c</em>' ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'large', 'link_type' => 'custom', 'link_url' => 'http://x.test/a b', 'title' => '0' ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'custom', 'width' => 400, 'height' => 300, 'caption' => 'Custom' ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'custom', 'width' => 300, 'height' => 200 ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'custom', 'width' => 600, 'height' => 400 ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'custom', 'width' => 100, 'height' => 100 ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'custom', 'width' => 140, 'height' => 120 ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'custom', 'width' => 0, 'height' => 0 ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'custom', 'width' => 250, 'height' => 0 ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'custom', 'width' => 2000, 'height' => 1000 ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'bogus', 'caption' => 'No width' ) ),
	widget( array( 'attachment_id' => $photo, 'size' => '1536x1536' ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'full', 'caption' => 'Full' ) ),
	widget( array( 'attachment_id' => $plain, 'caption' => 'Plain' ) ),
	widget( array( 'attachment_id' => $plain, 'size' => 'full', 'link_type' => 'file' ) ),
	widget( array( 'attachment_id' => $pdf ) ),
	widget( array( 'attachment_id' => $pdf, 'url' => $url, 'link_type' => 'post' ) ),
	widget( array( 'attachment_id' => 999999, 'url' => $url ) ),
	widget( array( 'attachment_id' => 'abc', 'url' => $url, 'caption' => '0' ) ),
	widget( array( 'url' => $url, 'alt' => 'A <b>', 'width' => 640, 'height' => 480, 'link_type' => 'file', 'caption' => 'Cap', 'image_classes' => 'x' ) ),
	widget( array( 'url' => $url, 'width' => 300, 'height' => 200, 'link_type' => 'custom', 'link_url' => 'javascript:alert(1)', 'link_rel' => 'r', 'link_target_blank' => '1' ) ),
	widget( array( 'url' => $url, 'width' => 100, 'height' => 50, 'title' => 'Small' ) ),
	widget( array( 'url' => $url, 'width' => 300, 'height' => 200, 'caption' => 'Wide <i>c</i>' ) ),
	widget( array( 'url' => $url ) ),
	widget( array( 'url' => $url, 'width' => null, 'height' => null ) ),
	widget( array( 'attachment_id' => $nofile, 'size' => 'custom', 'width' => 600, 'height' => 400 ) ),
	widget( array( 'attachment_id' => $nofile, 'size' => 'custom', 'width' => 200, 'height' => 0 ) ),
	widget( array( 'attachment_id' => $plain, 'size' => 'full', 'caption' => 'Plain full' ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 5, 'link_type' => array( 'file' ) ) ),
	widget( array( 'attachment_id' => -$photo, 'url' => $url, 'caption' => '<script></script>', 'width' => 50 ) ),
	widget( array( 'url' => $url, 'caption' => 'No width' ) ),
	widget( array( 'url' => $url, 'width' => '120', 'height' => '80px', 'link_type' => 'none', 'image_classes' => 'a b' ) ),
	widget( array( 'url' => $url, 'link_url' => 'http://x.test/', 'width' => 10, 'height' => 10 ) ),
	widget( array( 'attachment_id' => $photo, 'link_url' => 'http://x.test/b' ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'custom', 'width' => '0', 'height' => 300 ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'custom', 'width' => '0', 'height' => '0' ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'custom', 'width' => "\xff", 'height' => 300 ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'custom', 'width' => 300, 'height' => 201 ) ),
	widget( array( 'attachment_id' => $photo, 'size' => 'custom', 'width' => 100, 'height' => 200 ) ),
	widget( array( 'attachment_id' => $withfull, 'size' => 'custom', 'width' => 100, 'height' => 0 ) ),
	widget( array( 'attachment_id' => $photo, 'link_url' => array() ) ),
	widget( array( 'url' => $url, 'width' => 1, 'height' => 1, 'caption' => 'One' ) ),
	widget( array( 'attachment_id' => $plain, 'size' => 'custom', 'width' => 100, 'height' => 100 ) ),
	widget( array( 'attachment_id' => $nofile, 'size' => 'custom', 'width' => 2000, 'height' => 2000 ) ),
);

$attachments = array();
foreach ( array( $photo, $nofile, $withfull, $plain, $pdf ) as $id ) {
	$a             = get_post( $id );
	$attachments[] = array( $id, $a->post_mime_type, $a->post_excerpt, $a->guid, get_post_meta( $id ) );
}
$state = array(
	'attachments' => $attachments,
	'sizes'       => get_intermediate_image_sizes(),
	'html5'       => current_theme_supports( 'html5', 'caption' ),
);

// Not $request: WordPress sets that global.
foreach ( array( '', 'p=1' ) as $query ) {
	$GLOBALS['wp'] = new WP();
	wp( $query );
	if ( is_singular() ) {
		// As get_the_block_template_html() renders a singular template.
		$GLOBALS['wp_query']->the_post();
	}
	foreach ( $cases as $markup ) {
		wp_increase_content_media_count( -wp_increase_content_media_count( 0 ) );
		wp_high_priority_element_flag( true );
		$level = ob_get_level();
		try {
			$html = do_blocks( $markup );
		} catch ( Throwable $e ) {
			// The legacy widget block's output buffer is left open.
			while ( ob_get_level() > $level ) {
				ob_end_clean();
			}
			$html = get_class( $e );
		}
		echo bin2hex( $markup ), "\t", bin2hex( $query ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( $html ), "\n";
	}
}
$wpdb->query( 'ROLLBACK' );
