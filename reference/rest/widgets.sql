-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The default install's widgets as the first sidebars or widgets request
-- leaves them under a block theme, which registers no sidebars:
-- retrieve_widgets() moves the classic theme's sidebars' block widgets to
-- the inactive widgets. reference/rest/golden.sh adds this for
-- reference/rest/widgets.list, so its requests write nothing, and puts the
-- install's row back after with the statements below `-- cleanup`; its
-- rows are in mysqldump's form so the tests read them too.
DELETE FROM wp_options WHERE option_name = 'sidebars_widgets';
INSERT INTO `wp_options` VALUES (9911,'sidebars_widgets','a:2:{s:19:\"wp_inactive_widgets\";a:5:{i:0;s:7:\"block-2\";i:1;s:7:\"block-3\";i:2;s:7:\"block-4\";i:3;s:7:\"block-5\";i:4;s:7:\"block-6\";}s:13:\"array_version\";i:3;}','auto');
-- cleanup
DELETE FROM wp_options WHERE option_id = 9911;
INSERT INTO wp_options (option_id, option_name, option_value, autoload) VALUES (107,'sidebars_widgets','a:4:{s:19:\"wp_inactive_widgets\";a:0:{}s:9:\"sidebar-1\";a:3:{i:0;s:7:\"block-2\";i:1;s:7:\"block-3\";i:2;s:7:\"block-4\";}s:9:\"sidebar-2\";a:2:{i:0;s:7:\"block-5\";i:1;s:7:\"block-6\";}s:13:\"array_version\";i:3;}','auto');
