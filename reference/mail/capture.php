<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/mail/golden.sh: wp_mail() keeps its
// arguments, after its `wp_mail` filter, and reports success without sending.

add_filter(
	'pre_wp_mail',
	function ( $sent, $atts ) {
		$GLOBALS['hyscribe_mail'][] = $atts;
		return true;
	},
	10,
	2
);
