<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's actions for the site transients url-details caches its
// fetches in (ADR 0019), loaded beside actions.php. Names start with
// `oracle_`, so they never clash with the fixture's.

return array(
	'site-transient-set'     => array(
		'setup'  => function () {
			set_site_transient( 'oracle_kept', 'one', 3600 );
			set_site_transient( 'oracle_lasting', 'one' );
		},
		'action' => function () {
			set_site_transient( 'oracle_new', '<html>a page</html>', HOUR_IN_SECONDS );
			set_site_transient( 'oracle_plain', 'a:1:{i:0;i:1;}' );
			set_site_transient( 'oracle_kept', 'two', 7200 );
			set_site_transient( 'oracle_lasting', 'two' );
		},
	),
	'site-transient-expired' => array(
		'setup'  => function () {
			set_site_transient( 'oracle_old', 'old', 3600 );
			set_site_transient( 'oracle_fresh', 'fresh', 3600 );
			update_option( '_site_transient_timeout_oracle_old', time() - 10, false );
			add_option( '_site_transient_timeout_oracle_lone', time() - 10, '', false );
		},
		'action' => function () {
			get_site_transient( 'oracle_old' );
			get_site_transient( 'oracle_fresh' );
			get_site_transient( 'oracle_lone' );
		},
	),
);
