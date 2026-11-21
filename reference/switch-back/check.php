<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The switch-back suite's checks of the data HyScribe wrote, run by
// reference/switch-back/run.sh in the PHP container with WordPress's own
// functions: every option, meta value and user loads and unserializes,
// the counts WordPress recomputes match the stored ones, and the rewrite
// rules work. Prints one line per failure and exits 1 on any, or on any
// PHP error, warning, notice or deprecation.

$failures = array();
set_error_handler(
	function ( $level, $message, $file, $line ) use ( &$failures ) {
		$failures[] = "PHP error $level: $message ($file:$line)";
		return true;
	}
);
require '/var/www/html/wp-load.php';
global $wpdb, $wp_rewrite;

// Every option, as wp_load_alloptions() and get_option() read them.
wp_load_alloptions();
foreach ( $wpdb->get_results( "SELECT option_name, option_value FROM $wpdb->options" ) as $row ) {
	if ( is_serialized( $row->option_value ) && false === unserialize( $row->option_value ) && 'b:0;' !== $row->option_value ) {
		$failures[] = "option $row->option_name does not unserialize";
	}
	get_option( $row->option_name );
}

// Every meta value of every object type.
foreach ( array( 'postmeta' => 'post_id', 'commentmeta' => 'comment_id', 'termmeta' => 'term_id', 'usermeta' => 'user_id' ) as $table => $column ) {
	$rows = $wpdb->get_results( "SELECT meta_key, meta_value, $column AS id FROM {$wpdb->$table}" );
	foreach ( $rows as $row ) {
		if ( is_serialized( $row->meta_value ) && false === unserialize( $row->meta_value ) && 'b:0;' !== $row->meta_value ) {
			$failures[] = "$table $row->meta_key of $row->id does not unserialize";
		}
	}
}

// Every user, with a role WordPress knows.
foreach ( $wpdb->get_col( "SELECT ID FROM $wpdb->users" ) as $id ) {
	$user = get_userdata( $id );
	if ( ! $user || ! $user->exists() ) {
		$failures[] = "user $id does not load";
		continue;
	}
	foreach ( $user->roles as $role ) {
		if ( ! get_role( $role ) ) {
			$failures[] = "user $id has the unknown role $role";
		}
	}
	if ( ! $user->roles ) {
		$failures[] = "user $id has no role";
	}
}
count_users();

// Posts and their comment counts, recomputed inside a transaction rolled back.
foreach ( get_post_types() as $type ) {
	wp_count_posts( $type );
}
wp_count_comments();
$stored_comments = $wpdb->get_results( "SELECT ID, comment_count FROM $wpdb->posts", OBJECT_K );
$stored_terms    = $wpdb->get_results( "SELECT term_taxonomy_id, taxonomy, count FROM $wpdb->term_taxonomy", OBJECT_K );
$wpdb->query( 'START TRANSACTION' );
foreach ( array_keys( $stored_comments ) as $id ) {
	wp_update_comment_count_now( $id );
}
$by_taxonomy = array();
foreach ( $stored_terms as $tt_id => $row ) {
	$by_taxonomy[ $row->taxonomy ][] = (int) $tt_id;
}
foreach ( $by_taxonomy as $taxonomy => $tt_ids ) {
	if ( taxonomy_exists( $taxonomy ) ) {
		wp_update_term_count_now( $tt_ids, $taxonomy );
	}
}
$counted_comments = $wpdb->get_results( "SELECT ID, comment_count FROM $wpdb->posts", OBJECT_K );
$counted_terms    = $wpdb->get_results( "SELECT term_taxonomy_id, taxonomy, count FROM $wpdb->term_taxonomy", OBJECT_K );
$wpdb->query( 'ROLLBACK' );
foreach ( $stored_comments as $id => $row ) {
	if ( $row->comment_count !== $counted_comments[ $id ]->comment_count ) {
		$failures[] = "post $id stores comment_count $row->comment_count, PHP counts {$counted_comments[ $id ]->comment_count}";
	}
}
foreach ( $stored_terms as $tt_id => $row ) {
	if ( $row->count !== $counted_terms[ $tt_id ]->count ) {
		$failures[] = "term taxonomy $tt_id ($row->taxonomy) stores count $row->count, PHP counts {$counted_terms[ $tt_id ]->count}";
	}
}

