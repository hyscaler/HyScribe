-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Navigation menus that reference/rest/golden.sh adds for
-- reference/rest/navigation-fallback-existing.list and deletes after with the
-- statements below `-- cleanup`, in mysqldump's form so the tests read them
-- too: the newest published menu (the one the fallback is), an older
-- published one and a newer draft that is not.
DELETE FROM wp_posts WHERE post_type = 'wp_navigation';
INSERT INTO `wp_posts` VALUES (7001,1,'2026-01-02 00:00:00','2026-01-02 00:00:00','<!-- wp:page-list /-->','Newest menu','','publish','closed','closed','','newest-menu','','','2026-01-02 00:00:00','2026-01-02 00:00:00','',0,'http://127.0.0.1:8081/?p=7001',0,'wp_navigation','',0),(7002,1,'2026-01-01 00:00:00','2026-01-01 00:00:00','<!-- wp:page-list /-->','Older menu','','publish','closed','closed','','older-menu','','','2026-01-01 00:00:00','2026-01-01 00:00:00','',0,'http://127.0.0.1:8081/?p=7002',0,'wp_navigation','',0),(7003,1,'2026-01-03 00:00:00','2026-01-03 00:00:00','<!-- wp:page-list /-->','Draft menu','','draft','closed','closed','','draft-menu','','','2026-01-03 00:00:00','2026-01-03 00:00:00','',0,'http://127.0.0.1:8081/?p=7003',0,'wp_navigation','',0);
-- cleanup
DELETE FROM wp_posts WHERE post_type = 'wp_navigation';
