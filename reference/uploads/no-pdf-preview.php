<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for the upload goldens: while a PDF's metadata is made,
// no image editor is available, as on a host without Imagick and
// Ghostscript, so PHP writes no `-pdf.jpg` preview (ADR 0020). Images keep
// their editors.

add_filter(
	'fallback_intermediate_image_sizes',
	static function ( $sizes ) {
		add_filter( 'wp_image_editors', '__return_empty_array' );
		return $sizes;
	}
);
add_filter(
	'wp_generate_attachment_metadata',
	static function ( $metadata ) {
		remove_filter( 'wp_image_editors', '__return_empty_array' );
		return $metadata;
	}
);