// What the options point at exists.
foreach ( get_option( 'sticky_posts' ) as $id ) {
	if ( ! get_post( $id ) ) {
		$failures[] = "sticky post $id does not exist";
	}
}
foreach ( array( 'wp_page_for_privacy_policy', 'page_on_front', 'page_for_posts' ) as $name ) {
	$id = (int) get_option( $name );
	if ( $id && ! get_post( $id ) ) {
		$failures[] = "$name names the missing post $id";
	}
}
if ( ! term_exists( (int) get_option( 'default_category' ), 'category' ) ) {
	$failures[] = 'default_category names no category';
}

// The rewrite rules as HyScribe left them (run.sh saves them before PHP's
// first request regenerates empty ones) are what PHP generates, and load.
$stored = maybe_unserialize( file_get_contents( '/tmp/hyscribe-rewrite-rules' ) );
// As refresh_rewrite_rules() generates them for the option: `$matches[1]`, not `$1`.
$wp_rewrite->matches = 'matches';
$fresh               = $wp_rewrite->rewrite_rules();
if ( '' !== $stored && ! is_array( $stored ) ) {
	$failures[] = 'rewrite_rules is neither empty nor an array';
} elseif ( is_array( $stored ) && $stored !== $fresh ) {
	$missing = array_diff_assoc( $fresh, $stored );
	$extra   = array_diff_assoc( $stored, $fresh );
	$failures[] = sprintf(
		'rewrite_rules differ from the rules PHP generates: %d missing (%s), %d not PHP\'s (%s)%s',
		count( $missing ),
		implode( ' ', array_slice( array_keys( $missing ), 0, 3 ) ),
		count( $extra ),
		implode( ' ', array_slice( array_keys( $extra ), 0, 3 ) ),
		$missing || $extra ? '' : ', in another order'
	);
}
if ( get_option( 'permalink_structure' ) && ! $wp_rewrite->wp_rewrite_rules() ) {
	$failures[] = 'no rewrite rules under a permalink structure';
}

// Nothing a deletion left behind: rows of an object that is gone.
$orphans = array(
	'postmeta'           => "SELECT COUNT(*) FROM $wpdb->postmeta m LEFT JOIN $wpdb->posts p ON p.ID = m.post_id WHERE p.ID IS NULL",
	'commentmeta'        => "SELECT COUNT(*) FROM $wpdb->commentmeta m LEFT JOIN $wpdb->comments c ON c.comment_ID = m.comment_id WHERE c.comment_ID IS NULL",
	'usermeta'           => "SELECT COUNT(*) FROM $wpdb->usermeta m LEFT JOIN $wpdb->users u ON u.ID = m.user_id WHERE u.ID IS NULL",
	'termmeta'           => "SELECT COUNT(*) FROM $wpdb->termmeta m LEFT JOIN $wpdb->terms t ON t.term_id = m.term_id WHERE t.term_id IS NULL",
	'term_taxonomy'      => "SELECT COUNT(*) FROM $wpdb->term_taxonomy x LEFT JOIN $wpdb->terms t ON t.term_id = x.term_id WHERE t.term_id IS NULL",
	'term_relationships' => "SELECT COUNT(*) FROM $wpdb->term_relationships r JOIN $wpdb->term_taxonomy x USING (term_taxonomy_id) LEFT JOIN $wpdb->posts p ON p.ID = r.object_id WHERE x.taxonomy <> 'link_category' AND p.ID IS NULL",
	'comments'           => "SELECT COUNT(*) FROM $wpdb->comments c LEFT JOIN $wpdb->posts p ON p.ID = c.comment_post_ID WHERE p.ID IS NULL",
	'posts (parent)'     => "SELECT COUNT(*) FROM $wpdb->posts c LEFT JOIN $wpdb->posts p ON p.ID = c.post_parent WHERE c.post_parent <> 0 AND c.post_type <> 'attachment' AND p.ID IS NULL",
	'posts (author)'     => "SELECT COUNT(*) FROM $wpdb->posts p LEFT JOIN $wpdb->users u ON u.ID = p.post_author WHERE p.post_author <> 0 AND u.ID IS NULL",
);
foreach ( $orphans as $table => $sql ) {
	$count = (int) $wpdb->get_var( $sql );
	if ( $count ) {
		$failures[] = "$table has $count rows of objects that are gone";
	}
}

