<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-auth's capabilities: user_can() for users of every role
// over posts, comments, terms and users inserted into the fixture database
// inside a transaction that is rolled back. Lines are
//   option<TAB>name<TAB>hex(stored value)
//   user<TAB>ID<TAB>hex(stored wp_capabilities)
//   post<TAB>ID<TAB>post_author<TAB>post_status<TAB>post_type<TAB>post_parent
//   postmeta<TAB>ID<TAB>key<TAB>hex(value)
//   comment<TAB>ID<TAB>comment_post_ID
//   term<TAB>term_id<TAB>taxonomy
//   can<TAB>user ID<TAB>capability<TAB>hex(serialized args)<TAB>0 or 1
define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';
global $wpdb;
$wpdb->query( 'START TRANSACTION' );

$users = array( 1 );
foreach ( array( 'editor', 'author', 'contributor', 'subscriber' ) as $role ) {
	$users[ $role ] = wp_insert_user( array( 'user_login' => "cap_$role", 'user_pass' => 'x', 'role' => $role ) );
}
$users['custom']  = wp_insert_user( array( 'user_login' => 'cap_custom', 'user_pass' => 'x', 'role' => 'author' ) );
$custom           = new WP_User( $users['custom'] );
$custom->add_cap( 'custom_cap' );
$custom->add_cap( 'edit_published_posts', false );
$custom->add_cap( 'edit_others_posts', '1' );
foreach ( array( 'remove_users', 'delete_site', 'manage_blocks' ) as $granted ) {
	$custom->add_cap( $granted );
}
$users['none']    = wp_insert_user( array( 'user_login' => 'cap_none', 'user_pass' => 'x', 'role' => '' ) );
$users['unknown'] = wp_insert_user( array( 'user_login' => 'cap_unknown', 'user_pass' => 'x', 'role' => 'subscriber' ) );
update_user_meta( $users['unknown'], $wpdb->prefix . 'capabilities', array( 'ghost' => true, 'read' => '0' ) );
$users['garbled'] = wp_insert_user( array( 'user_login' => 'cap_garbled', 'user_pass' => 'x', 'role' => 'editor' ) );
update_user_meta( $users['garbled'], $wpdb->prefix . 'capabilities', 'not an array' );
$users['broken'] = wp_insert_user( array( 'user_login' => 'cap_broken', 'user_pass' => 'x', 'role' => 'editor' ) );
$wpdb->update( $wpdb->usermeta, array( 'meta_value' => 'a:1:{s:6:"editor"' ), array( 'user_id' => $users['broken'], 'meta_key' => $wpdb->prefix . 'capabilities' ) );
wp_cache_delete( $users['broken'], 'user_meta' );
$roles              = get_option( $wpdb->prefix . 'user_roles' );
$roles['nameless']  = array( 'capabilities' => array( 'edit_posts' => true, 'read' => true ) );
update_option( $wpdb->prefix . 'user_roles', $roles );
$users['nameless'] = wp_insert_user( array( 'user_login' => 'cap_nameless', 'user_pass' => 'x', 'role' => '' ) );
update_user_meta( $users['nameless'], $wpdb->prefix . 'capabilities', array( 'nameless' => true ) );
$users['missing'] = 999999;

