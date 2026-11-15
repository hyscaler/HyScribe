<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Records sanitize_option() for the options the Settings screens save, as
// crates/hyscribe-write/tests/golden/sanitize-option.tsv:
//   option<TAB>hex(serialize(value))<TAB>hex(serialize(sanitized))<TAB>error
// the error empty when there is none, and `fatal` for the sanitized value
// of an input PHP stops on (a TypeError). With the argument `zones` it prints
// timezone_identifiers_list( DateTimeZone::ALL_WITH_BC ) instead, one name a
// line, as crates/hyscribe-write/src/options/zones.txt. Only reads. Run in
// the PHP container over a fresh db.sh up:
//   docker exec -i hyscribe-default-install-php php /dev/stdin [zones] < reference/settings-saves/sanitize-golden.php
// HyScribe's constants: no translations can be installed (DISALLOW_FILE_MODS).
define( 'DISALLOW_FILE_MODS', true );
require '/var/www/html/wp-load.php';
require_once ABSPATH . 'wp-admin/includes/template.php';
remove_filter( 'option_home', '_config_wp_home' );
remove_filter( 'option_siteurl', '_config_wp_siteurl' );
ini_set( 'display_errors', '0' );

if ( 'zones' === ( $argv[1] ?? '' ) ) {
	echo implode( "\n", timezone_identifiers_list( DateTimeZone::ALL_WITH_BC ) ), "\n";
	exit;
}

$emails = array( 'admin@example.test', ' Admin@Example.TEST ', 'bad', '', null, "a\xffb@example.test", 'a<b>@example.test', 'x@y', array( 'a@example.test' ) );
$ints   = array( '150', '-5', '3.7', 'abc', '', null, '12abc', ' 7', array( '1' ), array(), '1e3', '-0', true );
$urls   = array( 'http://example.test', 'https://example.test/wp/', 'example.test', 'HTTP://Example.test', 'http://exa mple.test', 'javascript:alert(1)//http://x', '', null, "http://example.test/\xff", 'ftp://http://x', array( 'http://x' ) );
$texts  = array( 'F j, Y', '<b>Y</b>', '\c\u\s\t\o\m', '<a href="x" title="y">l</a> <em>j</em>', null, '', "caf\xc3\xa9 \xff", 'a &amp; b < c', array( 'Y' ) );
$lines  = array( "a\n b \n\na\nc", '', null, "x\r\ny\n\n", " \n ", array( 'a' ) );
$cases  = array(
	'admin_email'                  => $emails,
	'new_admin_email'              => $emails,
	'thumbnail_size_w'             => $ints,
	'start_of_week'                => array( '0', '6', '9' ),
	'site_icon'                    => array( '9', '-9' ),
	'page_on_front'                => array( '2', 'x' ),
	'users_can_register'           => array( '1', null ),
	'comments_per_page'            => array( '1e3', '50' ),
	'mailserver_port'              => array( '-995' ),
	'posts_per_page'               => array( '0', '-1', '-5', '7', 'x', '2.5', null, array( 'a' ), '-1.5' ),
	'posts_per_rss'                => array( '-3', '' ),
	'default_ping_status'          => array( '0', '', 'open', 'closed', null, 'x', array( 'a' ) ),
	'default_comment_status'       => array( '0', 'open' ),
	'blogname'                     => array( 'My <b>"Site"</b> & Co', "caf\xc3\xa9", "bad\xffbyte", "caf\xc3\xa9\xff", '', null, "it's", "a &amp; b", array( 'x' ) ),
	'blogdescription'              => array( '<script>x</script>', ' Just a tagline ' ),
	'blog_public'                  => array( null, '0', '1', '2', 'x', '' ),
	'date_format'                  => $texts,
	'time_format'                  => array( 'g:i a', '<i>H</i>' ),
	'mailserver_url'               => array( 'mail.example.com<script>x</script>' ),
	'mailserver_login'             => array( 'login@example.com' ),
	'mailserver_pass'              => array( 'p<a>ss', 'a <secret>' ),
	'ping_sites'                   => array( "https://rpc.pingomatic.com/\n\n  http://example.test/x  \njavascript:alert(1)\nfoo bar", '', null, "a\r\nb", array( 'x' ) ),
	'gmt_offset'                   => array( '5.5', '-3', '+2', '1e1', 'x', '', null, ' 4', '4 ', '0x1A', '5.5abc', '-0.5', '.5', '14', array( '1' ) ),
	'siteurl'                      => $urls,
	'home'                         => $urls,
	'WPLANG'                       => array( '', 'de_DE', null, 'en_US' ),
	'timezone_string'              => array( 'Europe/London', 'europe/london', 'Mars/Base', '', 'UTC', 'US/Eastern', 'Asia/Calcutta', null, 'Etc/GMT+5', 'Factory', '0' ),
	'permalink_structure'          => array( '/%postname%/', '/archives/', '', '/%year%/<x>/%postname%/', '/%year% %postname%/', 'http://x/%postname%/', '/%%/', "/%year%/\xff%postname%/", '/%a/b%/', '/x/%postname', array( 'x' ) ),
	'category_base'                => array( 'topics', '/topics/', 'a b', '', '/topics/ ', 'http://x/y', "a\"b'c" ),
	'tag_base'                     => array( '/#tags//' ),
	'default_role'                 => array( 'editor', 'king', '', null, 'administrator', 'Editor', array( 'x' ) ),
	'moderation_keys'              => $lines,
	'disallowed_keys'              => $lines,
	'comments_notify'              => array( '1', null, array( 'a' ) ),
	'image_default_link_type'      => array( null ),
	'close_comments_for_old_posts' => array( '1' ),
);

foreach ( $cases as $option => $values ) {
	foreach ( $values as $value ) {
		$GLOBALS['wp_settings_errors'] = array();
		try {
			$sanitized = bin2hex( serialize( sanitize_option( $option, $value ) ) );
		} catch ( Throwable $e ) {
			$sanitized = 'fatal';
		}
		$errors = array_column( get_settings_errors(), 'message' );
		echo $option, "\t", bin2hex( serialize( $value ) ), "\t", $sanitized, "\t", implode( '|', $errors ), "\n";
	}
}
