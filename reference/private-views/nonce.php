<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/private-views/golden.sh: a request with
// hyscribe_preview_nonce=<id> answers wp_create_nonce( 'post_preview_<id>' )
// for the user its cookies log in, the nonce the editor's Preview link
// carries, and nothing else. It writes nothing.
add_action(
	'init',
	function () {
		if ( isset( $_GET['hyscribe_preview_nonce'] ) ) {
			exit( wp_create_nonce( 'post_preview_' . (int) $_GET['hyscribe_preview_nonce'] ) );
		}
	},
	0
);
