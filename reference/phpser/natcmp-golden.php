<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Writes crates/hyscribe-phpser/tests/golden/natcmp.tsv: PHP's
// strnatcasecmp() of every pair of the strings below, the oracle for
// hyscribe-phpser's natcmp module. Each line is
//   hex(a)<TAB>hex(b)<TAB>sign of the result (-1, 0 or 1)
//
// Usage: php reference/phpser/natcmp-golden.php > crates/hyscribe-phpser/tests/golden/natcmp.tsv

$strings = array(
	'', ' ', '0', '00', '000', '1', '01', '001', '2', '10', '010', '9', '09', '100',
	'a', 'A', 'b', 'B', 'ab', 'aB', 'Ab', 'a1', 'a01', 'a2', 'a10', 'a 1', 'a  1', 'a1b', 'a1 b',
	'img12', 'img10', 'IMG2', 'img1.png', 'img12.png', 'x2-g8', 'x2-y08', 'x2-y7', 'x8-y8',
	'1.010', '1.01', '1.1', '1.02', '1.002', '0.5', '0.05', '.5', '-1', '-2', '+1',
	' a', '  a', "\ta", "a\t", 'a ', "a\0", "\0a", "\0", 'Ä', 'ä', 'é', 'z', 'Z', '_', '[', '`',
	'Foo', 'foo', 'Foo2', 'foo10', 'Empty', 'Links', 'tag-a', 'Tag B', '0a', '00a', '0 0', '1 0', '10 ', '12', '21', 'a12b', 'a21b',
);
foreach ( $strings as $a ) {
	foreach ( $strings as $b ) {
		echo bin2hex( $a ), "\t", bin2hex( $b ), "\t", strnatcasecmp( $a, $b ) <=> 0, "\n";
	}
}
