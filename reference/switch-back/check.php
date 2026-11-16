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
$fresh  = $wp_rewrite->rewrite_rules();
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

foreach ( $failures as $failure ) {
	echo "FAIL $failure\n";
}
echo count( $failures ), " failures\n";
exit( $failures ? 1 : 0 );
