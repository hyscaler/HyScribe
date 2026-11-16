<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Adds an EXIF segment and an IPTC segment to a JPEG, for the upload
// corpus (corpus.sh). The fields come as JSON:
//   php exif.php <in.jpg> <out.jpg> '<json>'
// {"exif": {"Orientation": 6, "Model": "Camera", ...}, "iptc": {"2#105": ["Headline"], ...}}
// EXIF strings are ASCII tags, rationals "n/d", ISOSpeedRatings and
// Orientation shorts, UserComment an ASCII comment and UndefinedComment
// one without a character code. A value starting "hex:" is given in hex.

[, $in, $out, $json] = $argv;
$spec = json_decode($json, true, 512, JSON_THROW_ON_ERROR);
$jpeg = file_get_contents($in);

const TAGS = [
	'ImageDescription'  => [0, 0x010E, 2],
	'Make'              => [0, 0x010F, 2],
	'Model'             => [0, 0x0110, 2],
	'Orientation'       => [0, 0x0112, 3],
	'Artist'            => [0, 0x013B, 2],
	'Copyright'         => [0, 0x8298, 2],
	'ExposureTime'      => [1, 0x829A, 5],
	'FNumber'           => [1, 0x829D, 5],
	'ISOSpeedRatings'   => [1, 0x8827, 3],
	'DateTimeDigitized' => [1, 0x9004, 2],
	'FocalLength'       => [1, 0x920A, 5],
	'UserComment'       => [1, 0x9286, 7],
	'UndefinedComment'  => [1, 0x9286, 8],
];

// One entry's count and value bytes.
function value(int $type, $value): array {
	switch ($type) {
		case 2:
			return [strlen($value) + 1, $value . "\0"];
		case 3:
			return [1, pack('n', $value) . "\0\0"];
		case 5:
			[$n, $d] = explode('/', $value);
			return [1, pack('NN', $n, $d)];
		case 7:
			$bytes = "ASCII\0\0\0" . $value;
			return [strlen($bytes), $bytes];
		default:
			$bytes = str_repeat("\0", 8) . $value;
			return [strlen($bytes), $bytes];
	}
}

// An IFD at offset $at of the TIFF data, its values after it.
function ifd(array $entries, int $at): string {
	ksort($entries);
	$head = pack('n', count($entries));
	$data = '';
	$data_at = $at + 2 + 12 * count($entries) + 4;
	foreach ($entries as $tag => [$type, $count, $bytes]) {
		$head .= pack('nnN', $tag, $type, $count);
		if (strlen($bytes) <= 4) {
			$head .= str_pad($bytes, 4, "\0");
		} else {
			$head .= pack('N', $data_at + strlen($data));
			$data .= $bytes . (strlen($bytes) % 2 ? "\0" : '');
		}
	}
	return $head . pack('N', 0) . $data;
}

$segments = '';
if (!empty($spec['exif'])) {
	$ifds = [[], []];
	foreach ($spec['exif'] as $name => $value) {
		[$which, $tag, $type] = TAGS[$name];
		[$count, $bytes] = value($type, $value);
		$ifds[$which][$tag] = [min($type, 7), $count, $bytes];
	}
	$exif_ifd = '';
	if ($ifds[1]) {
		$ifds[0][0x8769] = [4, 1, pack('N', 0)];
		$size0 = strlen(ifd($ifds[0], 8));
		$ifds[0][0x8769] = [4, 1, pack('N', 8 + $size0)];
		$exif_ifd = ifd($ifds[1], 8 + $size0);
	}
	$tiff = "MM\0*" . pack('N', 8) . ifd($ifds[0], 8) . $exif_ifd;
	$body = "Exif\0\0" . $tiff;
	$segments .= "\xFF\xE1" . pack('n', strlen($body) + 2) . $body;
}
// After SOI and the JFIF APP0, as cameras write them.
$app0 = 2 + 2 + unpack('n', substr($jpeg, 4, 2))[1];
$jpeg = substr($jpeg, 0, $app0) . $segments . substr($jpeg, $app0);
if (!empty($spec['iptc'])) {
	$iim = '';
	foreach ($spec['iptc'] as $key => $values) {
		[$record, $dataset] = array_map('intval', explode('#', $key));
		foreach ($values as $value) {
			$value = str_starts_with($value, 'hex:') ? hex2bin(substr($value, 4)) : $value;
			$iim .= pack('CCCn', 0x1C, $record, $dataset, strlen($value)) . $value;
		}
	}
	file_put_contents($out, $jpeg);
	$jpeg = iptcembed($iim, $out);
}
file_put_contents($out, $jpeg);
