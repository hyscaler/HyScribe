<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/rest/people-off.list: the public users
// endpoint off, as REST_PUBLIC_USERS=off serves it, which answers a visitor
// as a site where no user has published posts. Writes nothing.
add_filter(
	'get_usernumposts',
	function ( $count ) {
		return is_user_logged_in() ? $count : 0;
	}
);
add_action(
	'pre_user_query',
	function ( $query ) {
		if ( ! is_user_logged_in() && ! empty( $query->query_vars['has_published_posts'] ) ) {
			$query->query_where .= ' AND 1=0';
		}
	}
);
