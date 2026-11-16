-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- What reference/rest/golden.sh adds for index-site.list and takes out
-- after: a 512 pixel site icon, a logo set as the classic theme mod, and a
-- time zone without daylight saving time.
DELETE FROM wp_options WHERE option_name IN ('site_icon', 'timezone_string', 'theme_mods_twentytwentyfive');
INSERT INTO `wp_options` VALUES (9901,'site_icon','951','on'),(9902,'timezone_string','Asia/Kolkata','on'),(9903,'theme_mods_twentytwentyfive','a:1:{s:11:\"custom_logo\";i:952;}','auto');
INSERT INTO `wp_posts` VALUES (951,1,'2026-11-01 10:00:00','2026-11-01 10:00:00','','icon','','inherit','open','closed','','icon','','','2026-11-01 10:00:00','2026-11-01 10:00:00','',0,'http://127.0.0.1:8081/wp-content/uploads/2026/11/icon.png',0,'attachment','image/png',0),(952,1,'2026-11-01 10:00:00','2026-11-01 10:00:00','','logo','','inherit','open','closed','','logo','','','2026-11-01 10:00:00','2026-11-01 10:00:00','',0,'http://127.0.0.1:8081/wp-content/uploads/2026/11/logo.png',0,'attachment','image/png',0);
INSERT INTO `wp_postmeta` VALUES (951,951,'_wp_attached_file','2026/11/icon.png'),(952,951,'_wp_attachment_metadata','a:5:{s:5:\"width\";i:512;s:6:\"height\";i:512;s:4:\"file\";s:16:\"2026/11/icon.png\";s:5:\"sizes\";a:1:{s:9:\"thumbnail\";a:4:{s:4:\"file\";s:16:\"icon-150x150.png\";s:5:\"width\";i:150;s:6:\"height\";i:150;s:9:\"mime-type\";s:9:\"image/png\";}}s:10:\"image_meta\";a:0:{}}'),(953,952,'_wp_attached_file','2026/11/logo.png'),(954,952,'_wp_attachment_metadata','a:5:{s:5:\"width\";i:200;s:6:\"height\";i:100;s:4:\"file\";s:16:\"2026/11/logo.png\";s:5:\"sizes\";a:0:{}s:10:\"image_meta\";a:0:{}}');
-- cleanup
DELETE FROM wp_options WHERE option_id BETWEEN 9901 AND 9903;
INSERT INTO wp_options (option_id, option_name, option_value, autoload) VALUES (81, 'timezone_string', '', 'on'), (87, 'site_icon', '0', 'on');
DELETE FROM wp_postmeta WHERE post_id BETWEEN 951 AND 952;
DELETE FROM wp_posts WHERE ID BETWEEN 951 AND 952;
