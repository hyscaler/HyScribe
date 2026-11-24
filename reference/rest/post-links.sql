-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Posts reference/rest/golden.sh adds for reference/rest/post-links.list over the default
-- install and deletes after with the statements below `-- cleanup`, in mysqldump's form so
-- the tests read them too: private posts and a private page by the administrator and by
-- reference/rest/users.sql's author, a pending and a future post with names, and a draft
-- whose stored dates are the zero date.
INSERT INTO `wp_posts` VALUES (9301,1,'2026-10-28 10:00:00','2026-10-28 10:00:00','Private.','Private thoughts','','private','open','open','','private-thoughts','','','2026-10-28 10:00:00','2026-10-28 10:00:00','',0,'http://127.0.0.1:8081/?p=9301',0,'post','',0),(9302,1,'0000-00-00 00:00:00','0000-00-00 00:00:00','Zero.','Zero dates','','draft','open','open','','','','','0000-00-00 00:00:00','0000-00-00 00:00:00','',0,'http://127.0.0.1:8081/?p=9302',0,'post','',0),(9303,1,'2026-10-28 11:00:00','2026-10-28 11:00:00','Private page.','Private page','','private','closed','closed','','private-page','','','2026-10-28 11:00:00','2026-10-28 11:00:00','',0,'http://127.0.0.1:8081/?page_id=9303',0,'page','',0),(9304,702,'2026-10-28 12:00:00','2026-10-28 12:00:00','Theirs.','Author private','','private','open','open','','author-private','','','2026-10-28 12:00:00','2026-10-28 12:00:00','',0,'http://127.0.0.1:8081/?p=9304',0,'post','',0),(9305,1,'2026-10-28 13:00:00','2026-10-28 13:00:00','Pending.','Pending words','','pending','open','open','','pending-words','','','2026-10-28 13:00:00','2026-10-28 13:00:00','',0,'http://127.0.0.1:8081/?p=9305',0,'post','',0),(9306,1,'2030-01-01 10:00:00','2030-01-01 10:00:00','Later.','Later words','','future','open','open','','later-words','','','2026-10-28 13:00:00','2026-10-28 13:00:00','',0,'http://127.0.0.1:8081/?p=9306',0,'post','',0);
-- cleanup
DELETE FROM wp_posts WHERE ID BETWEEN 9301 AND 9399;
