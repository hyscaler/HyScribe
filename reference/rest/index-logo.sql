-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- What reference/rest/golden.sh adds for index-logo.list and takes out
-- after: the Site Logo block's option, naming the first post, before a theme
-- mod's logo, and the sample page as the site icon.
DELETE FROM wp_options WHERE option_name IN ('site_icon', 'site_logo', 'theme_mods_twentytwentyfive');
INSERT INTO `wp_options` VALUES (9901,'site_icon','2','on'),(9902,'site_logo','1','auto'),(9903,'theme_mods_twentytwentyfive','a:1:{s:11:\"custom_logo\";i:952;}','auto');
-- cleanup
DELETE FROM wp_options WHERE option_id BETWEEN 9901 AND 9903;
INSERT INTO wp_options (option_id, option_name, option_value, autoload) VALUES (87, 'site_icon', '0', 'on');
