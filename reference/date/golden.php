<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Writes crates/hyscribe-render/tests/golden/date.tsv: PHP's date
// formatting of stored local times, the oracle for hyscribe-render's date
// module. wp_date() formats the same way for the default English locale.
// Each line is
//   hex(stored)<TAB>hex(zone)<TAB>hex(format)<TAB>hex(result)
//
// Usage: php reference/date/golden.php > crates/hyscribe-render/tests/golden/date.tsv

mt_srand( 20261028 );

$stored = array(
	'2026-10-27 08:15:42', '2026-01-01 00:00:00', '2025-12-31 23:59:59', '2024-02-29 12:00:00',
	'2021-01-03 13:05:09', '2020-12-31 00:30:00', '2027-01-01 11:00:00', '2000-03-01 01:01:01',
	'1999-12-31 12:59:59', '1970-01-01 00:00:00', '1969-12-31 23:59:59', '2038-01-19 03:14:08',
	'1900-02-28 06:07:08', '0999-05-05 05:05:05', '2026-03-29 02:30:00', '2009-12-28 18:00:00',
	'1900-03-01 00:00:00', '2000-02-29 10:00:00', '2000-03-01 00:00:00', '2100-02-28 23:00:00',
	'2100-03-01 01:00:00', '2400-02-29 00:00:00', '1600-02-29 12:00:00', '0000-03-01 00:00:00',
);
for ( $i = 0; $i < 60; $i++ ) {
	$stored[] = sprintf( '%04d-%02d-%02d %02d:%02d:%02d', mt_rand( 1950, 2100 ), mt_rand( 1, 12 ), mt_rand( 1, 28 ), mt_rand( 0, 23 ), mt_rand( 0, 59 ), mt_rand( 0, 59 ) );
}

$zones = array( '+00:00', 'UTC', '+05:30', '-03:00', '+14:00', '-09:30' );

$formats = str_split( 'dDjlNSwzWFmMntLoXxYyaABgGhHisuveIOPpTZcrU' );
$formats = array_merge(
	$formats,
	array( 'F j, Y', 'g:i a', 'Y-m-d H:i:s', 'D, d M Y H:i:s O', 'jS \o\f F', '\\\\Y', 'Y\\', 'l \t\h\e jS', 'H\h i\m', '', 'q~!', 'ü Y', 'c', 'r', 'U' )
);

foreach ( $stored as $time ) {
	foreach ( $zones as $zone ) {
		$date = new DateTimeImmutable( $time, new DateTimeZone( $zone ) );
		foreach ( $formats as $format ) {
			echo bin2hex( $time ), "\t", bin2hex( $zone ), "\t", bin2hex( $format ), "\t", bin2hex( $date->format( $format ) ), "\n";
		}
	}
}
