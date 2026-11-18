<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's cron actions (MVP 8.1): each core job HyScribe runs, as
// its hook's callback with no user, as wp-cron.php runs it, and wp-cron.php's
// loop over the hooks HyScribe runs (ADR 0015). A post whose ID ends up in a
// cron key is inserted with an import_id; a time a job compares with is a
// fixed one long past, or `$cron_time` far ahead, so the goldens keep.

const HYSCRIBE_CRON_OLD    = 1000000000;
const HYSCRIBE_CRON_FUTURE = '2999-01-01 00:00:00';

// The hooks HyScribe runs; any other event stays for PHP WordPress.
$cron_hooks = array(
	'publish_future_post',
	'wp_scheduled_delete',
	'delete_expired_transients',
	'wp_scheduled_auto_draft_delete',
	'wp_privacy_delete_old_export_files',
	'wp_privacy_personal_data_cleanup_requests',
);

$cron_post = function ( $args = array() ) {
	return wp_insert_post(
		$args + array(
			'post_title'   => 'A write oracle post',
			'post_content' => 'Its content.',
			'post_status'  => 'publish',
			'post_author'  => 1,
		),
		true
	);
};

// A post scheduled for 2030 whose date is then moved to 2026-01-01, as a
// post missed while no cron ran, with its event moved to that time.
$cron_missed = function () use ( $cron_post ) {
	global $wpdb;
	$id = $cron_post(
		array(
			'import_id' => 900,
			'post_date' => '2030-01-01 10:00:00',
		)
	);
	$date = '2026-01-01 10:00:00';
	$wpdb->update(
		$wpdb->posts,
		array(
			'post_date'     => $date,
			'post_date_gmt' => $date,
		),
		array( 'ID' => $id )
	);
	clean_post_cache( $id );
	wp_clear_scheduled_hook( 'publish_future_post', array( $id ) );
	wp_schedule_single_event( strtotime( "$date GMT" ), 'publish_future_post', array( $id ) );
	return $id;
};

// A comment written raw with its trash meta, so the HyScribe side can write
// the same rows.
$cron_comment = function ( $comment_id, $approved, $trashed ) {
	global $wpdb;
	$wpdb->insert(
		$wpdb->comments,
		array(
			'comment_ID'       => $comment_id,
			'comment_post_ID'  => 1,
			'comment_author'   => 'A reader',
			'comment_date'     => '2026-01-01 00:00:00',
			'comment_date_gmt' => '2026-01-01 00:00:00',
			'comment_content'  => 'A comment.',
			'comment_approved' => $approved,
		)
	);
	foreach ( array(
		'_wp_trash_meta_status' => '1',
		'_wp_trash_meta_time'   => $trashed,
	) as $key => $value ) {
		$wpdb->insert(
			$wpdb->commentmeta,
			array(
				'comment_id' => $comment_id,
				'meta_key'   => $key,
				'meta_value' => $value,
			)
		);
	}
};

// Posts and comments trashed long ago and lately, and old trash meta left
// on items no longer in the trash or gone.
$cron_trash = function () use ( $cron_post, $cron_comment ) {
	$old = $cron_post();
	wp_trash_post( $old );
	update_post_meta( $old, '_wp_trash_meta_time', HYSCRIBE_CRON_OLD );
	wp_trash_post( $cron_post() );
	$restored = $cron_post();
	add_post_meta( $restored, '_wp_trash_meta_status', 'publish' );
	add_post_meta( $restored, '_wp_trash_meta_time', HYSCRIBE_CRON_OLD );
	add_post_meta( HYSCRIBE_MISSING_ID, '_wp_trash_meta_time', HYSCRIBE_CRON_OLD );
	$cron_comment( 901, 'trash', HYSCRIBE_CRON_OLD );
	$cron_comment( 902, '1', HYSCRIBE_CRON_OLD );
	$cron_comment( 903, 'trash', time() );
};

// Transients past their time, one still valid, and one with no timeout.
$cron_transients = function () {
	global $wpdb;
	set_transient( 'oracle_old', 'old', HOUR_IN_SECONDS );
	set_transient( 'oracle_fresh', 'fresh', HOUR_IN_SECONDS );
	set_site_transient( 'oracle_site_old', 'old', HOUR_IN_SECONDS );
	add_option( '_transient_oracle_lone', 'lone' );
	foreach ( array( '_transient_timeout_oracle_old', '_site_transient_timeout_oracle_site_old' ) as $name ) {
		$wpdb->update( $wpdb->options, array( 'option_value' => HYSCRIBE_CRON_OLD ), array( 'option_name' => $name ) );
	}
	wp_cache_flush();
};

