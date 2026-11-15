<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints the block widget in a legacy widget block (signed under the fixed
// salt `test-salt`), over an attachment inserted for it and rolled back after.
// Lines are either
//   class<TAB>hex(content)<TAB>hex(class name)
// for WP_Widget_Block::get_dynamic_classname(), or
//   hex(markup)<TAB>hex(mode)<TAB>hex(state JSON)<TAB>hex(html)
// where the mode is `` (do_blocks() outside the loop), `loop` (do_blocks() in
// a singular post's loop) or `content` (the_content in that loop), and the
// state holds the attachment (ID, MIME type, excerpt, GUID and its meta).
// Each case starts with the content media count and the high priority flag
// reset.

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

$widget = $GLOBALS['wp_widget_factory']->get_widget_object( 'block' );
$method = new ReflectionMethod( $widget, 'get_dynamic_classname' );
$names  = array( 'paragraph', 'calendar', 'search', 'html', 'archives', 'latest-posts', 'latest-comments', 'tag-cloud', 'categories', 'audio', 'video', 'image', 'gallery', 'rss', 'heading' );
$firsts = array( '', 'text', "\n<!-- wp:paragraph /-->", '<!-- wp:my/block /-->' );
foreach ( $names as $name ) {
	$firsts[] = "<!-- wp:$name /--><!-- wp:paragraph /-->";
}
foreach ( $firsts as $content ) {
	echo "class\t", bin2hex( $content ), "\t", bin2hex( $method->invoke( $widget, $content ) ), "\n";
}

function widget( $instance, $id_base = 'block' ) {
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
$paragraph = "<!-- wp:paragraph -->\n<p>It's <b>bold</b> :)</p>\n<!-- /wp:paragraph -->";
$image     = "<!-- wp:image {\"id\":$photo,\"sizeSlug\":\"large\"} -->\n<figure class=\"wp-block-image size-large\"><img src=\"http://127.0.0.1:8081/wp-content/uploads/2026/01/photo-1024x683.jpg\" alt=\"\" class=\"wp-image-$photo\"/></figure>\n<!-- /wp:image -->";
$iframe    = "<!-- wp:html -->\n<iframe src=\"https://example.org/x\" width=\"10\" height=\"10\"></iframe>\n<!-- /wp:html -->";
$cases     = array(
	widget( array() ),
	widget( array( 'content' => '' ) ),
	widget( array( 'content' => $paragraph ) ),
	widget( array( 'content' => "\n" . $paragraph ) ),
	widget( array( 'content' => "<!-- wp:heading -->\n<h2 class=\"wp-block-heading\">Head</h2>\n<!-- /wp:heading -->" ) ),
	widget( array( 'content' => $image ) ),
	widget( array( 'content' => $image . $iframe ) ),
	widget( array( 'content' => $iframe . $image ) ),
	widget( array( 'content' => '<!-- wp:search {"label":"Search","buttonText":"Go"} /-->' ) ),
	widget( array( 'content' => 'Plain <img src="http://127.0.0.1:8081/wp-content/uploads/2026/01/photo-300x200.jpg" class="wp-image-' . $photo . '">' ) ),
	widget( array( 'content' => widget( array( 'content' => $image ) ) ) ),
	widget( array( 'content' => $paragraph, 'title' => 'Ignored' ) ),
	// The image widget inside a block widget: its image's context is not the
	// filter's, which adds the loading attributes after.
	widget( array( 'content' => widget( array( 'attachment_id' => $photo, 'size' => 'medium' ), 'media_image' ) ) ),
	// Stored inactive widgets: the instance as saved, an object or a query
	// string, which wp_parse_args() reads.
	'<!-- wp:legacy-widget {"id":"block-2"} /-->',
	'<!-- wp:legacy-widget {"id":"block-3"} /-->',
);

$stored          = new stdClass();
$stored->content = '<!-- wp:paragraph --><p>Stored</p><!-- /wp:paragraph -->';
update_option( 'widget_block', array( 2 => $stored, 3 => 'content=<b>q</b>' ) );
update_option( 'sidebars_widgets', array( 'wp_inactive_widgets' => array( 'block-2', 'block-3' ), 'array_version' => 3 ) );
// Instances register at widgets_init, before these were stored, and the
// sidebars are cached by then.
$GLOBALS['wp_widget_factory']->get_widget_object( 'block' )->_register();
$GLOBALS['_wp_sidebars_widgets'] = array();
$raw   = static fn( $name ) => $wpdb->get_var( $wpdb->prepare( "SELECT option_value FROM $wpdb->options WHERE option_name = %s", $name ) );
$state = array(
	'attachments' => array( array( $photo, 'image/jpeg', '', get_post( $photo )->guid, get_post_meta( $photo ) ) ),
	'options'     => array(
		'widget_block'     => $raw( 'widget_block' ),
		'sidebars_widgets' => $raw( 'sidebars_widgets' ),
	),
);

// Not $request: WordPress sets that global.
foreach ( array( '', 'loop', 'content' ) as $mode ) {
	$GLOBALS['wp'] = new WP();
	wp( '' === $mode ? '' : 'p=1' );
	if ( is_singular() ) {
		// As get_the_block_template_html() renders a singular template.
		$GLOBALS['wp_query']->the_post();
	}
	foreach ( $cases as $markup ) {
		wp_increase_content_media_count( -wp_increase_content_media_count( 0 ) );
		wp_high_priority_element_flag( true );
		$html = 'content' === $mode ? apply_filters( 'the_content', $markup ) : do_blocks( $markup );
		echo bin2hex( $markup ), "\t", bin2hex( $mode ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( $html ), "\n";
	}
}
$wpdb->query( 'ROLLBACK' );
