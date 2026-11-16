-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- What reference/rest/golden.sh adds for zone.list and takes out after: a
-- named time zone over the stored `gmt_offset` of 0, and a draft whose
-- GMT dates are unset, in mysqldump's form so the tests read them too.
DELETE FROM wp_options WHERE option_name = 'timezone_string';
INSERT INTO `wp_options` VALUES (9911,'timezone_string','Asia/Kolkata','on');
INSERT INTO `wp_posts` VALUES (961,1,'2026-11-01 10:00:00','0000-00-00 00:00:00','','A zoned draft','','draft','open','open','','','','','2026-11-02 11:30:00','0000-00-00 00:00:00','',0,'http://127.0.0.1:8081/?p=961',0,'post','',0);
-- cleanup
DELETE FROM wp_options WHERE option_id = 9911;
INSERT INTO wp_options (option_id, option_name, option_value, autoload) VALUES (81, 'timezone_string', '', 'on');
DELETE FROM wp_posts WHERE ID = 961;
