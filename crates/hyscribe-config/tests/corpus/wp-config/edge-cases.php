<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
// Hand-written edge cases for the wp-config.php reader. PHP must run this
// file without a fatal error; reference/wpconfig/golden.sh records what it
// defines.

# Hash comment.
/* Block comment with define('IN_COMMENT', 1); inside. */
#[EdgeCase]
function edge_attribute() {}
define( 'DB_NAME', 'edge' );
DEFINE( 'UPPER_DEFINE', TRUE );
define( 'LOWER_FALSE', false );
define( 'A_NULL', NULL );
define( 'AN_INT', 42 );
define( 'NEGATIVE', -7 );
define( 'POSITIVE', +7 );
define( 'HEX', 0x1F );
define( 'BINARY', 0b101 );
define( 'OCTAL', 0o17 );
define( 'OLD_OCTAL', 017 );
define( 'UNDERSCORES', 1_000_000 );
define( 'A_FLOAT', 1.5 );
define( 'EXPONENT', 1e3 );
define( 'TOO_BIG', 9223372036854775808 );
define( 'SINGLE', 'it\'s a \\ back\slash' );
define( 'DOUBLE', "tab\tnew\nline \\ \$ \" \e\f\v\r" );
define( 'DOUBLE_CODES', "\101\x42\u{43}\u{1F600} \400 \xZ \q" );
$suffix = 'fix';
define( 'INTERPOLATED', "pre$suffix" );
define( 'CONCATENATED', 'a' . 'b' );
define( 'FROM_CONSTANT', PHP_INT_SIZE );
define( 'WITH_ARRAY', array( 1, 2 ) );
define( 'SHORT_ARRAY', [ 1, 2 ] );
define( 'HEREDOC', <<<EOT
  heredoc ; ' " text
  EOT );
define( 'NOWDOC', <<<'EOT'
nowdoc text
EOT
);
define( 'DUPLICATE', 'first' );
define( 'DUPLICATE', 'second' );
define( 'CASE_ARG', 'value', true );
defined( 'OR_DEFINED' ) || define( 'OR_DEFINED', 'yes' );
if ( ! defined( 'IN_BLOCK' ) ) {
	define( 'IN_BLOCK', 'block' );
}
if ( false ) {
	define( 'NEVER', 'never' );
}
if ( true ) define( 'NO_BRACES', 'yes' );
define( 'DB_HOST', 'localhost:/run/mysqld/mysqld.sock' );
$table_prefix = 'first_';
$table_prefix = "edge_";
if ( $table_prefix == 'x' ) {
	define( 'NOT_X', 1 );
}
// A one-line comment ends at the close tag ?> inline <?php
define( 'AFTER_TAG', 'yes' );
?>
Inline HTML with define('IN_HTML', 1); in it.
<?PHP
define( 'UPPER_OPEN_TAG', 'yes' );
define( 'ÜNICODE_NAME', 'ü' );
