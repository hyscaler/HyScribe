-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The fixture child's global styles post, which PHP creates on an
-- administrator's first request when it is the active theme, added for
-- reference/rest/theme.list over the default install and deleted after with
-- the statements below `-- cleanup`, in mysqldump's form so the tests read
-- it too. The navigation menu PHP's fallback creates on php.sh's first page
-- view, not in the fixture, is taken out first.
DELETE FROM wp_posts WHERE ID = 4 AND post_type = 'wp_navigation';
INSERT INTO `wp_terms` VALUES (901,'hyscribe-fixture-child','hyscribe-fixture-child',0);
INSERT INTO `wp_term_taxonomy` VALUES (901,901,'wp_theme','',0,1);
INSERT INTO `wp_posts` VALUES (901,1,'2026-11-02 10:00:00','2026-11-02 10:00:00','{\"version\": 3, \"isGlobalStylesUserThemeJSON\": true }','Custom Styles','','publish','closed','closed','','wp-global-styles-hyscribe-fixture-child','','','2026-11-02 10:00:00','2026-11-02 10:00:00','',0,'http://127.0.0.1:8081/?p=901',0,'wp_global_styles','',0);
INSERT INTO `wp_term_relationships` VALUES (901,901,0);
-- cleanup
DELETE FROM wp_posts WHERE ID = 901;
DELETE FROM wp_term_relationships WHERE object_id = 901;
DELETE FROM wp_term_taxonomy WHERE term_taxonomy_id = 901;
DELETE FROM wp_terms WHERE term_id = 901;
