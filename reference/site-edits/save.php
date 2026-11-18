<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The site editor's saves, as its JavaScript sends them to the REST API,
// made by PHP WordPress as user 1 for reference/site-edits/save.sh:
// the theme's global styles post the editor's first view writes, then
// edited global styles, a navigation menu, the header part naming it, an
// edited footer part and home and single templates, a custom template
// given to the Hello world post, the archive template emptied, an empty
// custom template given to a new page, templates for the sample page and
// the privacy policy page.
// Run in the PHP container: php /dev/stdin < save.php. As on HyScribe,
// which defines it, DISALLOW_UNFILTERED_HTML has the saves filtered.

define( 'DISALLOW_UNFILTERED_HTML', true );
require '/var/www/html/wp-load.php';
wp_set_current_user( 1 );

$send = function ( $method, $route, $body = array(), $query = array() ) {
	$request = new WP_REST_Request( $method, $route );
	$request->set_query_params( $query );
	if ( $body ) {
		$request->set_header( 'Content-Type', 'application/json' );
		$request->set_body( wp_json_encode( $body ) );
	}
	$response = rest_do_request( $request );
	if ( $response->is_error() ) {
		fwrite( STDERR, "$method $route: " . wp_json_encode( $response->get_data() ) . "\n" );
		exit( 1 );
	}
	return $response->get_data();
};

// The content the editor opens a template or part with.
$raw = function ( $route ) use ( $send ) {
	return $send( 'GET', $route, array(), array( 'context' => 'edit' ) )['content']['raw'];
};

$paragraph = fn( $text ) => "<!-- wp:paragraph -->\n<p>$text</p>\n<!-- /wp:paragraph -->";

$styles = WP_Theme_JSON_Resolver::get_user_global_styles_post_id();
$send(
	'POST',
	"/wp/v2/global-styles/$styles",
	array(
		'id'       => $styles,
		'settings' => array(
			'color' => array(
				'palette' => array(
					'custom' => array(
						array(
							'color' => '#a1b2c3',
							'name'  => 'Brand',
							'slug'  => 'custom-brand',
						),
					),
				),
			),
		),
		'styles'   => array(
			'color'      => array(
				'background' => 'var:preset|color|accent-2',
				'text'       => 'var:preset|color|contrast',
			),
			'typography' => array( 'lineHeight' => '1.7' ),
			'elements'   => array(
				'link' => array( 'color' => array( 'text' => 'var:preset|color|custom-brand' ) ),
			),
			'blocks'     => array(
				'core/paragraph' => array( 'typography' => array( 'fontSize' => 'var:preset|font-size|large' ) ),
			),
			'css'        => 'body{letter-spacing:0.01em}',
		),
	)
);

$menu = $send(
	'POST',
	'/wp/v2/navigation',
	array(
		'title'   => 'Main menu',
		'status'  => 'publish',
		'content' => '<!-- wp:navigation-link {"label":"Sample Page","type":"page","id":2,"url":"http://127.0.0.1:8081/?page_id=2","kind":"post-type"} /-->'
			. "\n\n" . '<!-- wp:navigation-link {"label":"Elsewhere","url":"https://example.com/","kind":"custom"} /-->',
	)
)['id'];

$header = $raw( '/wp/v2/template-parts/twentytwentyfive//header' );
$send(
	'POST',
	'/wp/v2/template-parts/twentytwentyfive//header',
	array(
		'id'      => 'twentytwentyfive//header',
		'content' => str_replace( '<!-- wp:navigation {', '<!-- wp:navigation {"ref":' . $menu . ',', $header ),
	)
);

$send(
	'POST',
	'/wp/v2/template-parts/twentytwentyfive//footer',
	array(
		'id'      => 'twentytwentyfive//footer',
		'content' => $paragraph( 'Edited footer' ),
	)
);

foreach ( array( 'home', 'single' ) as $slug ) {
	$content = $raw( "/wp/v2/templates/twentytwentyfive//$slug" );
	$send(
		'POST',
		"/wp/v2/templates/twentytwentyfive//$slug",
		array(
			'id'      => "twentytwentyfive//$slug",
			'content' => preg_replace( '/<!-- wp:template-part \{"slug":"footer"/', $paragraph( "Edited $slug" ) . "\n\n$0", $content, 1 ),
		)
	);
}

$send(
	'POST',
	'/wp/v2/templates',
	array(
		'slug'    => 'wp-custom-template-landing',
		'title'   => 'Landing',
		'content' => '<!-- wp:template-part {"slug":"header","theme":"twentytwentyfive"} /-->' . "\n\n" . $paragraph( 'Landing' ) . "\n\n<!-- wp:post-title /-->\n\n<!-- wp:post-content /-->",
	)
);
$send( 'POST', '/wp/v2/posts/1', array( 'template' => 'wp-custom-template-landing' ) );

// Emptied in the editor: visitors see the theme's file.
$send(
	'POST',
	'/wp/v2/templates/twentytwentyfive//archive',
	array(
		'id'      => 'twentytwentyfive//archive',
		'content' => '',
	)
);
// An empty custom template, which has no file, for a new page.
$send(
	'POST',
	'/wp/v2/templates',
	array(
		'slug'    => 'wp-custom-template-blank',
		'title'   => 'Blank',
		'content' => '',
	)
);
$send(
	'POST',
	'/wp/v2/pages',
	array(
		'title'    => 'Blank page',
		'status'   => 'publish',
		'template' => 'wp-custom-template-blank',
	)
);

$send(
	'POST',
	'/wp/v2/templates',
	array(
		'slug'             => 'page-sample-page',
		'title'            => 'Page: Sample Page',
		'description'      => 'Template for Sample Page',
		'is_wp_suggestion' => true,
		'content'          => $paragraph( 'Only the sample page' ) . "\n\n<!-- wp:post-content /-->\n\n" . '<!-- wp:template-part {"slug":"footer","theme":"twentytwentyfive"} /-->',
	)
);

// The privacy policy page's template, which comes before a page's own.
$send(
	'POST',
	'/wp/v2/templates',
	array(
		'slug'    => 'privacy-policy',
		'title'   => 'Privacy Policy',
		'content' => $paragraph( 'Our policy' ) . "\n\n<!-- wp:post-content /-->",
	)
);
