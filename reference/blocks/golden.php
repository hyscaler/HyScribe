<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-phpser's json_decode and hyscribe-render's block
// parser. Prints "hex(input)<TAB>hex(serialize(result))" lines:
//   php golden.php json   > crates/hyscribe-phpser/tests/golden/json-decode.tsv
//   php golden.php blocks <wordpress> > crates/hyscribe-render/tests/golden/parse-blocks.tsv
// Run through reference/blocks/golden.sh. Cases are generated with a fixed
// seed, so the output is stable.

mt_srand( 20261027 );

function emit( $input, $result ) {
	echo bin2hex( $input ), "\t", bin2hex( serialize( $result ) ), "\n";
}

function json_cases() {
	$cases = array(
		'0', '-0', '1', '-1', '01', '-01', '1.0', '-0.0', '0.5', '.5', '5.', '1e2', '1E+2', '1e-2', '1e', '1.e1',
		'+1', '--1', '-', '9223372036854775807', '9223372036854775808', '-9223372036854775808',
		'-9223372036854775809', '18446744073709551616', '1e400', '-1e400', '123.456e-789', '0.1', '1.5e308',
		'true', 'false', 'null', 'TRUE', 'True', 'nul', 'nulll', '',
		' 1 ', "\t\n\r1\r\n", '1 2', '"a"', '"a', '"\\"\\\\\\/\\b\\f\\n\\r\\t"', '"\\u00e9"', '"\\u00E9"',
		'"\\ud83d\\ude00"', '"\\ud83d"', '"\\ude00"', '"\\ud83d\\u0041"', '"\\ud83dx"', '"\\u0000"', '"\\u12"',
		'"\\uzzzz"', '"\\x"', '"\\', "\"\x01\"", "\"\x7f\"", "\"\xc3\xa9\"", "\"\xff\"", "\"\xc0\x80\"",
		"\"\xed\xa0\x80\"", "\"\xf4\x90\x80\x80\"", "\"\xf0\x9f\x98\x80\"", "\"\xe2\x82\"", "\"\xc3\"",
		'[]', '{}', '[1,]', '[,1]', '[1 2]', '{"a":1,}', '{"a" 1}', '{1:2}', '{"a":1', '[1', '[', '{', ']', '}',
		'[1,[2,[3]]]', '{"a":{"b":{"c":[]}}}', ' [ 1 , 2 ] ', '{ "a" : 1 , "b" : [ ] }',
		'{"0":"a","1":"b"}', '{"1":"a","0":"b"}', '{"-0":1}', '{"07":1}', '{"-5":1}', '{"5":1,"5":2}',
		'{"9223372036854775807":1}', '{"9223372036854775808":1}', '{"-9223372036854775808":1}',
		'{"-9223372036854775809":1}', '{"":1}', '{"a":1,"b":2,"a":3}', '{"1":1,"01":2,"1":3}', '{"-":1}',
		'{"1a":1}', '{" 1":1}', '{"\\u0031":1}', '{"a":true,"b":false,"c":null,"d":1.5,"e":"x"}',
		'["a","b"]', '[null,true]', '{"x":[{"y":{}}]}',
	);
	foreach ( array( 511, 512, 513 ) as $depth ) {
		$cases[] = str_repeat( '[', $depth ) . str_repeat( ']', $depth );
		$cases[] = str_repeat( '{"a":', $depth - 1 ) . '{}' . str_repeat( '}', $depth - 1 );
		$cases[] = str_repeat( '[', $depth - 1 ) . '{"a":1}' . str_repeat( ']', $depth - 1 );
	}
	$atoms = array( '1', '-2.5', '"s"', '"\\u00fc"', 'true', 'null', '[]', '{}', '"k"', ':', ',', '[', ']', '{', '}', ' ', '0', '"', '\\' );
	for ( $i = 0; $i < 600; $i++ ) {
		$doc = '';
		$n   = mt_rand( 1, 12 );
		for ( $j = 0; $j < $n; $j++ ) {
			$doc .= $atoms[ mt_rand( 0, count( $atoms ) - 1 ) ];
		}
		$cases[] = $doc;
	}
	return $cases;
}

