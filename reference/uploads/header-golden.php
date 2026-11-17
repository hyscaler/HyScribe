<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The header reader's oracle, plain PHP 8.4:
//   php reference/uploads/header-golden.php > crates/hyscribe-image/tests/golden/header.tsv
// For each case, a corpus file changed by a recipe or bytes given in hex,
// what exif_imagetype() and getimagesize( $file, $info ) say: the type's
// MIME type, width, height, type, and the MD5 of $info['APP13'].
// Recipes: `cut:N` keeps the first N bytes, `set:AT:BYTE` sets one byte.

$dir = dirname(__DIR__, 2) . '/crates/hyscribe-image/tests/corpus';
$cases = [];
foreach (glob("$dir/*") as $path) {
	$name = basename($path);
	$size = filesize($path);
	$cases[] = [$name, ''];
	foreach ([0, 1, 2, 3, 8, 12, 16, 20, 24, 26, 28, 30, 40, 64, 100, 200, 400, 1000] as $cut) {
		if ($cut < $size) {
			$cases[] = [$name, "cut:$cut"];
		}
	}
	mt_srand(crc32($name));
	for ($i = 0; $i < 24; $i++) {
		$at = mt_rand(0, min($size, 600) - 1);
		$cases[] = [$name, "set:$at:" . mt_rand(0, 255)];
	}
}
// Hand made headers.
$riff = fn(string $chunk) => 'RIFF' . pack('V', 4 + strlen($chunk)) . 'WEBP' . $chunk;
$box = fn(string $type, string $body) => pack('N', 8 + strlen($body)) . $type . $body;
$full = fn(string $type, int $version, int $flags, string $body) => $box($type, pack('N', $version << 24 | $flags) . $body);
$ftyp = fn(string $major, string ...$compatible) => $box('ftyp', $major . "\0\0\0\0" . implode('', $compatible));
$ispe = fn(int $w, int $h) => $full('ispe', 0, 0, pack('NN', $w, $h));
$infe = fn(int $id) => $full('infe', 2, 0, pack('nn', $id, 0) . "av01\0");
$iinf = $full('iinf', 0, 0, pack('n', 3) . $infe(1) . $infe(2) . $infe(3));
$meta = fn(string $pitm, string $ipco, string $ipma) => $full('meta', 0, 0, $full('hdlr', 0, 0, "\0\0\0\0pict" . str_repeat("\0", 13)) . $pitm . $iinf . $box('iprp', $box('ipco', $ipco) . $ipma));
$pixi = $full('pixi', 0, 0, "\x03\x08\x08\x08");
$one = fn(int $item, int ...$props) => $full('ipma', 0, 0, pack('Nn', 1, $item) . pack('C*', count($props), ...$props));
$heif = [
	'avif' => $ftyp('avif', 'mif1', 'avif') . $meta($full('pitm', 0, 0, pack('n', 1)), $ispe(640, 480) . $pixi, $one(1, 0x81, 2)),
	'avif-av1c' => $ftyp('avif') . $meta($full('pitm', 0, 0, pack('n', 1)), $box('av1C', "\x81\x00\x0c\x00") . $ispe(64, 32), $one(1, 1, 2)),
	'avif-no-depth' => $ftyp('avif') . $meta($full('pitm', 0, 0, pack('n', 1)), $ispe(640, 480), $one(1, 1)),
	'avis-compatible' => $ftyp('mif1', 'avis') . $meta($full('pitm', 0, 0, pack('n', 1)), $ispe(30, 20) . $pixi, $one(1, 1, 2)),
	'avif-pitm-v1' => $ftyp('avif') . $meta($full('pitm', 1, 0, pack('N', 2)), $ispe(10, 10) . $ispe(70, 50) . $pixi, $full('ipma', 1, 1, pack('NNCnnNCnn', 2, 1, 2, 1, 3, 2, 2, 0x8002, 3))),
	'avif-no-ispe' => $ftyp('avif') . $meta($full('pitm', 0, 0, pack('n', 1)), $pixi, $one(1, 1)),
	'avif-other-item' => $ftyp('avif') . $meta($full('pitm', 0, 0, pack('n', 3)), $ispe(64, 48) . $pixi, $one(1, 1, 2)),
	'avif-large-size' => $ftyp('avif') . pack('N', 1) . 'free' . pack('J', 20) . 'pad!' . $meta($full('pitm', 0, 0, pack('n', 1)), $ispe(9, 7) . $pixi, $one(1, 1, 2)),
	'avif-irot' => $ftyp('avif') . $meta($full('pitm', 0, 0, pack('n', 1)), $ispe(80, 40) . $box('irot', "\x01") . $pixi, $one(1, 1, 2, 3)),
	'avif-no-iinf' => $ftyp('avif') . $full('meta', 0, 0, $full('pitm', 0, 0, pack('n', 1)) . $box('iprp', $box('ipco', $ispe(12, 34) . $pixi) . $one(1, 1, 2))),
	'avif-meta-to-end' => $ftyp('avif') . substr_replace($meta($full('pitm', 0, 0, pack('n', 1)), $ispe(23, 45) . $pixi, $one(1, 1, 2)), "\0\0\0\0", 0, 4),
	'avif-meta-later' => $ftyp('avif') . $box('free', 'xxxx') . $meta($full('pitm', 0, 0, pack('n', 1)), $ispe(56, 78) . $pixi, $one(1, 1, 2)),
	'avif-zero-index' => $ftyp('avif') . $meta($full('pitm', 0, 0, pack('n', 1)), $ispe(56, 78) . $pixi, $one(1, 0, 1, 2)),
	'ftyp-other' => $ftyp('isom', 'mp41'),
	// PHP's getimagesize() does not read these; WordPress asks the editor.
	'heic' => $ftyp('heic', 'mif1') . $meta($full('pitm', 0, 0, pack('n', 1)), $ispe(80, 40), $one(1, 1)),
	'heic-irot' => $ftyp('heic', 'mif1') . $meta($full('pitm', 0, 0, pack('n', 1)), $ispe(80, 40) . $box('irot', "\x03"), $one(1, 1, 2)),
	'heif' => $ftyp('heif', 'mif1') . $meta($full('pitm', 0, 0, pack('n', 1)), $ispe(8, 4), $one(1, 1)),
	'heif-mif1' => $ftyp('mif1', 'heic'),
];
$hex = [
	'webp-vp8' => bin2hex($riff('VP8 ' . pack('V', 10) . "\x30\x01\x00\x9d\x01\x2a" . pack('vv', 0xC07B, 0x402D))),
	'webp-vp8l' => bin2hex($riff('VP8L' . pack('V', 5) . "\x2f\x7a\x00\x0b\x00")),
	'webp-vp8x' => bin2hex($riff('VP8X' . pack('V', 10) . "\x10\0\0\0" . "\x0f\x27\x00\x5f\x3d\x01")),
	'webp-other' => bin2hex($riff('ALPH' . pack('V', 10) . str_repeat("\1", 10))),
	'bmp-core' => bin2hex('BM' . str_repeat("\0", 12) . pack('V', 12) . pack('vvvv', 33, 22, 1, 24)),
	'bmp-core-padded' => bin2hex('BM' . str_repeat("\0", 12) . pack('V', 12) . pack('vvvv', 33, 22, 1, 24) . str_repeat("\0", 8)),
	'bmp-info' => bin2hex('BM' . str_repeat("\0", 12) . pack('V', 40) . pack('VV', 44, 0xFFFFFFF0) . pack('vv', 1, 24) . str_repeat("\0", 24)),
	'bmp-v5' => bin2hex('BM' . str_repeat("\0", 12) . pack('V', 124) . pack('VV', 5, 6) . str_repeat("\0", 120)),
	'bmp-odd' => bin2hex('BM' . str_repeat("\0", 12) . pack('V', 100) . pack('VV', 5, 6) . str_repeat("\0", 120)),
	'gif-zero' => bin2hex("GIF89a\0\0\0\0\0\0\0"),
	'jpeg-two-app13' => bin2hex("\xFF\xD8" . "\xFF\xED\x00\x05one" . "\xFF\xED\x00\x05two" . "\xFF\xC0\x00\x11\x08\x00\x10\x00\x20\x03" . str_repeat("\0", 9) . "\xFF\xDA"),
	'jpeg-app13-after-sof' => bin2hex("\xFF\xD8" . "\xFF\xC2\x00\x11\x08\x00\x10\x00\x20\x03" . str_repeat("\0", 9) . "\xFF\xFF\xED\x00\x06late" . "\xFF\xD9"),
	'jpeg-junk-between' => bin2hex("\xFF\xD8junk\xFF\xC4\x00\x04ab\xFF\xC1\x00\x11\x08\x00\x07\x00\x09\x03" . str_repeat("\0", 9)),
	'jpeg-two-sof' => bin2hex("\xFF\xD8\xFF\xC0\x00\x11\x08\x00\x01\x00\x02\x03" . str_repeat("\0", 9) . "\xFF\xC0\x00\x11\x08\x00\x03\x00\x04\x03" . str_repeat("\0", 9) . "\xFF\xDA"),
	'jpeg-short-length' => bin2hex("\xFF\xD8\xFF\xE0\x00\x01\xFF\xC0\x00\x11\x08\x00\x05\x00\x06\x03" . str_repeat("\0", 9) . "\xFF\xDA"),
	'jpeg-sof-cut' => bin2hex("\xFF\xD8\xFF\xC0\x00\x11\x08\x00\x05"),
	'jpeg-no-sof' => bin2hex("\xFF\xD8\xFF\xE1\x00\x04ab\xFF\xD9"),
	'jpeg-empty-app13' => bin2hex("\xFF\xD8\xFF\xED\x00\x02\xFF\xC0\x00\x11\x08\x00\x05\x00\x06\x03" . str_repeat("\0", 9) . "\xFF\xDA"),
	'jpeg-cut-app13' => bin2hex("\xFF\xD8\xFF\xC0\x00\x11\x08\x00\x05\x00\x06\x03" . str_repeat("\0", 9) . "\xFF\xED\x00\x40abc"),
	'tiff-mm' => bin2hex("MM\0*\0\0\0\x08\0\0"),
	'psd' => bin2hex('8BPS' . str_repeat("\0", 30)),
	'empty' => '',
];
foreach ($heif as $name => $bytes) {
	$hex[$name] = bin2hex($bytes);
}
foreach ($hex as $name => $bytes) {
	$cases[] = ["hex:$name", $bytes];
}
// Every prefix of the hand made ISO base media files, as the box and
// property readers meet them. (HyScribe checks fewer of libavifinfo's
// rules on a whole file: a malformed AVIF it reads may be one PHP refuses.)
foreach ($heif as $name => $bytes) {
	for ($cut = 1; $cut < strlen($bytes); $cut++) {
		$cases[] = ["hex:$name/$cut", bin2hex(substr($bytes, 0, $cut))];
	}
}

$tmp = tempnam(sys_get_temp_dir(), 'hs-header');
echo "# case\trecipe\tmime\twidth\theight\ttype\tapp13 md5\n";
foreach ($cases as [$name, $recipe]) {
	if (str_starts_with($name, 'hex:')) {
		$bytes = hex2bin($recipe);
	} else {
		$bytes = file_get_contents("$dir/$name");
		if (str_starts_with($recipe, 'cut:')) {
			$bytes = substr($bytes, 0, (int) substr($recipe, 4));
		} elseif (str_starts_with($recipe, 'set:')) {
			[, $at, $byte] = explode(':', $recipe);
			$bytes[(int) $at] = chr((int) $byte);
		}
	}
	file_put_contents($tmp, $bytes);
	$type = @exif_imagetype($tmp);
	$mime = $type ? image_type_to_mime_type($type) : '-';
	$info = [];
	$size = @getimagesize($tmp, $info);
	$app13 = isset($info['APP13']) ? md5($info['APP13']) : '-';
	$fields = $size ? [$size[0], $size[1], $size[2]] : ['-', '-', '-'];
	echo implode("\t", [$name, $recipe, $mime, ...$fields, $app13]), "\n";
}
unlink($tmp);
