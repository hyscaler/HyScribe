<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's actions of personal data requests (MVP 8.3), beside
// actions.php: WordPress's own functions over what wp-login.php hands them,
// with mail taken as sent, as HyScribe answers until its transport sends it.

// Privacy requests written raw, as the cron oracle writes them: a pending
// export and erasure, a failed one, a confirmed one nobody was told of, one
// they were told of, and a completed one.
$privacy_requests = function () {
	global $wpdb;
	$ids = array();
	foreach ( array(
		array( 'export_personal_data', 'request-pending' ),
		array( 'remove_personal_data', 'request-pending' ),
		array( 'export_personal_data', 'request-failed' ),
		array( 'export_personal_data', 'request-confirmed' ),
		array( 'remove_personal_data', 'request-confirmed' ),
		array( 'export_personal_data', 'request-completed' ),
		array( 'export_personal_data', 'request-pending' ),
	) as list( $name, $status ) ) {
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
				'post_modified'         => '2020-01-01 00:00:00',
				'post_modified_gmt'     => '2020-01-01 00:00:00',
				'post_content_filtered' => '',
				'post_type'             => 'user_request',
				'comment_status'        => 'closed',
				'ping_status'           => 'closed',
			)
		);
		$ids[] = $wpdb->insert_id;
	}
	// The erasure confirmed already told the admin.
	update_post_meta( $ids[4], '_wp_admin_notified', true );
	return $ids;
};

return array(
	// The confirmation link: each request, a post that is no request, and a
	// pending request under a negative ID, whose status wp_update_post() leaves.
	'login-confirm-request' => array(
		'setup'  => $privacy_requests,
		'action' => function ( $ids ) {
			add_filter( 'pre_wp_mail', '__return_true' );
			foreach ( array_merge( array_slice( $ids, 0, 6 ), array( 1, -$ids[6] ) ) as $id ) {
				do_action( 'user_request_action_confirmed', $id );
			}
		},
	),
);