// An auto-draft older than seven days and a new one.
$cron_auto_drafts = function () use ( $cron_post ) {
	global $wpdb;
	$old = $cron_post( array( 'post_status' => 'auto-draft' ) );
	$cron_post( array( 'post_status' => 'auto-draft' ) );
	$wpdb->update( $wpdb->posts, array( 'post_date' => '2020-01-01 00:00:00' ), array( 'ID' => $old ) );
	clean_post_cache( $old );
};

// Privacy requests written raw: pending ones last changed long ago and far
// ahead, and a confirmed one long ago.
$cron_requests = function () {
	global $wpdb;
	foreach ( array(
		array( 'export_personal_data', 'request-pending', '2020-01-01 00:00:00' ),
		array( 'remove_personal_data', 'request-pending', HYSCRIBE_CRON_FUTURE ),
		array( 'remove_personal_data-2', 'request-confirmed', '2020-01-01 00:00:00' ),
	) as list( $name, $status, $modified ) ) {
		$wpdb->insert(
			$wpdb->posts,
			array(
				'post_author'           => 0,
				'post_date'             => '2020-01-01 00:00:00',
				'post_date_gmt'         => '2020-01-01 00:00:00',
				'post_content'          => '[]',
				'post_title'            => 'reader@example.test',
				'post_excerpt'          => '',
				'post_status'           => $status,
				'post_password'         => 'a-request-key',
				'post_name'             => $name,
				'to_ping'               => '',
				'pinged'                => '',
				'post_modified'         => $modified,
				'post_modified_gmt'     => $modified,
				'post_content_filtered' => '',
				'post_type'             => 'user_request',
				'comment_status'        => 'closed',
				'ping_status'           => 'closed',
			)
		);
	}
};

// The cron array of a loop: each hook HyScribe runs, one it leaves to PHP
// (an update check) and a plugin's, all due now.
$cron_loop = function () use ( $cron_missed, $cron_trash, $cron_transients ) {
	$missed = $cron_missed();
	$cron_trash();
	$cron_transients();
	$now   = time();
	$crons = array();
	foreach ( array(
		'wp_scheduled_delete'                       => 'daily',
		'wp_version_check'                          => 'twicedaily',
		'delete_expired_transients'                 => 'daily',
		'oracle_plugin_hook'                        => 'hourly',
		'wp_privacy_delete_old_export_files'        => 'hourly',
		'wp_privacy_personal_data_cleanup_requests' => 'daily',
	) as $hook => $schedule ) {
		$crons[ $now ][ $hook ][ md5( serialize( array() ) ) ] = array(
			'schedule' => $schedule,
			'args'     => array(),
			'interval' => wp_get_schedules()[ $schedule ]['interval'],
		);
	}
	$crons[ $now ]['publish_future_post'][ md5( serialize( array( $missed ) ) ) ] = array(
		'schedule' => false,
		'args'     => array( $missed ),
	);
	_set_cron_array( $crons );
};

// The events a page view schedules that HyScribe's cron runs: init's two
// privacy events, then admin.php's, in that order.
$cron_view_events = array(
	'wp_privacy_delete_old_export_files',
	'wp_privacy_personal_data_cleanup_requests',
	'wp_scheduled_delete',
	'delete_expired_transients',
);

$cron_unscheduled = function ( $hooks ) {
	foreach ( $hooks as $hook ) {
		wp_clear_scheduled_hook( $hook );
	}
};

// wp-settings.php's init callbacks, then wp-admin/admin.php after auth_redirect().
$cron_view = function () {
	wp_schedule_delete_old_privacy_export_files();
	wp_schedule_personal_data_cleanup_requests();
	if ( ! wp_next_scheduled( 'wp_scheduled_delete' ) && ! wp_installing() ) {
		wp_schedule_event( time(), 'daily', 'wp_scheduled_delete' );
	}
	if ( ! wp_next_scheduled( 'delete_expired_transients' ) && ! wp_installing() ) {
		wp_schedule_event( time(), 'daily', 'delete_expired_transients' );
	}
};

// A hook's callbacks, as cron runs them: with no user.
$cron_run = function ( $hook, $args = array() ) {
	wp_set_current_user( 0 );
	do_action_ref_array( $hook, $args );
};

