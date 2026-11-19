-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The state reference/rest/golden.sh records reference/rest/editor-meta.list
-- over, in mysqldump's form so the tests and reference/writes/actions-editor.php
-- read it too: the default install with a post without footnotes, a post with
-- footnotes and a revision that holds them, a page, a password protected
-- post, a sticky post, and a pattern with footnotes. The navigation menu
-- PHP's fallback creates on php.sh's first page view is taken out;
-- db.sh resets the database after.
DELETE FROM wp_posts WHERE ID = 4 AND post_type = 'wp_navigation';
INSERT INTO `wp_posts` VALUES (9701,1,'2026-11-10 10:00:00','2026-11-10 10:00:00','<!-- wp:paragraph -->\n<p>No notes yet.</p>\n<!-- /wp:paragraph -->','Plain','','publish','open','open','','plain','','','2026-11-10 10:00:00','2026-11-10 10:00:00','',0,'http://127.0.0.1:8081/?p=9701',0,'post','',0),(9702,1,'2026-11-10 11:00:00','2026-11-10 11:00:00','<!-- wp:paragraph -->\n<p>Noted.</p>\n<!-- /wp:paragraph -->','Noted','','publish','open','open','','noted','','','2026-11-10 11:00:00','2026-11-10 11:00:00','',0,'http://127.0.0.1:8081/?p=9702',0,'post','',0),(9703,1,'2026-11-10 11:00:00','2026-11-10 11:00:00','<!-- wp:paragraph -->\n<p>Noted.</p>\n<!-- /wp:paragraph -->','Noted','','inherit','closed','closed','','9702-revision-v1','','','2026-11-10 11:00:00','2026-11-10 11:00:00','',9702,'http://127.0.0.1:8081/?p=9703',0,'revision','',0),(9704,1,'2026-11-10 12:00:00','2026-11-10 12:00:00','<!-- wp:paragraph -->\n<p>A page.</p>\n<!-- /wp:paragraph -->','Paged','','publish','closed','closed','','paged','','','2026-11-10 12:00:00','2026-11-10 12:00:00','',0,'http://127.0.0.1:8081/?page_id=9704',0,'page','',0),(9705,1,'2026-11-10 13:00:00','2026-11-10 13:00:00','<!-- wp:paragraph -->\n<p>Guarded.</p>\n<!-- /wp:paragraph -->','Guarded','','publish','open','open','secret','guarded','','','2026-11-10 13:00:00','2026-11-10 13:00:00','',0,'http://127.0.0.1:8081/?p=9705',0,'post','',0),(9706,1,'2026-11-10 14:00:00','2026-11-10 14:00:00','<!-- wp:paragraph -->\n<p>Pinned.</p>\n<!-- /wp:paragraph -->','Pinned','','publish','open','open','','pinned','','','2026-11-10 14:00:00','2026-11-10 14:00:00','',0,'http://127.0.0.1:8081/?p=9706',0,'post','',0),(9707,1,'2026-11-10 15:00:00','2026-11-10 15:00:00','<!-- wp:paragraph -->\n<p>Reusable.</p>\n<!-- /wp:paragraph -->','Reusable','','publish','closed','closed','','reusable','','','2026-11-10 15:00:00','2026-11-10 15:00:00','',0,'http://127.0.0.1:8081/?p=9707',0,'wp_block','',0);
INSERT INTO `wp_postmeta` VALUES (9701,9702,'footnotes','[{\"id\":\"a1\",\"content\":\"Old\"}]'),(9702,9703,'footnotes','[{\"id\":\"a1\",\"content\":\"Old\"}]'),(9703,9707,'footnotes','[{\"id\":\"p1\",\"content\":\"Pattern note\"}]');
UPDATE wp_options SET option_value = 'a:1:{i:0;i:9706;}' WHERE option_name = 'sticky_posts';
-- cleanup
DELETE FROM wp_posts WHERE ID BETWEEN 9701 AND 9799;
DELETE FROM wp_postmeta WHERE meta_id BETWEEN 9701 AND 9799;
UPDATE wp_options SET option_value = 'a:0:{}' WHERE option_name = 'sticky_posts';
