-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The state reference/rest/golden.sh records reference/rest/autosaves.list
-- over: the author's draft 861, the same with the administrator's lock
-- (864), the administrator's published post 862 with their autosave 865,
-- their draft page 863, and their draft 866 the editor holds the lock of,
-- in mysqldump's form so the tests read them too. The locks end in 2286,
-- so they stay active. The navigation menu PHP's fallback creates on
-- php.sh's first page view is taken out and the posts' counter put
-- back, so a new post gets the ID it gets in the tests; db.sh resets the
-- database after.
DELETE FROM wp_posts WHERE ID = 4 AND post_type = 'wp_navigation';
INSERT INTO `wp_posts` VALUES (861,702,'2026-11-06 10:00:00','0000-00-00 00:00:00','<!-- wp:paragraph -->\n<p>First words</p>\n<!-- /wp:paragraph -->','Author draft','','draft','open','open','','','','','2026-11-06 10:00:00','2026-11-06 10:00:00','',0,'http://127.0.0.1:8081/?p=861',0,'post','',0),(862,1,'2026-11-06 11:00:00','2026-11-06 11:00:00','<!-- wp:paragraph -->\n<p>Out there</p>\n<!-- /wp:paragraph -->','Published','','publish','open','open','','published','','','2026-11-06 11:00:00','2026-11-06 11:00:00','',0,'http://127.0.0.1:8081/?p=862',0,'post','',0),(863,1,'2026-11-06 12:00:00','0000-00-00 00:00:00','','Draft page','','draft','closed','closed','','','','','2026-11-06 12:00:00','2026-11-06 12:00:00','',0,'http://127.0.0.1:8081/?page_id=863',0,'page','',0),(864,702,'2026-11-06 13:00:00','0000-00-00 00:00:00','<!-- wp:paragraph -->\n<p>Locked words</p>\n<!-- /wp:paragraph -->','Locked draft','','draft','open','open','','','','','2026-11-06 13:00:00','2026-11-06 13:00:00','',0,'http://127.0.0.1:8081/?p=864',0,'post','',0),(865,1,'2026-11-06 14:00:00','2026-11-06 14:00:00','<!-- wp:paragraph -->\n<p>Autosaved before</p>\n<!-- /wp:paragraph -->','Published','','inherit','closed','closed','','862-autosave-v1','','','2026-11-06 14:00:00','2026-11-06 14:00:00','',862,'http://127.0.0.1:8081/?p=865',0,'revision','',0),(866,1,'2026-11-06 15:00:00','0000-00-00 00:00:00','','Held draft','','draft','open','open','','','','','2026-11-06 15:00:00','2026-11-06 15:00:00','',0,'http://127.0.0.1:8081/?p=866',0,'post','',0);
INSERT INTO `wp_postmeta` VALUES (861,864,'_edit_lock','9999999999:1'),(862,866,'_edit_lock','9999999999:701');
INSERT INTO `wp_term_relationships` VALUES (861,1,0),(862,1,0),(864,1,0),(866,1,0);
ALTER TABLE wp_posts AUTO_INCREMENT = 1;
-- cleanup
DELETE FROM wp_posts WHERE ID >= 861;
DELETE FROM wp_postmeta WHERE post_id >= 861;
DELETE FROM wp_term_relationships WHERE object_id >= 861;
