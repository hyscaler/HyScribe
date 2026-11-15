<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/admin-saves/golden.sh: every date of now
// the screens after a save print is printed at 1794500000, the test site's
// clock, as reference/settings/variants.php prints them.
add_filter(
	'date_i18n',
	fn( $date, $format, $timestamp, $gmt ) => wp_date( $format, 1794500000, $gmt ? new DateTimeZone( 'UTC' ) : null ),
	10,
	4
);
