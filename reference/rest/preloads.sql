-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The active theme's global styles post that reference/rest/golden.sh adds
-- for reference/rest/preloads.list over the default install, as
-- get_user_global_styles_post_id() creates it on the block editor's first
-- view, and deletes after with the statements below `-- cleanup`, in
-- mysqldump's form so the tests read them too; beside it a newer draft of
-- the same theme and a newer published one of another theme, which the
-- themes route's links pass over. The navigation menu PHP's fallback
-- creates on php.sh's first page view, not in the fixture, is taken
-- out first; db.sh resets the database after.
DELETE FROM wp_posts WHERE ID = 4 AND post_type = 'wp_navigation';
INSERT INTO `wp_terms` VALUES (901,'twentytwentyfive','twentytwentyfive',0),(902,'twentytwentyfour','twentytwentyfour',0);
INSERT INTO `wp_term_taxonomy` VALUES (901,901,'wp_theme','',0,2),(902,902,'wp_theme','',0,1);
INSERT INTO `wp_posts` VALUES (901,1,'2026-11-02 10:00:00','2026-11-02 10:00:00','{\"version\": 3, \"isGlobalStylesUserThemeJSON\": true }','Custom Styles','','publish','closed','closed','','wp-global-styles-twentytwentyfive','','','2026-11-02 10:00:00','2026-11-02 10:00:00','',0,'http://127.0.0.1:8081/?p=901',0,'wp_global_styles','',0),(902,1,'2026-11-03 10:00:00','2026-11-03 10:00:00','{\"version\": 3, \"isGlobalStylesUserThemeJSON\": true }','Custom Styles','','draft','closed','closed','','','','','2026-11-03 10:00:00','2026-11-03 10:00:00','',0,'http://127.0.0.1:8081/?p=902',0,'wp_global_styles','',0),(903,1,'2026-11-04 10:00:00','2026-11-04 10:00:00','{\"version\": 3, \"isGlobalStylesUserThemeJSON\": true }','Custom Styles','','publish','closed','closed','','wp-global-styles-twentytwentyfour','','','2026-11-04 10:00:00','2026-11-04 10:00:00','',0,'http://127.0.0.1:8081/?p=903',0,'wp_global_styles','',0);
INSERT INTO `wp_term_relationships` VALUES (901,901,0),(902,901,0),(903,902,0);
-- cleanup
DELETE FROM wp_posts WHERE ID BETWEEN 901 AND 903;
DELETE FROM wp_term_relationships WHERE object_id BETWEEN 901 AND 903;
DELETE FROM wp_term_taxonomy WHERE term_taxonomy_id BETWEEN 901 AND 902;
DELETE FROM wp_terms WHERE term_id BETWEEN 901 AND 902;