$insert = static fn( $fields ) => wp_insert_post( $fields + array( 'post_title' => 'Cap', 'post_status' => 'publish', 'post_type' => 'post' ), false, false );
$author = $users['author'];
$editor = $users['editor'];
$posts  = array(
	$insert( array( 'post_author' => $author ) ),
	$insert( array( 'post_author' => $author, 'post_status' => 'draft' ) ),
	$insert( array( 'post_author' => $author, 'post_status' => 'pending' ) ),
	$insert( array( 'post_author' => $author, 'post_status' => 'private' ) ),
	$insert( array( 'post_author' => $author, 'post_status' => 'future', 'post_date' => '2099-01-01 00:00:00' ) ),
	$insert( array( 'post_author' => $editor ) ),
	$insert( array( 'post_author' => $editor, 'post_status' => 'private' ) ),
	$insert( array( 'post_author' => $editor, 'post_status' => 'draft' ) ),
	$insert( array( 'post_author' => 0, 'post_status' => 'draft' ) ),
	$insert( array( 'post_author' => $author, 'post_type' => 'page' ) ),
	$insert( array( 'post_author' => $editor, 'post_type' => 'page', 'post_status' => 'draft' ) ),
	$insert( array( 'post_author' => $author, 'post_type' => 'wp_block' ) ),
	$insert( array( 'post_author' => $editor, 'post_type' => 'wp_block', 'post_status' => 'draft' ) ),
	$insert( array( 'post_author' => $editor, 'post_type' => 'custom_css' ) ),
	$insert( array( 'post_author' => $editor, 'post_type' => 'wp_template' ) ),
	$insert( array( 'post_author' => $author, 'post_type' => 'nav_menu_item' ) ),
	$insert( array( 'post_author' => $author, 'post_type' => 'unregistered_type' ) ),
	$insert( array( 'post_author' => $author, 'post_status' => 'invented' ) ),
);
$trashed = $insert( array( 'post_author' => $author ) );
wp_trash_post( $trashed );
$trashed_draft = $insert( array( 'post_author' => $author, 'post_status' => 'draft' ) );
wp_trash_post( $trashed_draft );
$posts[] = $trashed;
$posts[] = $trashed_draft;
$own       = $users['custom'];
$posts[]   = $insert( array( 'post_author' => $own ) );
$own_trash = array( $insert( array( 'post_author' => $own ) ), $insert( array( 'post_author' => $own, 'post_status' => 'draft' ) ) );
foreach ( $own_trash as $id ) {
	wp_trash_post( $id );
	$posts[] = $id;
}
$posts[] = $insert( array( 'post_author' => $author, 'post_type' => 'revision', 'post_status' => 'inherit', 'post_parent' => $posts[0] ) );
$posts[] = $insert( array( 'post_author' => $author, 'post_type' => 'revision', 'post_status' => 'inherit', 'post_parent' => 0 ) );
$posts[] = $insert( array( 'post_author' => $author, 'post_type' => 'attachment', 'post_status' => 'inherit', 'post_parent' => $posts[1] ) );
$posts[] = $insert( array( 'post_author' => $author, 'post_type' => 'attachment', 'post_status' => 'inherit', 'post_parent' => 0 ) );
$posts[] = $insert( array( 'post_author' => $editor, 'post_type' => 'attachment', 'post_status' => 'inherit', 'post_parent' => $trashed ) );
$posts[] = $insert( array( 'post_author' => $editor, 'post_type' => 'attachment', 'post_status' => 'inherit', 'post_parent' => $trashed_draft ) );
$posts[] = $insert( array( 'post_author' => $editor, 'post_type' => 'attachment', 'post_status' => 'private' ) );
$posts[] = $insert( array( 'post_author' => $editor, 'post_type' => 'attachment', 'post_status' => 'draft' ) );
$attachments = array( 'private', 'draft', 'trash', 'auto-draft' );
foreach ( $attachments as $status ) {
	$id = $insert( array( 'post_author' => $editor, 'post_type' => 'attachment', 'post_status' => 'inherit' ) );
	$wpdb->update( $wpdb->posts, array( 'post_status' => $status ), array( 'ID' => $id ) );
	clean_post_cache( $id );
	$posts[] = $id;
}
$forgotten = $insert( array( 'post_author' => $author ) );
wp_trash_post( $forgotten );
delete_post_meta( $forgotten, '_wp_trash_meta_status' );
$posts[] = $forgotten;
$posts[] = $insert( array( 'post_author' => $editor, 'post_type' => 'attachment', 'post_status' => 'inherit', 'post_parent' => $forgotten ) );
$posts[] = 1;
$posts[] = 2;
$posts[] = 999999;
$front   = $posts[9];
$privacy = $posts[10];
update_option( 'page_on_front', $front );
update_option( 'wp_page_for_privacy_policy', $privacy );
update_post_meta( $posts[0], 'plain', 'x' );