function block_cases( $wordpress ) {
	$cases = array(
		'', 'plain text', '0',
		'<!-- wp:paragraph --><p>Hi</p><!-- /wp:paragraph -->',
		'<!-- wp:paragraph -->x',
		"<!-- wp:paragraph -->\n<p>a</p>\n<!-- /wp:paragraph -->\n\n<!-- wp:paragraph -->\n<p>b</p>\n<!-- /wp:paragraph -->",
		'before<!-- wp:a -->in<!-- /wp:a -->after',
		'<!-- wp:a {"x":1} -->y<!-- /wp:a -->',
		'<!-- wp:a {"x":"}"} /-->',
		'<!-- wp:a {"x":{"y":2}} /-->',
		'<!-- wp:a {"x":"-->"} /-->',
		'<!-- wp:a {bad json} /-->',
		'<!-- wp:a {"x":1}--> no space',
		'<!-- wp:a {"x":1} --',
		'<!-- wp:my-plugin/block-name {"a":[1,2]} /-->',
		'<!-- wp:a/b/c /-->',
		'<!-- wp:A /-->',
		'<!--wp:a /-->',
		"<!--\twp:a\t/-->",
		"<!--\x0bwp:a\x0b/-->",
		'<!-- wp:a -->1<!-- wp:b -->2<!-- wp:c /-->3<!-- /wp:b -->4<!-- /wp:a -->',
		'<!-- wp:a --><!-- wp:b --><!-- /wp:b --><!-- /wp:a -->',
		'<!-- wp:a -->0<!-- wp:b /-->0<!-- /wp:a -->',
		'<!-- /wp:a -->stray closer first',
		'x<!-- wp:a -->y<!-- /wp:a --><!-- /wp:b -->z',
		'<!-- wp:a -->open<!-- wp:b -->inner',
		'<!-- wp:a -->1<!-- wp:b -->2<!-- wp:c -->3',
		'<!-- /wp:a /-->',
		'<!-- /wp:a {"x":1} -->',
		'<!-- wp:a /--><!-- wp:b /-->',
		'<!-- wp:a {"a":"}}"} /-->',
		'<!-- wp:a {"a":"} -->"} -->x<!-- /wp:a -->',
		'<!-- wp:a {"a":1}} /-->',
		'<!-- wp:a {} /-->',
		'<!-- wp:a { } /-->',
		'<!-- wp:a {"0":"z"} /-->',
		'<!-- wp:a {"a":1,"a":2} /-->',
		'<!-- wp:a -->x<!-- /wp:z -->',
		'<!-- wp:a -- >',
		'<!-- wp:a {"x":1} {"y":2} /-->',
		'<!-- wp:a {"x":1}' . "\n" . ' /-->',
		'<!-- wp:a_b-c1 /-->',
		'<!-- wp:core/a /-->',
		'<!-- wp: a /-->',
		'<!-- wp:a  /-->',
		"<!-- wp:a {\"long\":\"" . str_repeat( 'x', 5000 ) . "\"} /-->",
	);
	// The default theme's templates, parts and patterns: real block markup.
	foreach ( array( 'templates', 'parts', 'patterns' ) as $folder ) {
		foreach ( array( 'html', 'php' ) as $extension ) {
			foreach ( glob( "$wordpress/wp-content/themes/twentytwentyfive/$folder/*.$extension" ) as $file ) {
				$cases[] = file_get_contents( $file );
			}
		}
	}
	$atoms = array(
		'<!-- wp:a -->', '<!-- /wp:a -->', '<!-- wp:b {"k":1} -->', '<!-- /wp:b -->', '<!-- wp:c /-->',
		'<!-- wp:d {"x":"}"} /-->', 'text', '0', ' ', "\n", '<!--', '-->', '{', '}', '<!-- wp:', '/', 'wp:e ',
		'<!-- wp:f {"a":', '"} -->', '<p>p</p>',
	);
	for ( $i = 0; $i < 1500; $i++ ) {
		$doc = '';
		$n   = mt_rand( 1, 10 );
		for ( $j = 0; $j < $n; $j++ ) {
			$doc .= $atoms[ mt_rand( 0, count( $atoms ) - 1 ) ];
		}
		$cases[] = $doc;
	}
	return $cases;
}

if ( 'json' === $argv[1] ) {
	foreach ( json_cases() as $case ) {
		emit( $case, json_decode( $case, true ) );
	}
} else {
	define( 'ABSPATH', rtrim( $argv[2], '/' ) . '/' );
	require ABSPATH . 'wp-includes/class-wp-block-parser.php';
	foreach ( block_cases( $argv[2] ) as $case ) {
		emit( $case, ( new WP_Block_Parser() )->parse( $case ) );
	}
}
