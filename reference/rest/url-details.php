<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The must-use plugin of reference/rest/url-details.list: the test site
// pages.test, which reference/url-details/golden.sh serves inside the oracle
// container on 127.0.0.1, counts as an external host for
// wp_http_validate_url(), as a public site would.

add_filter(
	'http_request_host_is_external',
	function ( $external, $host ) {
		return $external || 'pages.test' === $host;
	},
	10,
	2
);