// Comment and term IDs past every user's, so neither reads as a user.
for ( $n = 0; $n < 20; $n++ ) {
	wp_insert_comment( array( 'comment_post_ID' => 1, 'comment_content' => "filler $n" ) );
	wp_insert_term( "Filler $n", 'post_tag' );
}
$comments = array(
	wp_insert_comment( array( 'comment_post_ID' => $posts[0], 'comment_content' => 'c', 'user_id' => $author ) ),
	wp_insert_comment( array( 'comment_post_ID' => 0, 'comment_content' => 'c' ) ),
	1,
	999999,
	-1,
);
$category = wp_insert_term( 'Cap category', 'category' )['term_id'];
$tag      = wp_insert_term( 'Cap tag', 'post_tag' )['term_id'];
$menu     = wp_insert_term( 'Cap menu', 'nav_menu' )['term_id'];
$wpdb->insert( $wpdb->terms, array( 'name' => 'Ghost', 'slug' => 'ghost' ) );
$ghost = $wpdb->insert_id;
$wpdb->insert( $wpdb->term_taxonomy, array( 'term_id' => $ghost, 'taxonomy' => 'ghost_tax', 'description' => '' ) );
$terms    = array( 1, $category, $tag, $menu, $ghost, 999999 );

foreach ( array( $wpdb->prefix . 'user_roles', 'page_on_front', 'page_for_posts', 'wp_page_for_privacy_policy', 'default_category', 'default_term_category', 'default_post_tag', 'link_manager_enabled' ) as $name ) {
	echo 'option', "\t", $name, "\t", bin2hex( maybe_serialize( get_option( $name, '' ) ) ), "\n";
}
foreach ( $users as $id ) {
	echo 'user', "\t", $id, "\t", bin2hex( maybe_serialize( get_user_meta( $id, $wpdb->prefix . 'capabilities', true ) ) ), "\n";
}
foreach ( $posts as $id ) {
	$p = get_post( $id );
	if ( $p ) {
		echo 'post', "\t", $p->ID, "\t", $p->post_author, "\t", $p->post_status, "\t", $p->post_type, "\t", $p->post_parent, "\n";
		echo 'postmeta', "\t", $p->ID, "\t_wp_trash_meta_status\t", bin2hex( get_post_meta( $p->ID, '_wp_trash_meta_status', true ) ), "\n";
	}
}
foreach ( $comments as $id ) {
	$c = get_comment( $id );
	if ( $c ) {
		echo 'comment', "\t", $c->comment_ID, "\t", $c->comment_post_ID, "\n";
	}
}
foreach ( $terms as $id ) {
	$taxonomy = $wpdb->get_var( $wpdb->prepare( "SELECT taxonomy FROM $wpdb->term_taxonomy WHERE term_id = %d", $id ) );
	if ( $taxonomy ) {
		echo 'term', "\t", $id, "\t", $taxonomy, "\n";
	}
}

