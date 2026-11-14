<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Writes crates/hyscribe-rest/tests/golden/fields.tsv: what
// rest_filter_response_fields() keeps of a response's data for each
// `_fields`, as reference/rest/golden.sh runs it in the PHP container. Each
// line is
//   hex(json data)<TAB>hex(serialize(_fields))<TAB>hex(json kept)
// Only reads.
require '/var/www/html/wp-load.php';

$list   = '[{"id":1,"title":{"rendered":"a","raw":"b"},"x":2},{"id":2,"title":"t"}]';
$object = '{"a":{"b":{"c":1,"d":2}},"e":3,"7":"seven"}';
$cases  = array(
	array( $list, array( 'id,title.rendered', 'title', 'title.rendered,title', 'title,title.rendered,id', '', ',', array( 'id', ' x ' ), 'title.rendered.deeper', 'nope' ) ),
	array( $object, array( 'a.b.c', 'a.b.c,e', 'a.b,a.b.c', 'e.x', 'a,7', array( 3 => 'e' ), 'a.b.c,a,e' ) ),
	array( '[]', array( 'a' ) ),
);
foreach ( $cases as list( $json, $all ) ) {
	foreach ( $all as $fields ) {
		$request = new WP_REST_Request( 'GET', '/' );
		$request->set_param( '_fields', $fields );
		$kept = rest_filter_response_fields( new WP_REST_Response( json_decode( $json, true ) ), null, $request )->get_data();
		echo bin2hex( $json ), "\t", bin2hex( serialize( $fields ) ), "\t", bin2hex( wp_json_encode( $kept ) ), "\n";
	}
}
