<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// version_compare( $a, $b ) over pairs of versions, as Hello Elementor's
// version write asks it of the stored hello_theme_version (MVP 10.5):
//   php reference/hello-elementor/version-golden.php > crates/hyscribe-theme-hello-elementor/tests/golden/version-compare.tsv
// Each line: a, b (hex), and -1, 0 or 1.

$versions = [
	'', '0', '1', '1.0', '1.0.0', '1.', '.1', '3.5.2', '3.5.1', '3.5.10', '3.4.4', '3.5',
	'3.5.2.0', '3.5.2.1', '3.5.2-beta', '3.5.2beta1', '3.5.2RC1', '3.5.2rc', '3.5.2-dev',
	'3.5.2pl1', '3.5.2p', '3.5.2a', '3.5.2alpha2', '3.5.2b', '3.5.2#', '3.5.2-x', '4.0-beta',
	'4', '10', '2', 'abc', 'dev', 'pl', '#', '#1', '-1', '1--2', '1__2', '1++2', '1 2', '1..2',
	'1.a', 'a.1', '1a', 'a1', '1.0a', "3.5.2\xc3\xa9", '99999999999999999999', '3.05.2', '03.5.2',
	'1.0.0.0.0', '1.0.0.0.1', 'v3.5.2', '3.5.2 ', ' 3.5.2', 'true', 'RC', 'beta', 'alpha',
];
foreach ( $versions as $a ) {
	foreach ( $versions as $b ) {
		echo bin2hex( $a ), "\t", bin2hex( $b ), "\t", version_compare( $a, $b ), "\n";
	}
}
