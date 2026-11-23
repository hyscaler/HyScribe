<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// sanitize_title_for_query() of request slugs, as
//   hex(title)<TAB>hex(slug)
// for crates/hyscribe-render/tests/golden/sanitize-title-query.tsv. Needs
// the default install oracle (reference/default-install/php.sh up); only
// reads.

require '/var/www/html/wp-load.php';

$titles = array(
	'hello-world', 'Hello-World', 'hello world', 'Hello  World!', 'hello_world', 'hello.world', 'a/b', 'a%20b', 'a%zz', '%', '%e9', '%C3%A9',
	'a---ff---b', 'a---zz---b', '---4f------50---', '<b>bold</b> text', 'a&amp;b', 'a&nbsp;b', 'café', 'CAFÉ', 'naïve résumé', 'Ünïcode',
	'İstanbul', 'ΣΊΣΥΦΟΣ', 'Привет мир', '日本語のタイトル', 'emoji 😀 here', "tab\there", "new\nline", '--a--', '-', '',
	str_repeat( 'a', 210 ), str_repeat( 'é', 70 ), str_repeat( 'a', 198 ) . 'é', "\xff\xfe bad", "a\xc3", 'ß', 'ǅ', 'Ⅻ', '™ mark',
	'a—b', 'a–b', 'a’b', '“quoted”', 'x×y', '½', 'ﬁ', 'İi', 'ΑΒΓ σς', 'Ⓐ', '١٢٣', 'k%E2%80%93m', 'a---4z---b', 'a---z4---b',
	str_repeat( 'a', 195 ) . 'é', str_repeat( 'a', 194 ) . '日', str_repeat( 'a', 188 ) . '😀',
);
foreach ( $titles as $title ) {
	echo bin2hex( $title ), "\t", bin2hex( sanitize_title_for_query( $title ) ), "\n";
}
