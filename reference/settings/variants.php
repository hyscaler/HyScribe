<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/settings/golden.sh, through filters only:
// each variant gives the options it names, as get_option() reads them (a
// named zone's gmt_offset still from wp_timezone_override_offset()), s1nopages
// finds no pages, and every date of now is printed at 1794500000, the test
// site's clock.
$hyscribe_options = array(
	's1static'   => array(
		'show_on_front' => 'page',
		'page_on_front' => '2',
	),
	's1same'     => array(
		'show_on_front'  => 'page',
		'page_on_front'  => '2',
		'page_for_posts' => '2',
	),
	's1privacy'  => array(
		'show_on_front'  => 'page',
		'page_on_front'  => '2',
		'page_for_posts' => '3',
	),
	's1private'  => array(
		'blog_public'     => '0',
		'rss_use_excerpt' => '1',
	),
	's1general'  => array(
		'blogname'           => 'My <b>"Site"</b> & Co',
		'blogdescription'    => 'Tag \'line\' &amp; more',
		'date_format'        => 'j F Y',
		'time_format'        => 'H:i',
		'start_of_week'      => '0',
		'users_can_register' => '1',
		'default_role'       => 'author',
		'new_admin_email'    => 'new<b>@example.test',
		'timezone_string'    => 'America/New_York',
	),
	's1offset'   => array(
		'timezone_string' => '',
		'gmt_offset'      => '-5.5',
		'date_format'     => 'Y-m-d',
		'time_format'     => 'g:i A',
	),
	's1etc'      => array(
		'timezone_string' => 'Etc/GMT+3',
		'date_format'     => '',
	),
	's1kolkata'  => array(
		'timezone_string' => 'Asia/Kolkata',
		'default_role'    => 'editor',
	),
	's1calcutta' => array(
		'timezone_string' => 'Asia/Calcutta',
		'new_admin_email' => 'admin@default-install.test',
	),
	's1utc'      => array(
		'timezone_string' => 'UTC',
		'start_of_week'   => '6',
	),
	's1plus'     => array(
		'timezone_string' => '',
		'gmt_offset'      => '5.75',
	),
	's1writing'  => array(
		'initial_db_version'   => '30000',
		'use_smilies'          => '1',
		'use_balanceTags'      => '1',
		'default_category'     => '99',
		'default_post_format'  => 'aside',
		'link_manager_enabled' => '1',
		'mailserver_url'       => 'mail.example.test',
		'mailserver_port'      => '995',
		'mailserver_login'     => 'me@example.test',
		'mailserver_pass'      => 'p"w&<x>',
		'ping_sites'           => "http://a.test/\nhttp://b.test/?x=<y>&z",
	),
);
$hyscribe_variant = $_COOKIE['hyscribe_variant'] ?? '';
foreach ( $hyscribe_options[ $hyscribe_variant ] ?? array() as $hyscribe_name => $hyscribe_value ) {
	add_filter( "pre_option_$hyscribe_name", fn() => $hyscribe_value );
}
if ( 's1nopages' === $hyscribe_variant ) {
	add_filter( 'get_pages', fn() => array() );
}
add_filter(
	'date_i18n',
	fn( $date, $format, $timestamp, $gmt ) => wp_date( $format, 1794500000, $gmt ? new DateTimeZone( 'UTC' ) : null ),
	10,
	4
);
