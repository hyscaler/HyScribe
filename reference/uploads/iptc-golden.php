<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The IPTC reader's oracle, plain PHP 8.4:
//   php reference/uploads/iptc-golden.php > crates/hyscribe-image/tests/golden/iptc.tsv
// For each buffer, in hex, what iptcparse() gives: `false`, or each key
// with its values in hex, `key=hex,hex;key=...`.

$dataset = fn(int $record, int $set, string $value) => pack('CCCn', 0x1C, $record, $set, strlen($value)) . $value;
$extended = fn(int $record, int $set, string $value) => pack('CCCnN', 0x1C, $record, $set, 0x8004, strlen($value)) . $value;
$photoshop = fn(string $iim) => "Photoshop 3.0\x008BIM\x04\x04\0\0" . pack('N', strlen($iim)) . $iim;
$bases = [
	$photoshop($dataset(2, 105, 'Headline') . $dataset(2, 25, 'one') . $dataset(2, 25, 'two') . $dataset(2, 120, "Caption\xE9")),
	$photoshop($dataset(1, 90, "\x1B%G") . $dataset(2, 0, "\0\4") . $dataset(2, 80, 'By') . $extended(2, 120, str_repeat('x', 40))),
	$dataset(3, 1, 'record three first') . $dataset(2, 5, 'Object'),
	"\x1C\x02" . 'no room',
	'',
	"junk\x1C\x07\x05\x00\x01a" . $dataset(2, 5, 'after'),
	$dataset(2, 116, '') . $dataset(2, 116, 'second'),
	$dataset(2, 10, 'abc') . "\x1C\x02\x0A\x00\x10short",
	$dataset(2, 10, 'abc') . "\x1C\x02\x0A\x80\x04\x00\x00\x00",
	$dataset(2, 10, 'abc') . "\x1C\x02\x0A\x80\x04\xFF\xFF\xFF\xFFxyz",
	$dataset(2, 10, 'abc') . "\x1D\x02\x0A\x00\x01q",
	$extended(2, 55, '') ,
];
$cases = $bases;
mt_srand(31);
foreach ($bases as $base) {
	for ($cut = 0; $cut < strlen($base); $cut += 3) {
		$cases[] = substr($base, 0, $cut);
	}
	for ($i = 0; $i < 40 && strlen($base) > 0; $i++) {
		$mutated = $base;
		$mutated[mt_rand(0, strlen($base) - 1)] = chr(mt_rand(0, 255));
		$cases[] = $mutated;
	}
}
foreach ($cases as $case) {
	$parsed = @iptcparse($case);
	$out = 'false';
	if (is_array($parsed)) {
		$out = implode(';', array_map(fn($key) => $key . '=' . implode(',', array_map('bin2hex', $parsed[$key])), array_keys($parsed)));
	}
	echo bin2hex($case), "\t", $out, "\n";
}
