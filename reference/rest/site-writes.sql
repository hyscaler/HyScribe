-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The state reference/rest/golden.sh records reference/rest/site-writes.list
-- over: the default install as the tests load it, with a published
-- navigation menu by the administrator and a draft one by the editor, a
-- published synced pattern by the administrator and a draft one by the
-- author, in mysqldump's form so the tests read them too. The navigation
-- menu PHP's fallback creates on php.sh's first page view is taken
-- out and the posts' counter put back, so a new post gets the ID it gets in
-- the tests; db.sh resets the database after.
DELETE FROM wp_posts WHERE ID = 4 AND post_type = 'wp_navigation';
INSERT INTO `wp_posts` VALUES (851,1,'2026-11-05 10:00:00','2026-11-05 10:00:00','<!-- wp:navigation-link {\"label\":\"Sample Page\",\"type\":\"page\",\"id\":2,\"url\":\"http://127.0.0.1:8081/?page_id=2\",\"kind\":\"post-type\"} /-->','Header','','publish','closed','closed','','header','','','2026-11-05 10:00:00','2026-11-05 10:00:00','',0,'http://127.0.0.1:8081/?p=851',0,'wp_navigation','',0),(852,701,'2026-11-06 10:00:00','2026-11-06 10:00:00','','Draft menu','','draft','closed','closed','','','','','2026-11-06 10:00:00','2026-11-06 10:00:00','',0,'http://127.0.0.1:8081/?p=852',0,'wp_navigation','',0),(853,1,'2026-11-02 10:00:00','2026-11-02 10:00:00','<!-- wp:paragraph -->\n<p>Synced call to action</p>\n<!-- /wp:paragraph -->','Call to action','','publish','closed','closed','','call-to-action','','','2026-11-03 10:00:00','2026-11-03 10:00:00','',0,'http://127.0.0.1:8081/?p=853',0,'wp_block','',0),(854,702,'2026-11-04 10:00:00','2026-11-04 10:00:00','<!-- wp:heading -->\n<h2 class=\"wp-block-heading\">Drafted</h2>\n<!-- /wp:heading -->','Draft pattern','','draft','closed','closed','','','','','2026-11-04 10:00:00','2026-11-04 10:00:00','',0,'http://127.0.0.1:8081/?p=854',0,'wp_block','',0);
ALTER TABLE wp_posts AUTO_INCREMENT = 1;
-- cleanup
DELETE FROM wp_posts WHERE ID >= 851;
DELETE FROM wp_postmeta WHERE post_id >= 851;