$checks = array();
foreach ( array( 'read', 'edit_posts', 'edit_others_posts', 'edit_published_posts', 'publish_posts', 'edit_pages', 'manage_options', 'manage_categories', 'moderate_comments', 'upload_files', 'edit_theme_options', 'list_users', 'switch_themes', 'activate_plugins', 'deactivate_plugin', 'unfiltered_html', 'edit_css', 'unfiltered_upload', 'export', 'import', 'create_users', 'delete_users', 'delete_user', 'promote_users', 'promote_user', 'add_users', 'remove_users', 'edit_users', 'edit_dashboard', 'customize', 'delete_site', 'exist', 'do_not_allow', 'install_languages', 'update_languages', 'view_site_health_checks', 'resume_plugins', 'resume_plugin', 'resume_theme', 'resume_themes', 'manage_links', 'update_core', 'update_php', 'update_https', 'edit_files', 'edit_plugins', 'install_plugins', 'upload_themes', 'edit_blocks', 'publish_blocks', 'read_private_blocks', 'edit_others_blocks', 'delete_published_blocks', 'setup_network', 'manage_privacy_options', 'export_others_personal_data', 'manage_network', 'manage_post_tags', 'edit_categories', 'assign_categories', 'assign_post_tags', 'custom_cap', 'nonexistent_cap', 'edit_post', 'edit_comment', 'edit_term', 'publish_post', 'edit_user_meta', 'create_app_password', 'manage_blocks', 'read_blocks', '0', '1', '7', '10', 'edit_block_binding' ) as $cap ) {
	$checks[] = array( $cap, array() );
}
foreach ( $posts as $id ) {
	foreach ( array( 'edit_post', 'delete_post', 'read_post', 'publish_post', 'edit_page', 'delete_page', 'read_page', 'edit_block', 'read_block', 'delete_block' ) as $cap ) {
		$checks[] = array( $cap, array( $id ) );
	}
	foreach ( array( 'plain', '_private', '', '0', "\x01_hidden", "\xe9_x", "\xaa_x", "\xb5_x", "\xba_x", "\xd7_x", "\xf7_x", "\xff_x", "\xc0_x", "\xf8_x" ) as $key ) {
		$checks[] = array( 'edit_post_meta', array( $id, $key ) );
	}
	$checks[] = array( 'delete_post_meta', array( $id ) );
	$checks[] = array( 'add_post_meta', array( $id, 'plain' ) );
}
$checks[] = array( 'edit_post', array( (string) $posts[0] ) );
$checks[] = array( 'edit_post', array( "{$posts[0]}x" ) );
$checks[] = array( 'edit_block_binding', array( new WP_Block_Editor_Context( array( 'name' => 'core/edit-site' ) ) ) );
$checks[] = array( 'edit_block_binding', array( new WP_Block_Editor_Context( array( 'name' => 'core/edit-post' ) ) ) );
$checks[] = array( 'edit_block_binding', array( new WP_Block_Editor_Context( array( 'post' => new WP_Post( (object) array( 'ID' => 999999 ) ) ) ) ) );
foreach ( $posts as $id ) {
	$p = get_post( $id );
	if ( $p && get_post_type_object( $p->post_type ) ) {
		$checks[] = array( 'edit_block_binding', array( new WP_Block_Editor_Context( array( 'post' => $p ) ) ) );
	}
}
$checks[] = array( 'edit_post', array( 0 ) );
foreach ( $comments as $id ) {
	$checks[] = array( 'edit_comment', array( $id ) );
	$checks[] = array( 'edit_comment_meta', array( $id, 'plain' ) );
	$checks[] = array( 'delete_comment_meta', array( $id, '_x' ) );
	$checks[] = array( 'edit_comment_meta', array( $id, '_wp_note_status' ) );
	$checks[] = array( 'edit_comment', array( "{$id}x" ) );
	$checks[] = array( 'edit_comment', array( (string) $id ) );
}
foreach ( $terms as $id ) {
	foreach ( array( 'edit_term', 'delete_term', 'assign_term' ) as $cap ) {
		$checks[] = array( $cap, array( $id ) );
	}
	$checks[] = array( 'edit_term_meta', array( $id, 'plain' ) );
	$checks[] = array( 'add_term_meta', array( $id ) );
}
foreach ( $users as $id ) {
	foreach ( array( 'edit_user', 'delete_user', 'remove_user', 'promote_user', 'create_app_password', 'list_app_passwords', 'delete_app_password' ) as $cap ) {
		$checks[] = array( $cap, array( $id ) );
	}
	$checks[] = array( 'edit_user_meta', array( $id, 'nickname' ) );
	$checks[] = array( 'delete_user_meta', array( $id, '_secret' ) );
}
foreach ( array( 0, '1', 1 ) as $value ) {
	update_option( 'link_manager_enabled', $value );
	echo 'option', "\tlink_manager_enabled\t", bin2hex( maybe_serialize( $value ) ), "\n";
	foreach ( $users as $id ) {
		echo 'can', "\t", $id, "\tmanage_links\t", bin2hex( serialize( array() ) ), "\t", user_can( $id, 'manage_links' ) ? 1 : 0, "\n";
	}
}
update_option( 'link_manager_enabled', 0 );
echo 'option', "\tlink_manager_enabled\t", bin2hex( maybe_serialize( 0 ) ), "\n";
// A registered meta's auth callback asks about the current user.
foreach ( $users as $id ) {
	wp_set_current_user( $id );
	foreach ( $checks as list( $cap, $args ) ) {
		echo 'can', "\t", $id, "\t", $cap, "\t", bin2hex( serialize( $args ) ), "\t", user_can( $id, $cap, ...$args ) ? 1 : 0, "\n";
	}
}
$wpdb->query( 'ROLLBACK' );
