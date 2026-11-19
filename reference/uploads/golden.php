<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Run by golden.sh inside the PHP container: for each corpus file in
// $argv[1] (copied into the uploads folder $argv[2] under 2026/11/), what
// WordPress makes of it. Nothing is written to the database: the
// attachment ID is 0, which update_attached_file() and
// wp_update_attachment_metadata() refuse. Prints, tab separated:
//   meta <file> <serialize( wp_create_image_subsizes() ) in hex>
//   read <file> <serialize( wp_read_image_metadata() ) in hex>
//   out  <file> <made file> <md5> <bytes>
//   check <corpus file> <upload name> <ext> <type> <proper_filename>

[, $corpus, $base] = $argv;
define('WP_USE_THEMES', false);
require '/var/www/html/wp-load.php';
require_once ABSPATH . 'wp-admin/includes/image.php';
require_once ABSPATH . 'wp-admin/includes/file.php';

add_filter('upload_dir', function ($dirs) use ($base) {
	$dirs['basedir'] = $base;
	$dirs['path'] = "$base/2026/11";
	$dirs['subdir'] = '/2026/11';
	return $dirs;
});
$dir = "$base/2026/11";
$names = array_map('basename', glob("$corpus/*"));
sort($names);
foreach ($names as $name) {
	// The corpus also holds the files that are not images (media-corpus.sh).
	if (!preg_match('/\.(jpe?g|png|gif|webp|avif|bmp|tiff?)$/', $name)) {
		continue;
	}
	$file = "$dir/$name";
	copy("$corpus/$name", $file);
	$before = scandir($dir);
	echo "read\t$name\t", bin2hex(serialize(wp_read_image_metadata($file))), "\n";
	$meta = wp_create_image_subsizes($file, 0);
	$meta = apply_filters('wp_generate_attachment_metadata', $meta, 0, 'create');
	echo "meta\t$name\t", bin2hex(serialize($meta)), "\n";
	foreach (array_diff(scandir($dir), $before) as $made) {
		echo "out\t$name\t$made\t", md5_file("$dir/$made"), "\t", filesize("$dir/$made"), "\n";
	}
}
// image_resize_dimensions() and wp_constrain_dimensions() over a grid.
$sides = [0, 1, 99, 100, 101, 150, 299, 300, 301, 640, 1024, 2560];
foreach ([[640, 480], [300, 300], [100, 2000], [1, 1], [0, 10]] as [$w, $h]) {
	foreach ($sides as $dw) {
		foreach ($sides as $dh) {
			$crops = [false, true, ['left', 'top'], ['right', 'bottom'], ['center', 'top']];
			foreach ($crops as $crop) {
				$dims = image_resize_dimensions($w, $h, $dw, $dh, $crop);
				echo "dims\t$w\t$h\t$dw\t$dh\t", json_encode($crop), "\t", $dims ? implode(',', $dims) : 'false', "\n";
			}
			echo "constrain\t$w\t$h\t$dw\t$dh\t", implode(',', wp_constrain_dimensions($w, $h, $dw, $dh)), "\n";
		}
	}
}
// sanitize_file_name() and wp_unique_filename(), in an empty folder and
// one holding these files.
$names = [
	'photo.jpg', 'Photo.JPG', 'photo-150x150.jpg', 'photo-scaled.jpg', 'photo-rotated.png', 'pic.png', 'pic-scaled.png',
	'upper.jpg', 'Upper.JPG', 'doc.jpg', 'doc.heic', 'doc.jpeg', 'img.jpg', 'img-1.jpg', 'a-1.jpg.jpg', 'my photo (1).jpg',
	"caf\xC3\xA9.jpg", '.jpg', 'noext', 'a.b.c.jpg', 'file.php.jpg', 'x.tar.gz', '-_.hidden.jpg', 'JPG', 'jpg', 'gz',
	'a..b...jpg', "tab\tname.png", "na\xC3\xAFve r\xC3\xA9sum\xC3\xA9.png", 'über file+name&x.jpg', 'a%20b.jpg',
	"smart\xE2\x80\x99quote.jpg", "a\xE2\x80\x83b.jpg", "bad\xFF name.jpg", "bad\xFF", 'v1.data2.jpg', 'x.exe.jpg',
	'x.html.jpg', 'x.mp3.jpg', 'x.abcdef.jpg', 'x.ab12.jpg', 'x.ab1.jpg', 'a - b  --  c.jpg', '0.jpg', 'video.MP4',
	'IMG-1.JPG', 'q.heic', 'q.heif', 'r.jpg', 's.HEIC', "\x81\x82 odd.jpg", "\x9D", "\xC3.png",
];
$folder = "$base/unique";
@mkdir($folder, 0777, true);
$files = [
	'photo.jpg', 'photo-1.jpg', 'photo-300x200.jpg', 'pic-scaled.png', 'Upper.JPG', 'doc.heic', 'doc.jpg', 'img-150x150.jpg',
	'a-1.jpg.jpg', 'noext', 'noext-1', 'img-1.jpg', 'IMG-1.jpg', 'q.jpg', 'r-scaled.heic', 's.jpg', 'video.mp4', '0-1.jpg',
];
$empty = "$base/empty";
@mkdir($empty, 0777, true);
foreach ($files as $file) {
	touch("$folder/$file");
}
foreach ($names as $name) {
	echo "name\t", bin2hex($name), "\t", bin2hex(sanitize_file_name($name)), "\t", wp_unique_filename($empty, $name), "\t", wp_unique_filename($folder, $name), "\n";
}
echo "files\t", implode(',', $files), "\n";
// The type checks of uploads named as their content or otherwise.
$renamed = [
	['photo.jpg', 'photo.jpg'], ['photo.jpg', 'photo.JPEG'], ['photo.jpg', 'photo.png'], ['photo.jpg', 'photo.gif'],
	['rgb.png', 'rgb.jpg'], ['rgb.png', 'rgb.webp'], ['still.gif', 'still.bmp'], ['lossy.webp', 'lossy.jpg'],
	['photo.avif', 'photo.avif'], ['photo.avif', 'photo.jpg'], ['photo.bmp', 'photo.bmp'], ['photo.tif', 'photo.tiff'],
	['photo.tif', 'photo.jpg'], ['photo.jpg', 'photo.tar.jpg'], ['photo.jpg', 'photo.heic'], ['photo.jpg', 'photo.heif'],
	['photo.jpg', 'photo.php'], ['photo.jpg', 'photo.svg'], ['photo.jpg', 'photo'], ['photo.jpg', 'photo.jpg.php'],
	['photo.jpg', 'photo.php.jpg'], ['photo.jpg', 'photo.ico'], ['photo.jpg', 'photo.jpe'], ['rgb.png', 'rgb.PnG'], ['photo.bmp', 'photo.jpg'],
];
// Files that are not images, under their own names and others.
$kinds = [
	'plain.txt' => ['txt', 'csv', 'vtt', 'ics', 'rtf', 'pdf', 'docx', 'mp3', 'mp4', 'jpg', 'zip', 'psd', 'doc'],
	'words.csv' => ['csv', 'txt', 'tsv', 'vtt', 'pdf'],
	'commas.txt' => ['txt', 'csv', 'rtx'],
	'captions.vtt' => ['vtt', 'txt', 'srt', 'csv'],
	'event.ics' => ['ics', 'txt', 'vtt'],
	'script.txt' => ['txt', 'csv'],
	'not-a-picture.bin' => ['jpg', 'png', 'txt', 'pdf', 'mp3', 'docx'],
	'random.bin' => ['jpg', 'txt', 'pdf', 'mp3', 'mp4', 'docx', 'psd', 'zip'],
	'one-byte.bin' => ['txt', 'mp3', 'pdf', 'jpg'],
	'sample.pdf' => ['pdf', 'doc', 'txt', 'mp3', 'jpg', 'docx', 'zip'],
	'sample.rtf' => ['rtf', 'txt', 'doc', 'pdf'],
	'sample.zip' => ['zip', 'docx', 'xlsx', 'odt', 'pdf', 'txt', 'mp3', 'psd', 'gz'],
	'sample.tar' => ['tar', 'zip', 'gz'],
	'sample.gz' => ['gz', 'gzip', 'zip', 'tar'],
	'sample.docx' => ['docx', 'xlsx', 'zip', 'doc', 'odt', 'pdf', 'docm', 'dotx', 'txt'],
	'sample.xlsx' => ['xlsx', 'docx', 'pptx', 'xlsm', 'zip'],
	'sample.odt' => ['odt', 'ods', 'docx', 'zip', 'pdf'],
	'sample.ods' => ['ods', 'odt', 'xlsx'],
	'sample.doc' => ['doc', 'xls', 'docx', 'pdf'],
	'sample.xls' => ['xls', 'doc', 'xlsx'],
	'cbr.mp3' => ['mp3', 'm4a', 'wav', 'ogg', 'mp4', 'txt', 'pdf', 'docx', 'jpg', 'aac'],
	'noxing.mp3' => ['mp3', 'wav'],
	'tagged.mp3' => ['mp3', 'mp4'],
	'tone.wav' => ['wav', 'mp3', 'ogg', 'mp4', 'txt', 'pdf'],
	'tone.flac' => ['flac', 'mp3', 'wav', 'mp4'],
	'tone.ogg' => ['ogg', 'oga', 'mp3', 'ogv', 'mp4'],
	'tone.m4a' => ['m4a', 'mp3', 'mp4', 'wav', 'm4b'],
	'tone.aac' => ['aac', 'mp3', 'm4a'],
	'tone.wma' => ['wma', 'wmv', 'mp3', 'asf', 'wax'],
	'tune.mid' => ['mid', 'midi', 'mp3', 'wav'],
	'clip.mp4' => ['mp4', 'm4v', 'mov', 'qt', 'webm', 'mp3', 'wav', 'txt', 'pdf', '3gp', 'avi', 'ogv'],
	'clip-audio.mp4' => ['mp4', 'mov', 'mp3'],
	'clip.mov' => ['mov', 'mp4', 'mp3'],
	'clip.avi' => ['avi', 'divx', 'mp4', 'mp3'],
	'clip.webm' => ['webm', 'mkv', 'mp4', 'mp3'],
	'clip.mkv' => ['mkv', 'webm', 'mka', 'mp4'],
	'clip.3gp' => ['3gp', '3gpp', '3g2', 'mp4', 'mp3'],
	'clip.wmv' => ['wmv', 'asf', 'wma', 'mp4'],
	'clip.flv' => ['flv', 'mp4', 'mp3'],
	'clip.mpg' => ['mpg', 'mpeg', 'mpe', 'mp4', 'mp3'],
];
foreach ($kinds as $name => $exts) {
	foreach ($exts as $ext) {
		$renamed[] = [$name, 'upload.' . $ext];
	}
}
foreach ($renamed as [$name, $upload]) {
	$checked = wp_check_filetype_and_ext("$corpus/$name", $upload);
	echo implode("\t", ['check', $name, $upload, var_export($checked['ext'], true), var_export($checked['type'], true), var_export($checked['proper_filename'], true)]), "\n";
}