return array(
	// wp_publish_post(), which check_and_publish_future_post() calls.
	'publish-future-post'         => array(
		'setup'  => fn() => $cron_post(
			array(
				'import_id' => 900,
				'post_date' => '2030-01-01 10:00:00',
			)
		),
		'action' => fn( $id ) => wp_publish_post( $id ),
	),
	'publish-uncategorized-draft' => array(
		'setup'  => function () use ( $cron_post ) {
			global $wpdb;
			$id = $cron_post( array( 'post_status' => 'draft' ) );
			$wpdb->delete( $wpdb->term_relationships, array( 'object_id' => $id ) );
			clean_object_term_cache( $id, 'post' );
			return $id;
		},
		'action' => fn( $id ) => wp_publish_post( $id ),
	),
	'publish-draft-page'          => array(
		'setup'  => fn() => $cron_post(
			array(
				'post_status' => 'draft',
				'post_type'   => 'page',
			)
		),
		'action' => fn( $id ) => wp_publish_post( $id ),
	),
	'cron-publish-missed'     => array(
		'setup'  => $cron_missed,
		'action' => fn( $id ) => $cron_run( 'publish_future_post', array( $id ) ),
	),
	// Someone jumped the gun: the event comes before the post's date.
	'cron-publish-early'      => array(
		'setup'  => function () use ( $cron_post ) {
			$id = $cron_post(
				array(
					'import_id' => 900,
					'post_date' => '2030-01-01 10:00:00',
				)
			);
			wp_clear_scheduled_hook( 'publish_future_post', array( $id ) );
			wp_schedule_single_event( HYSCRIBE_CRON_OLD, 'publish_future_post', array( $id ) );
			return $id;
		},
		'action' => fn( $id ) => $cron_run( 'publish_future_post', array( $id ) ),
	),
	'cron-scheduled-delete'   => array(
		'setup'  => $cron_trash,
		'action' => fn() => $cron_run( 'wp_scheduled_delete' ),
	),
	'cron-expired-transients' => array(
		'setup'  => $cron_transients,
		'action' => fn() => $cron_run( 'delete_expired_transients' ),
	),
	'cron-auto-drafts'        => array(
		'setup'  => $cron_auto_drafts,
		'action' => fn() => $cron_run( 'wp_scheduled_auto_draft_delete' ),
	),
	'cron-privacy-requests'   => array(
		'setup'  => $cron_requests,
		'action' => fn() => $cron_run( 'wp_privacy_personal_data_cleanup_requests' ),
	),
	// An admin request on a site whose cron option has none of the four.
	'cron-schedule-events'         => array(
		'setup'  => fn() => $cron_unscheduled( $cron_view_events ),
		'action' => $cron_view,
	),
	// The default install as it is: init's events scheduled by a page view of the front end, admin.php's not.
	'cron-schedule-events-fixture' => array(
		'action' => $cron_view,
	),
	// One already scheduled, the others not.
	'cron-schedule-events-some'    => array(
		'setup'  => function () use ( $cron_unscheduled, $cron_view_events ) {
			$cron_unscheduled( $cron_view_events );
			wp_schedule_event( time() + HOUR_IN_SECONDS, 'daily', 'delete_expired_transients' );
		},
		'action' => $cron_view,
	),
	// All four scheduled: the option stays as it is.
	'cron-schedule-events-present' => array(
		'setup'  => function () use ( $cron_unscheduled, $cron_view_events, $cron_view ) {
			$cron_unscheduled( $cron_view_events );
			$cron_view();
		},
		'action' => $cron_view,
	),
	// wp-cron.php's loop, which leaves the events of other hooks as they are.
	'cron-loop'               => array(
		'setup'  => $cron_loop,
		'action' => function () use ( $cron_hooks, $cron_run ) {
			foreach ( wp_get_ready_cron_jobs() as $timestamp => $cronhooks ) {
				foreach ( $cronhooks as $hook => $keys ) {
					if ( ! in_array( $hook, $cron_hooks, true ) ) {
						continue;
					}
					foreach ( $keys as $v ) {
						if ( $v['schedule'] ) {
							wp_reschedule_event( $timestamp, $v['schedule'], $hook, $v['args'], true );
						}
						wp_unschedule_event( $timestamp, $hook, $v['args'], true );
						$cron_run( $hook, $v['args'] );
					}
				}
			}
		},
	),
);
