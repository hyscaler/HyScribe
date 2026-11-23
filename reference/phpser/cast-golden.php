<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Writes crates/hyscribe-phpser/tests/golden/cast.tsv: PHP's empty(),
// is_numeric(), (float), (int) and (string) of each value, and each string
// plus 0 (null for the TypeError of one holding no number), the oracle for
// hyscribe-phpser's cast module. Each line is
//   kind<TAB>hex(serialize(value))<TAB>hex(serialize(result))
//
// Usage: php reference/phpser/cast-golden.php > crates/hyscribe-phpser/tests/golden/cast.tsv

ini_set( 'serialize_precision', '-1' );
ini_set( 'precision', '14' );

$values = array(
	null, true, false, 0, 1, -1, 7, PHP_INT_MAX, PHP_INT_MIN,
	0.0, -0.0, 1.0, 0.1, 0.1 + 0.2, 1 / 3, 2.5, 0.196, -0.00012345, 1.0e-10,
	1.5e-5, 0.0001, 0.00001, 1e25, -1e25, 1e15, 1e14, 99999999999999.9,
	100000000000000.0, 123456789012345.6, 1.2345678901234567, 676144192310117.2,
	INF, -INF, NAN,
	'', '0', '00', '0.0', ' ', 'a', 'abc', 'Array', '1', ' 1', '1 ', "\n1\n",
	"\t1", "\r1", "\x0b1", "1\x0b", "\x0c1", "1\x0c", "1\0", "\0" . '1', '+1', '-1', '1.5',
	'.5', '5.', '-.5', '+.5', '.', '-', '+', '1e5', '1E5', '1e', '1e+', '1e-',
	'1.5e+3', '-1.5e3x', '1.e5', '-.5e-2', '1e400', '-1e400', '0x1A', '1 1',
	'1.5.6', 'inf', 'NAN', '1.2rem', '  3px', " \t\n42", '12abc', "\xff", "1\xff",
	'9223372036854775808', '-9223372036854775809', '0.30000000000000004',
	'007', '1_000', '١', '-5', ' -12abc', '1e3', '1.9', '-1.9', '9.9e18', '-9.9e18',
	9.9e18, -9.9e18, 1.9, -1.9, 1e300, 9223372036854775808.0, -9223372036854775808.0, 2.5e19, -2.5e19, '9223372036854775807.5', '+7', '  +3e2x', '.5e1', '9223372036854775808x',
	array(), array( 0 ), array( 'a' => 1 ), array( array() ),
);

foreach ( array( 'empty', 'is_numeric', 'float', 'int', 'string', 'plus' ) as $kind ) {
	foreach ( $values as $value ) {
		switch ( $kind ) {
			case 'empty':
				$result = empty( $value );
				break;
			case 'is_numeric':
				$result = is_numeric( $value );
				break;
			case 'float':
				$result = (float) $value;
				break;
			case 'int':
				$result = (int) $value;
				break;
			case 'plus':
				if ( ! is_string( $value ) ) {
					continue 2;
				}
				try {
					$result = @( $value + 0 );
				} catch ( TypeError $e ) {
					$result = null;
				}
				break;
			default:
				// Arrays print "Array" with a warning.
				$result = @(string) $value;
		}
		echo $kind, "\t", bin2hex( serialize( $value ) ), "\t", bin2hex( serialize( $result ) ), "\n";
	}
}
