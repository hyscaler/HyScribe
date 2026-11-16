<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-fetch: WordPress's wp_http_validate_url() and
// WP_Http::make_absolute_url(), from a WordPress 7.1.3 tree, with the home
// http://example.org:8081, filters at their defaults and gethostbyname()
// answered from the table below instead of DNS. Lines are
//   validate<TAB>hex(url)<TAB>0|1
//   absolute<TAB>hex(relative)<TAB>hex(base)<TAB>hex(result)
//   php golden.php <wordpress> > crates/hyscribe-fetch/tests/golden/urls.tsv

$root = rtrim( $argv[1], '/' ) . '/wp-includes/';
function hs_extract( $file, $name, $indent = '' ) {
	preg_match( '/\n' . $indent . '(?:public static )?function ' . $name . '\(.*?\n' . $indent . '}\n/s', file_get_contents( $file ), $found );
	return $found[0];
}
foreach ( array( 'wp_kses_bad_protocol', 'wp_kses_no_null', 'wp_kses_bad_protocol_once', 'wp_kses_bad_protocol_once2', 'wp_kses_decode_entities', '_wp_kses_decode_entities_chr', '_wp_kses_decode_entities_chr_hexdec' ) as $name ) {
	eval( hs_extract( $root . 'kses.php', $name ) );
}
foreach ( array( 'wp_parse_url', '_get_component_from_parsed_url_array', '_wp_translate_php_url_constant_to_key' ) as $name ) {
	eval( hs_extract( $root . 'http.php', $name ) );
}
eval( str_replace( 'gethostbyname(', 'hs_gethostbyname(', hs_extract( $root . 'http.php', 'wp_http_validate_url' ) ) );
eval( 'class WP_Http {' . hs_extract( $root . 'class-wp-http.php', 'make_absolute_url', '\t' ) . '}' );

function get_option( $name ) {
	return 'http://example.org:8081';
}
function apply_filters( $hook, $value ) {
	return $value;
}
function hs_gethostbyname( $host ) {
	$hosts = array(
		'public.test'   => '93.184.215.14',
		'private.test'  => '10.0.0.5',
		'metadata.test' => '169.254.169.254',
		'loopback.test' => '127.0.0.1',
	);
	return $hosts[ strtolower( $host ) ] ?? $host;
}

$urls = array(
	'', 'http://public.test/', 'https://public.test/a/b?c=d#e', 'HTTP://Public.Test/', 'http://public.test./x',
	'ftp://public.test/', 'javascript:alert(1)', '//public.test/', 'public.test', 'http:///x',
	'http://user@public.test/', 'http://user:pass@public.test/', 'http://:@public.test/',
	'http://private.test/', 'http://metadata.test/', 'http://loopback.test/', 'http://nowhere.test/',
	'http://10.0.0.1/', 'http://127.0.0.1/', 'http://169.254.169.254/latest/meta-data/', 'http://0.0.0.0/',
	'http://172.16.0.1/', 'http://172.32.0.1/', 'http://192.168.0.1/', 'http://100.64.0.1/', 'http://8.8.8.8/',
	'http://224.0.0.1/', 'http://255.255.255.255/', 'http://198.18.0.1/', 'http://203.0.113.9/',
	'http://[::1]/', 'http://[2606:4700::1111]/', 'http://public.test:80/', 'http://public.test:443/',
	'http://public.test:8080/', 'http://public.test:8081/', 'http://public.test:22/', 'http://public.test:0/',
	'http://example.org:8081/', 'http://example.org:9000/', 'http://EXAMPLE.org:8081/', 'http://example.org/',
	'http://public.test/a b', "http://public.test/\xc3\xa9", 'http://pub%6cic.test/', 'http://public.test#x',
	'http://public.test?x', 'http://public.test\\@private.test/', 'http://public.test/%0d%0a', '123',
	"http://public.test/\0", 'https://public.test:443/x', 'http://public.test:65536/', 'http://public.test:-1/',
	'http://exa_mple.test/', 'http://public.test:8080abc/',
);
foreach ( $urls as $url ) {
	echo "validate\t", bin2hex( $url ), "\t", false === wp_http_validate_url( $url ) ? 0 : 1, "\n";
}

$bases     = array( 'http://example.com/dir/page.html?q=1#f', 'https://example.com:8443', 'http://example.com/', '', '0', '//example.com/x/y', 'http://example.com/a/b/c/' );
$relatives = array(
	'/favicon.ico', 'favicon.ico', '../icons/a.png', '../../../../x.png', 'a/b/../../c/./d.png', '/a/b/../c.png',
	'?x=1', '#top', '?0', '#0', '0', '', '//cdn.example.com/a.jpg', '//cdn.example.com:8080/a.jpg', 'https://other.example/x',
	'data:image/png;base64,AAAA', 'mailto:x@example.com', 'x/../../y/../z', '..', '../', './', 'a/..b/../c',
	'/../a', '/../../a', 'img.png?v=1#frag', 'http:///bad', 'a//b/../c', 'a../b',
);
foreach ( $bases as $base ) {
	foreach ( $relatives as $relative ) {
		echo "absolute\t", bin2hex( $relative ), "\t", bin2hex( $base ), "\t", bin2hex( WP_Http::make_absolute_url( $relative, $base ) ), "\n";
	}
}
