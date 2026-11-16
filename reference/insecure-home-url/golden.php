<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints wp_replace_insecure_home_url() over one content for each home,
// siteurl and https_migration_required state. The first line is
// hex(content), then lines are
//   hex(home)<TAB>hex(siteurl)<TAB>hex(stored option or empty)<TAB>stored(1 or 0)<TAB>hex(result)
// with `=` for a result that is the content unchanged.
// The URLs come through `pre_option_` filters, as get_option() would return
// them; the option's stored text goes through maybe_unserialize(), as
// get_option() reads it from the database. Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

if ( false !== get_option( 'https_migration_required' ) ) {
	fwrite( STDERR, "https_migration_required is set in the fixture\n" );
	exit( 1 );
}

$content = '<a href="http://example.org/x">x</a> <a href="http://example.org">h</a> '
	. '"http:\/\/example.org\/y" http://example.orgx http://example.org:8443/p '
	. 'http://Example.org/c https://example.org/s http://example.org/sub/z '
	. 'http://other.org/o http://example.org/https://example.org/q '
	. 'http://user@example.org/u http://example.org/http://example.org/w https:example.org/v';

$sites = array(
	array( 'https://example.org', 'https://example.org' ),
	array( 'https://example.org/', 'https://example.org/' ),
	array( 'https://example.org/sub', 'https://example.org' ),
	array( 'https://example.org', 'https://example.org/wp' ),
	array( 'https://example.org', 'https://example.org:8443' ),
	array( 'https://example.org:8443', 'https://example.org' ),
	array( 'https://example.org', 'http://example.org' ),
	array( 'http://example.org', 'https://example.org' ),
	array( 'http://example.org', 'http://example.org' ),
	array( 'https://example.org', 'https://other.org' ),
	array( 'https://example.org', 'https://Example.org' ),
	array( 'HTTPS://example.org', 'https://example.org' ),
	array( 'https://example.org', 'HTTPS://example.org' ),
	array( ' https://example.org', 'https://example.org' ),
	array( 'https://example.org', ' https://example.org' ),
	array( "https://example.org \n", 'https://example.org' ),
	array( 'https://example.org', "https://example.org \n" ),
	array( "https://example.org \n", "https://example.org \n" ),
	array( 'https://user@example.org', 'https://example.org' ),
	array( 'https:example.org', 'https:other.org' ),
	array( '//example.org', 'https://example.org' ),
	array( 'https://example.org', '//example.org' ),
	array( 'https://example.org/https://example.org', 'https://example.org' ),
	array( '', 'https://example.org' ),
);
$options = array( null, '1', '', '0', ' ', 'false', 'b:0;', 'b:1;', 'i:0;', 'a:0:{}', 'a:1:{i:0;i:0;}', 's:0:"";', 's:1:"0";', 'N;', 'b:2;', 'x:0;' );

echo bin2hex( $content ), "\n";
foreach ( $sites as list( $home, $siteurl ) ) {
	foreach ( $options as $stored ) {
		$filters = array(
			'pre_option_home'    => static fn() => $home,
			'pre_option_siteurl' => static fn() => $siteurl,
		);
		if ( null !== $stored ) {
			// get_option() returns maybe_unserialize() of the stored text; a
			// false short circuit would fall through to the (missing) row.
			$filters['pre_option_https_migration_required'] = static function () use ( $stored ) {
				$value = maybe_unserialize( $stored );
				return false === $value ? 0 : $value;
			};
		}
		foreach ( $filters as $hook => $filter ) {
			add_filter( $hook, $filter );
		}
		$result = wp_replace_insecure_home_url( $content );
		foreach ( $filters as $hook => $filter ) {
			remove_filter( $hook, $filter );
		}
		echo bin2hex( $home ), "\t", bin2hex( $siteurl ), "\t", bin2hex( (string) $stored ), "\t", null === $stored ? '0' : '1', "\t", $content === $result ? '=' : bin2hex( $result ), "\n";
	}
}