// What was deleted for good: from the Trash, with the Trash off, emptied.
foreach ( array( 'Scratch A', 'Scratch C', 'Scratch D', 'Scratch E', 'Deleted over REST', 'REST draft menu' ) as $title ) {
	if ( $wpdb->get_var( $wpdb->prepare( "SELECT ID FROM $wpdb->posts WHERE post_title = %s", $title ) ) ) {
		$failures[] = "the deleted post $title is still there";
	}
}
// The default install's comment and term IDs; the Theme Test Data's differ.
$default_install = 'ttd' !== getenv( 'HYSCRIBE_SB_FIXTURE' );
foreach ( $default_install ? array( 3, 7, 8, 9, 10, 11 ) : array() as $id ) {
	if ( get_comment( $id ) ) {
		$failures[] = "the deleted comment $id is still there";
	}
}
foreach ( $default_install ? array( 6, 7, 9 ) : array() as $id ) {
	if ( get_term( $id ) ) {
		$failures[] = "the deleted term $id is still there";
	}
}

// Cron: the scheduled post HyScribe published and the trash it emptied.
$cron_id = (int) $wpdb->get_var( "SELECT ID FROM $wpdb->posts WHERE post_title = 'A cron scheduled post'" );
if ( $cron_id && 'publish' !== get_post_status( $cron_id ) ) {
	$failures[] = 'the scheduled post is ' . get_post_status( $cron_id ) . ', not published by cron';
}
if ( $cron_id && false !== wp_next_scheduled( 'publish_future_post', array( $cron_id ) ) ) {
	$failures[] = 'the scheduled post still has its publish_future_post event';
}
if ( $wpdb->get_var( "SELECT ID FROM $wpdb->posts WHERE post_title = 'A cron trashed post'" ) ) {
	$failures[] = 'the post trashed 40 days ago is still there after cron';
}

// Attachments HyScribe made in the last day: the files, sizes and links PHP uses.
foreach ( $wpdb->get_col( "SELECT ID FROM $wpdb->posts WHERE post_type = 'attachment' AND post_date_gmt > UTC_TIMESTAMP() - INTERVAL 1 DAY" ) as $id ) {
	$file = get_attached_file( $id );
	if ( ! $file || ! file_exists( $file ) ) {
		$failures[] = "attachment $id has no file at " . var_export( $file, true );
		continue;
	}
	if ( ! wp_get_attachment_url( $id ) || ! get_attachment_link( $id ) ) {
		$failures[] = "attachment $id has no URL";
	}
	if ( ! wp_attachment_is_image( $id ) ) {
		continue;
	}
	$meta = wp_get_attachment_metadata( $id );
	if ( ! is_array( $meta ) || empty( $meta['width'] ) || empty( $meta['file'] ) ) {
		$failures[] = "attachment $id has no metadata PHP can use";
		continue;
	}
	$size = getimagesize( $file );
	if ( ! $size || $size[0] !== $meta['width'] || $size[1] !== $meta['height'] ) {
		$failures[] = "attachment $id: file is " . ( $size ? "{$size[0]}x{$size[1]}" : 'not an image' ) . ", metadata says {$meta['width']}x{$meta['height']}";
	}
	foreach ( $meta['sizes'] ?? array() as $name => $info ) {
		$path = path_join( dirname( $file ), $info['file'] );
		$real = file_exists( $path ) ? getimagesize( $path ) : false;
		if ( ! $real || $real[0] !== $info['width'] || $real[1] !== $info['height'] ) {
			$failures[] = "attachment $id size $name ({$info['file']}) is " . ( $real ? "{$real[0]}x{$real[1]}" : 'missing' ) . ", metadata says {$info['width']}x{$info['height']}";
		}
	}
	if ( ! wp_get_attachment_image( $id, 'thumbnail' ) ) {
		$failures[] = "attachment $id: wp_get_attachment_image() is empty";
	}
	if ( ! empty( $meta['original_image'] ) && ! file_exists( (string) wp_get_original_image_path( $id ) ) ) {
		$failures[] = "attachment $id: its original image is missing";
	}
}

