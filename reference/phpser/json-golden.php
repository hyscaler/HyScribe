<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Writes crates/hyscribe-phpser/tests/golden/json-encode.tsv: json_encode()
// with no flags, as wp_json_encode() calls it, and with the flags
// serialize_block_attributes() passes, the oracle for hyscribe-phpser's
// json_encode and json_encode_unescaped, and with the flags
// print_script_module_data() passes, for json_encode_script_data, and pretty
// printed, as the REST API sends it for `_pretty`, for json_encode_pretty. Each line
// is
//   hex(serialize(value))<TAB>hex(json_encode(value))<TAB>hex(json_encode(value, flags))<TAB>hex(json_encode(value, script flags))<TAB>hex(json_encode(value, JSON_PRETTY_PRINT))
//
// Usage: php reference/phpser/json-golden.php > crates/hyscribe-phpser/tests/golden/json-encode.tsv

ini_set( 'serialize_precision', '-1' );

class Golden_Props {
	public $open = 'yes';
	protected $kept = 'no';
	private $hidden = 'no';
	public $list = array( 1, '<b>' );
}
$numbered       = new stdClass();
$numbered->{'7'} = 'seven';

$values = array(
	null, true, false, 0, -7, PHP_INT_MAX, PHP_INT_MIN, 0.0, -0.0, 1.0, 1.5, 0.1, 1e25, -1e-7, 123456789012345678.0, 1e15, 1e16,
	'', 'plain', 'a/b', 'quote " and \\ backslash', "tab\tnew\nline\rret", "\x00\x01\x1f\x7f", 'é', '€', '😀', "\u{2028}\u{2029}", "\u{80}\u{85}\u{a0}", 'a\\/b', '--<!-- -->', '<>&\'',
	array(), array( array() ), array( 1, 2, 3 ), array( 'a' => 1 ), array( 1 => 'x' ), array( 0 => 'a', 2 => 'b' ), array( 'k' => array( 'n' => array() ) ),
	array( 'mixed' => array( 1, 'two', array( 'three' => 3.0 ) ), 'null' => null ),
	array( 'top' => 'var:preset|spacing|40', 'type' => 'constrained', 'contentSize' => '620px' ),
	new stdClass(), (object) array( 'a' => 'x/y', 'b' => array() ), new Golden_Props(), $numbered, array( 'isOpen' => new stdClass() ),
);
foreach ( $values as $value ) {
	echo bin2hex( serialize( $value ) ), "\t", bin2hex( json_encode( $value ) ), "\t", bin2hex( json_encode( $value, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE ) ), "\t", bin2hex( json_encode( $value, JSON_HEX_TAG | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_LINE_TERMINATORS ) ), "\t", bin2hex( json_encode( $value, JSON_PRETTY_PRINT ) ), "\n";
}
