-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Tags and comments reference/rest/golden.sh adds for reference/rest/pagination.list over the
-- default install and deletes after with the statements below `-- cleanup`, in mysqldump's
-- form so the tests read them too: three tags and two approved comments on the first post,
-- enough rows for the collections to have more than one page of one item.
INSERT INTO `wp_terms` VALUES (9701,'Alpha','alpha',0),(9702,'Beta','beta',0),(9703,'Gamma','gamma',0);
INSERT INTO `wp_term_taxonomy` VALUES (9701,9701,'post_tag','',0,0),(9702,9702,'post_tag','',0,0),(9703,9703,'post_tag','',0,0);
INSERT INTO `wp_comments` VALUES (9701,1,'First','first@example.test','','127.0.0.1','2026-10-28 10:00:00','2026-10-28 10:00:00','The first.',0,'1','','comment',0,0),(9702,1,'Second','second@example.test','','127.0.0.1','2026-10-28 11:00:00','2026-10-28 11:00:00','The second.',0,'1','','comment',0,0);
-- cleanup
DELETE FROM wp_terms WHERE term_id BETWEEN 9701 AND 9799;
DELETE FROM wp_term_taxonomy WHERE term_taxonomy_id BETWEEN 9701 AND 9799;
DELETE FROM wp_comments WHERE comment_ID BETWEEN 9701 AND 9799;
