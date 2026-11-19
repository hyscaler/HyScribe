-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Rows reference/rest/golden.sh adds for reference/rest/media-writes.list and deletes after
-- with the statements below `-- cleanup`, in mysqldump's form so the tests read them too:
-- four attachments with two sizes, a backup of the original and an alt text each, the
-- administrator's image on post 1 (its featured image, with a comment on it), the author's
-- loose one, the administrator's in the trash and a private one on page 2.
INSERT INTO `wp_posts` VALUES (9301,1,'2026-11-10 09:00:00','2026-11-10 09:00:00','Its description','Oracle picture','Its caption','inherit','open','closed','','oracle-picture','','','2026-11-10 09:00:00','2026-11-10 09:00:00','',1,'http://127.0.0.1:8081/?attachment_id=9301',0,'attachment','image/jpeg',0),
(9302,702,'2026-11-10 09:00:00','2026-11-10 09:00:00','Its description','Author picture','Its caption','inherit','open','closed','','author-picture','','','2026-11-10 09:00:00','2026-11-10 09:00:00','',0,'http://127.0.0.1:8081/?attachment_id=9302',0,'attachment','image/jpeg',0),
(9303,1,'2026-11-10 09:00:00','2026-11-10 09:00:00','Its description','Trashed picture','Its caption','trash','open','closed','','trashed-picture__trashed','','','2026-11-10 09:00:00','2026-11-10 09:00:00','',0,'http://127.0.0.1:8081/?attachment_id=9303',0,'attachment','image/jpeg',0),
(9304,1,'2026-11-10 09:00:00','2026-11-10 09:00:00','Its description','Private picture','Its caption','private','open','closed','','private-picture','','','2026-11-10 09:00:00','2026-11-10 09:00:00','',2,'http://127.0.0.1:8081/?attachment_id=9304',0,'attachment','image/jpeg',0);
INSERT INTO `wp_postmeta` VALUES (9301,9301,'_wp_attached_file','2026/11/oracle-one.jpg'),
(9302,9301,'_wp_attachment_metadata','a:6:{s:5:"width";i:1200;s:6:"height";i:800;s:4:"file";s:22:"2026/11/oracle-one.jpg";s:8:"filesize";i:345678;s:5:"sizes";a:2:{s:6:"medium";a:5:{s:4:"file";s:22:"oracle-one-300x200.jpg";s:5:"width";i:300;s:6:"height";i:200;s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:12345;}s:9:"thumbnail";a:5:{s:4:"file";s:22:"oracle-one-150x150.jpg";s:5:"width";i:150;s:6:"height";i:150;s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:5432;}}s:10:"image_meta";a:12:{s:8:"aperture";s:1:"0";s:6:"credit";s:0:"";s:6:"camera";s:0:"";s:7:"caption";s:0:"";s:17:"created_timestamp";s:1:"0";s:9:"copyright";s:0:"";s:12:"focal_length";s:1:"0";s:3:"iso";s:1:"0";s:13:"shutter_speed";s:1:"0";s:5:"title";s:0:"";s:11:"orientation";s:1:"0";s:8:"keywords";a:0:{}}}'),
(9303,9301,'_wp_attachment_image_alt','Old alt'),
(9304,9301,'_wp_attachment_backup_sizes','a:1:{s:9:"full-orig";a:3:{s:5:"width";i:1200;s:6:"height";i:800;s:4:"file";s:19:"oracle-one-orig.jpg";}}'),
(9305,9302,'_wp_attached_file','2026/11/oracle-two.jpg'),
(9306,9302,'_wp_attachment_metadata','a:6:{s:5:"width";i:1200;s:6:"height";i:800;s:4:"file";s:22:"2026/11/oracle-two.jpg";s:8:"filesize";i:345678;s:5:"sizes";a:2:{s:6:"medium";a:5:{s:4:"file";s:22:"oracle-two-300x200.jpg";s:5:"width";i:300;s:6:"height";i:200;s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:12345;}s:9:"thumbnail";a:5:{s:4:"file";s:22:"oracle-two-150x150.jpg";s:5:"width";i:150;s:6:"height";i:150;s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:5432;}}s:10:"image_meta";a:12:{s:8:"aperture";s:1:"0";s:6:"credit";s:0:"";s:6:"camera";s:0:"";s:7:"caption";s:0:"";s:17:"created_timestamp";s:1:"0";s:9:"copyright";s:0:"";s:12:"focal_length";s:1:"0";s:3:"iso";s:1:"0";s:13:"shutter_speed";s:1:"0";s:5:"title";s:0:"";s:11:"orientation";s:1:"0";s:8:"keywords";a:0:{}}}'),
(9307,9302,'_wp_attachment_image_alt','Old alt'),
(9308,9302,'_wp_attachment_backup_sizes','a:1:{s:9:"full-orig";a:3:{s:5:"width";i:1200;s:6:"height";i:800;s:4:"file";s:19:"oracle-two-orig.jpg";}}'),
(9309,9303,'_wp_attached_file','2026/11/oracle-three.jpg'),
(9310,9303,'_wp_attachment_metadata','a:6:{s:5:"width";i:1200;s:6:"height";i:800;s:4:"file";s:24:"2026/11/oracle-three.jpg";s:8:"filesize";i:345678;s:5:"sizes";a:2:{s:6:"medium";a:5:{s:4:"file";s:24:"oracle-three-300x200.jpg";s:5:"width";i:300;s:6:"height";i:200;s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:12345;}s:9:"thumbnail";a:5:{s:4:"file";s:24:"oracle-three-150x150.jpg";s:5:"width";i:150;s:6:"height";i:150;s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:5432;}}s:10:"image_meta";a:12:{s:8:"aperture";s:1:"0";s:6:"credit";s:0:"";s:6:"camera";s:0:"";s:7:"caption";s:0:"";s:17:"created_timestamp";s:1:"0";s:9:"copyright";s:0:"";s:12:"focal_length";s:1:"0";s:3:"iso";s:1:"0";s:13:"shutter_speed";s:1:"0";s:5:"title";s:0:"";s:11:"orientation";s:1:"0";s:8:"keywords";a:0:{}}}'),
(9311,9303,'_wp_attachment_image_alt','Old alt'),
(9312,9303,'_wp_attachment_backup_sizes','a:1:{s:9:"full-orig";a:3:{s:5:"width";i:1200;s:6:"height";i:800;s:4:"file";s:21:"oracle-three-orig.jpg";}}'),
(9313,9303,'_wp_trash_meta_status','inherit'),
(9314,9303,'_wp_trash_meta_time','1790000000'),
(9315,9304,'_wp_attached_file','2026/11/oracle-four.jpg'),
(9316,9304,'_wp_attachment_metadata','a:6:{s:5:"width";i:1200;s:6:"height";i:800;s:4:"file";s:23:"2026/11/oracle-four.jpg";s:8:"filesize";i:345678;s:5:"sizes";a:2:{s:6:"medium";a:5:{s:4:"file";s:23:"oracle-four-300x200.jpg";s:5:"width";i:300;s:6:"height";i:200;s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:12345;}s:9:"thumbnail";a:5:{s:4:"file";s:23:"oracle-four-150x150.jpg";s:5:"width";i:150;s:6:"height";i:150;s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:5432;}}s:10:"image_meta";a:12:{s:8:"aperture";s:1:"0";s:6:"credit";s:0:"";s:6:"camera";s:0:"";s:7:"caption";s:0:"";s:17:"created_timestamp";s:1:"0";s:9:"copyright";s:0:"";s:12:"focal_length";s:1:"0";s:3:"iso";s:1:"0";s:13:"shutter_speed";s:1:"0";s:5:"title";s:0:"";s:11:"orientation";s:1:"0";s:8:"keywords";a:0:{}}}'),
(9317,9304,'_wp_attachment_image_alt','Old alt'),
(9318,9304,'_wp_attachment_backup_sizes','a:1:{s:9:"full-orig";a:3:{s:5:"width";i:1200;s:6:"height";i:800;s:4:"file";s:20:"oracle-four-orig.jpg";}}'),
(9319,1,'_thumbnail_id','9301');
INSERT INTO `wp_comments` VALUES (9301,9301,'A reader','','','127.0.0.1','2026-11-11 09:00:00','2026-11-11 09:00:00','A comment.',0,'1','','comment',0,0);
-- cleanup
DELETE FROM wp_posts WHERE ID BETWEEN 9301 AND 9399;
DELETE FROM wp_postmeta WHERE meta_id BETWEEN 9301 AND 9399;
DELETE FROM wp_comments WHERE comment_ID BETWEEN 9301 AND 9399;