// The site editor's saves: templates, parts and global styles as PHP finds them.
foreach ( array( 'wp_template', 'wp_template_part' ) as $type ) {
	foreach ( $wpdb->get_results( $wpdb->prepare( "SELECT ID, post_name FROM $wpdb->posts WHERE post_type = %s AND post_status = 'publish'", $type ) ) as $row ) {
		$themes = wp_get_object_terms( $row->ID, 'wp_theme', array( 'fields' => 'names' ) );
		if ( 1 !== count( $themes ) || ! wp_get_theme( $themes[0] )->exists() ) {
			$failures[] = "$type $row->post_name has no wp_theme term of an installed theme";
			continue;
		}
		$template = get_block_template( "{$themes[0]}//$row->post_name", $type );
		if ( ! $template || 'custom' !== $template->source || (int) $template->wp_id !== (int) $row->ID ) {
			$failures[] = "PHP does not find the custom $type $row->post_name";
		}
	}
}
foreach ( array( 'single' => 'Single template saved from HyScribe.', 'switch-back-template' => 'Custom template saved from HyScribe.' ) as $slug => $text ) {
	$template = get_block_template( "twentytwentyfive//$slug", 'wp_template' );
	if ( $template && ! str_contains( $template->content, $text ) ) {
		$failures[] = "the template $slug does not hold its saved content";
	}
}
if ( get_block_template( 'twentytwentyfive//doomed-template', 'wp_template' ) ) {
	$failures[] = 'the deleted template is still there';
}
$styles = $wpdb->get_row( "SELECT ID, post_content FROM $wpdb->posts WHERE post_type = 'wp_global_styles' AND post_status = 'publish'" );
if ( $styles ) {
	$data = json_decode( $styles->post_content, true );
	if ( ! is_array( $data ) || empty( $data['isGlobalStylesUserThemeJSON'] ) ) {
		$failures[] = 'the global styles post is not user theme.json data';
	}
	if ( (int) WP_Theme_JSON_Resolver::get_user_global_styles_post_id() !== (int) $styles->ID ) {
		$failures[] = 'PHP names another global styles post than the one HyScribe saved';
	}
	if ( isset( $data['styles']['color']['background'] ) && '#fdf2d0' === $data['styles']['color']['background']
		&& ! str_contains( wp_get_global_stylesheet(), '#fdf2d0' ) ) {
		$failures[] = "PHP's global stylesheet does not carry the saved background";
	}
}

// The plugins: the site's own, which HyScribe does not have, still active, and
// the one the extension HyScribe activated stands in for, as PHP loads them.
if ( $default_install ) {
	$active = array( 'hello.php', 'hyscribe-test/hyscribe-test.php' );
	if ( get_option( 'active_plugins' ) !== $active ) {
		$failures[] = 'active_plugins is not ' . implode( ', ', $active );
	}
	$loaded = array_map( 'plugin_basename', wp_get_active_and_valid_plugins() );
	if ( $loaded !== $active ) {
		$failures[] = 'PHP loads the plugins ' . implode( ', ', $loaded );
	}
}

// The cron events HyScribe scheduled, as wp-cron.php reads them.
foreach ( _get_cron_array() as $timestamp => $hooks ) {
	foreach ( $hooks as $hook => $events ) {
		foreach ( $events as $key => $event ) {
			if ( ! is_int( $timestamp ) || md5( serialize( $event['args'] ) ) !== $key
				|| ( $event['schedule'] && ! wp_get_schedule( $hook, $event['args'] ) ) ) {
				$failures[] = "cron event $hook at $timestamp is not as wp_schedule_event() stores it";
			}
		}
	}
}

