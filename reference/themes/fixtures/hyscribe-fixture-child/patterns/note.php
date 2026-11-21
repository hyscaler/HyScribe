<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Title: Child note
 * Slug: hyscribe-fixture-child/note
 * Description: A note with an image from the child theme and two from its parent.
 * Categories: text
 * Keywords: note, child
 * Viewport Width: 1100
 *
 * @package HyScribe_Fixture_Child
 */
?>
<!-- wp:group {"className":"is-child-note","layout":{"type":"constrained"}} -->
<div class="wp-block-group is-child-note">
	<!-- wp:paragraph -->
	<p><?php echo esc_html__( 'This note is the child theme\'s own.', 'hyscribe-fixture-child' ); ?></p>
	<!-- /wp:paragraph -->
	<!-- wp:image {"sizeSlug":"full","linkDestination":"none"} -->
	<figure class="wp-block-image size-full"><img src="<?php echo esc_url( get_theme_file_uri( 'assets/images/child-mark.png' ) ); ?>" alt=""/></figure>
	<!-- /wp:image -->
	<!-- wp:image {"sizeSlug":"full","linkDestination":"none"} -->
	<figure class="wp-block-image size-full"><img src="<?php echo esc_url( get_theme_file_uri( 'assets/images/location.webp' ) ); ?>" alt=""/></figure>
	<!-- /wp:image -->
	<!-- wp:image {"sizeSlug":"full","linkDestination":"none"} -->
	<figure class="wp-block-image size-full"><img src="<?php echo esc_url( get_parent_theme_file_uri( 'assets/images/nurse.webp' ) ); ?>" alt=""/></figure>
	<!-- /wp:image -->
</div>
<!-- /wp:group -->
