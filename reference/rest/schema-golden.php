<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Writes crates/hyscribe-rest/tests/golden/schema.tsv: what
// rest_validate_value_from_schema() and rest_sanitize_value_from_schema()
// make of each value under each schema, as reference/rest/golden.sh runs it
// in the PHP container. Each line is
//   hex(json schema)<TAB>hex(serialize(value))<TAB>hex(validated)<TAB>hex(sanitized)
// where validated is "true" or the error as JSON (code, message, data), and
// sanitized is the error as JSON or serialize() of the value. Only reads.
require '/var/www/html/wp-load.php';

// The schema of the posts route's `categories` argument.
$terms = ( new WP_REST_Posts_Controller( 'post' ) )->get_collection_params()['categories'];
unset( $terms['description'] );

$cases = array(
	array( array( 'type' => 'string', 'enum' => array( 'view', 'embed', 'edit' ) ), array( 'view', 'bogus', '', array( 'edit' ), 'EDIT', 5 ) ),
	array( array( 'type' => 'string', 'enum' => array( 'only' ) ), array( 'only', 'x' ) ),
	array( array( 'type' => 'string', 'enum' => array( 'a', 'b' ) ), array( 'c' ) ),
	array( array( 'type' => 'integer', 'enum' => array( 1, 2.0, true, null, array( 'x' ) ) ), array( '1', '2', 3 ) ),
	array( array( 'type' => 'string' ), array( 'x', 5, 1.5, true, null, array(), array( 'a' ) ) ),
	array( array( 'type' => 'null' ), array( null, '', 0 ) ),
	array( array( 'type' => 'boolean' ), array( true, false, 'true', 'FALSE', '0', '1', 'yes', 0, 1, 2, 1.0, null, array() ) ),
	array( array( 'type' => 'integer' ), array( 5, '5', ' -7 ', '+3', '1.0', '1.5', '1e3', 'x', 2.0, 2.5, true, null, '99999999999999999999', array() ) ),
	array( array( 'type' => 'number' ), array( 5, '5.5', 'x', ' 1 ', true, null, '1e400' ) ),
	array( array( 'type' => 'integer', 'minimum' => 1 ), array( 0, 1, '2' ) ),
	array( array( 'type' => 'integer', 'minimum' => 1, 'exclusiveMinimum' => true ), array( 1, 2 ) ),
	array( array( 'type' => 'integer', 'maximum' => 100 ), array( 100, 101 ) ),
	array( array( 'type' => 'integer', 'maximum' => 100, 'exclusiveMaximum' => true ), array( 99, 100 ) ),
	array( array( 'type' => 'integer', 'minimum' => 1, 'maximum' => 100 ), array( 0, 1, 100, 101 ) ),
	array( array( 'type' => 'integer', 'minimum' => 1, 'maximum' => 100, 'exclusiveMinimum' => true ), array( 1, 100 ) ),
	array( array( 'type' => 'integer', 'minimum' => 1, 'maximum' => 100, 'exclusiveMaximum' => true ), array( 1, 100 ) ),
	array( array( 'type' => 'integer', 'minimum' => 1, 'maximum' => 100, 'exclusiveMinimum' => true, 'exclusiveMaximum' => true ), array( 1, 50, 100 ) ),
	array( array( 'type' => 'number', 'minimum' => 0.5, 'maximum' => 2.5 ), array( 0.4, 2.6, '1.5' ) ),
	array( array( 'type' => 'number', 'multipleOf' => 0.5 ), array( 1.5, 1.2, '3' ) ),
	array( array( 'type' => 'integer', 'multipleOf' => 3 ), array( 9, 10 ) ),
	array( array( 'type' => 'string', 'minLength' => 2 ), array( 'a', 'ab', 'é' ) ),
	array( array( 'type' => 'string', 'minLength' => 1 ), array( '' ) ),
	array( array( 'type' => 'string', 'maxLength' => 2 ), array( 'abc', 'éé', 'ééé' ) ),
	array( array( 'type' => 'string', 'maxLength' => 1 ), array( 'ab' ) ),
	array( array( 'type' => 'array' ), array( array( 1, 2 ), array( 'a' => 1 ), 'a,b c', '', 5, null, true ) ),
	array( array( 'type' => 'array', 'items' => array( 'type' => 'integer' ) ), array( array( '1', '2' ), array( '1', 'x' ), '3, 4', array( 5 => '7' ) ) ),
	array( array( 'type' => 'array', 'minItems' => 2 ), array( array( 1 ), array( 1, 2 ) ) ),
	array( array( 'type' => 'array', 'minItems' => 1 ), array( array() ) ),
	array( array( 'type' => 'array', 'maxItems' => 1 ), array( array( 1, 2 ) ) ),
	array( array( 'type' => 'array', 'maxItems' => 1000 ), array( array( 1 ) ) ),
	array( array( 'type' => 'array', 'maxItems' => 2 ), array( array( 1, 2, 3 ) ) ),
	array( array( 'type' => 'array', 'uniqueItems' => true ), array( array( 1, 2, 1 ), array( 1, '1' ), array( array( 'a' => 1, 'b' => 2 ), array( 'b' => 2, 'a' => 1 ) ), array( 1.0, 1 ) ) ),
	array( array( 'type' => 'array', 'items' => array( 'type' => 'integer' ), 'uniqueItems' => true ), array( array( '1', 1 ) ) ),
	array( array( 'type' => 'object' ), array( array( 'a' => 1 ), array( 1, 2 ), '', 'x', 5, null ) ),
	array( array( 'type' => 'object', 'properties' => array( 'a' => array( 'type' => 'integer' ) ) ), array( array( 'a' => '1', 'b' => 'x' ), array( 'a' => 'x' ) ) ),
	array( array( 'type' => 'object', 'properties' => array( 'a' => array( 'type' => 'integer', 'required' => true ) ) ), array( array( 'b' => 1 ), array( 'a' => 1 ) ) ),
	array( array( 'type' => 'object', 'required' => array( 'a' ), 'properties' => array( 'a' => array( 'type' => 'integer' ) ) ), array( array( 'b' => 1 ) ) ),
	array( array( 'type' => 'object', 'additionalProperties' => false, 'properties' => array( 'a' => array( 'type' => 'string' ) ) ), array( array( 'a' => 'x', 'b' => 1 ), array( 'a' => 'x' ) ) ),
	array( array( 'type' => 'object', 'additionalProperties' => array( 'type' => 'integer' ) ), array( array( 'a' => '1' ), array( 'a' => 'x' ) ) ),
	array( array( 'type' => 'object', 'minProperties' => 1 ), array( array() ) ),
	array( array( 'type' => 'object', 'minProperties' => 2 ), array( array( 'a' => 1 ) ) ),
	array( array( 'type' => 'object', 'maxProperties' => 1 ), array( array( 'a' => 1, 'b' => 2 ) ) ),
	array( array( 'type' => 'object', 'maxProperties' => 2 ), array( array( 'a' => 1, 'b' => 2, 'c' => 3 ) ) ),
	array( array( 'type' => array( 'string', 'integer' ) ), array( '5', 5, '', 'x', null ) ),
	array( array( 'type' => array( 'integer', 'string' ) ), array( '5', '' ) ),
	array( array( 'type' => array( 'boolean', 'null' ) ), array( 'x', null, 'true' ) ),
	array( array( 'type' => array( 'array', 'object' ) ), array( array( 1 ), array( 'a' => 1 ), '' ) ),
	array( array( 'type' => array( 'number', 'string' ), 'minimum' => 3 ), array( '1', 'x' ) ),
	array( array( 'type' => array( 'string', 'bogus' ) ), array( 5 ) ),
	array( array( 'type' => 'bogus' ), array( 'x' ) ),
	array( array( 'type' => 'number', 'minimum' => 1 ), array( 0.5, '2.5' ) ),
	array( array( 'type' => 'string', 'minLength' => 1000 ), array( 'a' ) ),
	array( array( 'type' => 'array', 'enum' => array( array( 'x' ) ) ), array( array( 'x' ), array( 'y' ), array( 'x', 'y' ) ) ),
	array( array( 'type' => 'number', 'enum' => array( 1, 2 ) ), array( '2', 3 ) ),
	array( array( 'type' => 'array', 'uniqueItems' => true ), array( array( array( 1, 2 ), array( 1, 2 ) ) ) ),
	array( array( 'type' => 'string', 'minLength' => 100 ), array( 'a' ) ),
	array( array( 'type' => 'string', 'maxLength' => 2 ), array( "\xff\xff\xff", "a\xe2\x82" ) ),
	array( array( 'type' => 'object', 'enum' => array( array( 'a' => 1, 'b' => 2 ) ) ), array( array( 'b' => 2, 'a' => 1 ), array( 'a' => 1 ) ) ),
	array( array( 'type' => array( 'array', 'string' ) ), array( 'a,b' ) ),
	array( array( 'type' => 'integer', 'minimum' => 1 ), array( ' 2 ', "\x0b3" ) ),
	array( array( 'type' => 'string', 'format' => 'email' ), array( 'a@example.test', 'not an email', ' a@b.test ', '<b>x</b>@y.test', '' ) ),
	array( array( 'type' => 'string', 'format' => 'uri' ), array( 'http://example.test/a b', 'javascript:alert(1)', '' ) ),
	// The posts controller's term limits, and oneOf's other errors.
	array( $terms, array( array( '1', 2 ), '3, 4', '', array( 'terms' => array( '1' ) ), array( 'terms' => '5', 'include_children' => 'true', 'operator' => 'AND' ), array( 'terms' => array( 'x' ) ), array( 'bogus' => 1 ), array( 'operator' => 'NOT' ), 'x', 5, null, array( 'a', 'b' ) ) ),
	array( array( 'oneOf' => array( array( 'type' => 'integer' ), array( 'type' => 'string' ) ) ), array( 5, '5', 'x', null ) ),
	array( array( 'oneOf' => array( array( 'type' => 'object', 'properties' => array( 'a' => array( 'type' => 'integer' ) ) ), array( 'type' => 'object', 'properties' => array( 'a' => array( 'type' => 'integer' ), 'b' => array( 'type' => 'integer' ) ) ) ) ), array( array( 'a' => 'x', 'b' => 'y' ), array( 'c' => 1 ) ) ),
	array( array( 'oneOf' => array( array( 'type' => 'object', 'properties' => array( 'a' => array( 'type' => 'integer' ) ) ), array( 'type' => 'object', 'additionalProperties' => false ) ) ), array( array( 'b' => 'y' ) ) ),
	array( array( 'type' => 'string', 'oneOf' => array( array( 'minLength' => 3 ) ) ), array( 'ab', 'abc' ) ),
	array( array( 'type' => 'string', 'oneOf' => array( array( 'title' => 'Long', 'minLength' => 5 ), array( 'title' => 'Short', 'maxLength' => 1 ), array( 'title' => 'Odd', 'enum' => array( 'z' ) ) ) ), array( 'abc', 'z' ) ),
	array( array( 'type' => 'string', 'oneOf' => array( array( 'title' => 'Long', 'minLength' => 5 ), array( 'maxLength' => 1 ) ) ), array( 'abc', 'abcdef' ) ),
	array( array( 'type' => 'array', 'oneOf' => array( array( 'items' => array( 'type' => 'integer' ), 'uniqueItems' => true ) ) ), array( array( '1', 2 ), array( '1', 1 ) ) ),
	array( array( 'oneOf' => array( array( 'type' => 'integer' ) ), 'minimum' => 5 ), array( 3, 7 ) ),
	array( array( 'type' => array( 'string', 'integer' ), 'oneOf' => array( array( 'type' => 'integer' ) ), 'maxLength' => 0 ), array( '5' ) ),
	array( array( 'oneOf' => array( array( 'type' => 'object', 'properties' => array( 'x' => array( 'type' => 'integer' ) ), 'required' => array( 'x' ) ), array( 'type' => 'object', 'properties' => array( 'y' => array( 'type' => 'integer' ) ), 'required' => array( 'y' ) ) ) ), array( array( 'c' => 1 ) ) ),
);
foreach ( $cases as list( $schema, $values ) ) {
	foreach ( $values as $value ) {
		$valid     = rest_validate_value_from_schema( $value, $schema, 'p' );
		$sanitized = rest_sanitize_value_from_schema( $value, $schema, 'p' );
		$error     = fn( $e ) => wp_json_encode( array( 'code' => $e->get_error_code(), 'message' => $e->get_error_message(), 'data' => $e->get_error_data() ) );
		echo bin2hex( wp_json_encode( $schema ) ), "\t", bin2hex( serialize( $value ) ), "\t",
			bin2hex( is_wp_error( $valid ) ? $error( $valid ) : var_export( $valid, true ) ), "\t",
			bin2hex( is_wp_error( $sanitized ) ? $error( $sanitized ) : serialize( $sanitized ) ), "\n";
	}
}
