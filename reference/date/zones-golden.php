<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Writes crates/hyscribe-render/tests/golden/date-zones.tsv: PHP's dates in
// every named time zone, around its transitions. A stored local time is
// read as get_post_datetime() reads it, and a Unix time as wp_date() reads
// it. Each line is
//   zone<TAB>local|unix<TAB>input<TAB>result
// Run with reference/date/zones-golden.sh.

mt_srand( 20261111 );

$format = 'e|T|I|O|P|p|Z|U|Y-m-d H:i:s|B';
$names  = DateTimeZone::listIdentifiers( DateTimeZone::ALL_WITH_BC );
$names  = array_merge( $names, array( 'utc', 'europe/paris', 'US/Pacific' ) );

$local = function ( $zone, $stored ) use ( $format ) {
	$date = date_create_immutable_from_format( 'Y-m-d H:i:s', $stored, $zone );
	return $date->format( $format );
};
$unix = function ( $zone, $time ) use ( $format ) {
	return ( new DateTimeImmutable( '@' . $time ) )->setTimezone( $zone )->format( $format );
};

foreach ( array_unique( $names ) as $name ) {
	$zone        = new DateTimeZone( $name );
	$transitions = $zone->getTransitions( -5000000000, 7300000000 ) ?: array();
	$picked      = array();
	foreach ( $transitions as $i => $transition ) {
		$year = (int) gmdate( 'Y', $transition['ts'] );
		if ( $i > 0 && ( ( $year >= 2025 && $year <= 2027 ) || 0 === mt_rand( 0, 30 ) ) ) {
			$picked[] = array( $transitions[ $i - 1 ]['offset'], $transition );
		}
	}
	$times  = array();
	$stored = array( '1850-01-01 00:00:00', '1970-01-01 00:00:00', '2026-01-15 12:00:00', '2026-07-15 12:00:00', '2100-01-15 12:00:00', '2100-07-15 12:00:00', '2200-12-31 23:59:59' );
	foreach ( $picked as list( $before, $transition ) ) {
		$at      = $transition['ts'];
		$times[] = $at - 1;
		$times[] = $at;
		$after   = $transition['offset'];
		$low     = min( $before, $after );
		$high    = max( $before, $after );
		foreach ( array( $at + $low - 1, $at + $low, $at + intdiv( $low + $high, 2 ), $at + $high - 1, $at + $high ) as $wall ) {
			$stored[] = gmdate( 'Y-m-d H:i:s', $wall );
		}
	}
	foreach ( array_unique( $stored ) as $time ) {
		echo $name, "\tlocal\t", $time, "\t", $local( $zone, $time ), "\n";
	}
	foreach ( array_merge( array( -5000000000, 0, 1782900000, 7300000000 ), $times ) as $time ) {
		echo $name, "\tunix\t", $time, "\t", $unix( $zone, $time ), "\n";
	}
}