// Each object HyScribe's REST API wrote, read with PHP's REST API by user 1
// in the edit context: the same as HyScribe's last answer for it, or gone
// where that answer was a deletion. Objects created for admin actions to
// change after (cases named scratch-) are not compared.
wp_set_current_user( 1 );
$answers = array();
foreach ( file( '/tmp/hyscribe-rest.tsv', FILE_IGNORE_NEW_LINES ) as $line ) {
	list( $case, $path, $body ) = explode( "\t", $line, 3 );
	if ( str_starts_with( $case, 'scratch-' ) ) {
		continue;
	}
	parse_str( (string) parse_url( $path, PHP_URL_QUERY ), $query );
	// A template's ID is a string with slashes, which its route carries whole.
	$base   = preg_replace( '#^(/wp/v2/templates|/wp/v2/template-parts)/.*$#', '$1', preg_replace( '#/\d+$#', '', $query['rest_route'] ) );
	$answer = json_decode( $body, true );
	if ( '/wp/v2/settings' === $base ) {
		$answers[ $base ] = $answer;
		continue;
	}
	if ( isset( $answer['deleted'] ) ) {
		$answers[ "$base/{$answer['previous']['id']}" ] = null;
		// The files of a media item go with it.
		$file = '/wp/v2/media' === $base ? ( $answer['previous']['media_details']['file'] ?? '' ) : '';
		if ( $file && file_exists( wp_get_upload_dir()['basedir'] . "/$file" ) ) {
			$failures[] = "the file $file of the deleted media item is still there";
		}
	} else {
		$answers[ "$base/{$answer['id']}" ] = $answer;
	}
}
// Where PHP's and HyScribe's answers differ, as paths of keys.
function switch_back_diff( $php, $hyscribe, $at = '' ) {
	if ( ! is_array( $php ) || ! is_array( $hyscribe ) ) {
		return $php === $hyscribe ? array() : array( "$at: " . wp_json_encode( $hyscribe ) . ' in HyScribe, ' . wp_json_encode( $php ) . ' in PHP' );
	}
	$diff = array();
	foreach ( array_unique( array_merge( array_keys( $php ), array_keys( $hyscribe ) ) ) as $key ) {
		$diff = array_merge( $diff, switch_back_diff( $php[ $key ] ?? null, $hyscribe[ $key ] ?? null, "$at/$key" ) );
	}
	if ( ! $diff && array_keys( $php ) !== array_keys( $hyscribe ) ) {
		$diff[] = "$at: the keys in another order";
	}
	return $diff;
}
foreach ( $answers as $route => $answer ) {
	$request = new WP_REST_Request( 'GET', $route );
	$request->set_param( 'context', str_starts_with( $route, '/wp/v2/template' ) ? 'view' : 'edit' );
	$response = rest_do_request( $request );
	if ( null === $answer ) {
		if ( 404 !== $response->get_status() ) {
			$failures[] = "REST $route was deleted by HyScribe, PHP answers {$response->get_status()}";
		}
		continue;
	}
	$read = json_decode( wp_json_encode( rest_get_server()->response_to_data( $response, false ) ), true );
	// A GET in the edit context passes a post's password check for good
	// (get_item_permissions_check()), a write's answer does not.
	$at = array_search( 'post-password-protected', $read['class_list'] ?? array(), true );
	if ( false !== $at && in_array( 'post-password-required', $answer['class_list'], true ) ) {
		$read['class_list'][ $at ] = 'post-password-required';
	}
	// PHP answers a template's create in the view context and its update in edit,
	// the one with block_version: read in view, and leave that field out of both.
	if ( str_starts_with( $route, '/wp/v2/template' ) ) {
		unset( $answer['content']['block_version'] );
	}
	foreach ( array_slice( switch_back_diff( $read, $answer ), 0, 5 ) as $diff ) {
		$failures[] = "REST $route differs at $diff";
	}
}

foreach ( $failures as $failure ) {
	echo "FAIL $failure\n";
}
echo count( $failures ), " failures\n";
exit( $failures ? 1 : 0 );
