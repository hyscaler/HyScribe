-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The state reference/rest/golden.sh records reference/rest/comment-notes.list
-- over: the default install with a comment of the subscriber (10) and a note
-- of the administrator (11) on post 1 with a reply that resolves it (12),
-- a note on a post of the author (14),
-- and a note on the page (13); each insert is one line, as the write oracle
-- reads it, and the statements below `-- cleanup` run after; db.sh resets the
-- database.
INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, comment_author_email, comment_author_url, comment_author_IP, comment_date, comment_date_gmt, comment_content, comment_karma, comment_approved, comment_agent, comment_type, comment_parent, user_id) VALUES (10, 1, 'subscriber', 'subscriber@example.test', '', '127.0.0.1', '2026-11-10 00:00:00', '2026-11-10 00:00:00', 'A comment', 0, '1', '', 'comment', 0, 704), (11, 1, 'admin', 'admin@example.test', '', '127.0.0.1', '2026-11-11 00:00:00', '2026-11-11 00:00:00', 'A note', 0, '1', '', 'note', 0, 1), (12, 1, 'admin', 'admin@example.test', '', '127.0.0.1', '2026-11-12 00:00:00', '2026-11-12 00:00:00', 'Done', 0, '1', '', 'note', 11, 1), (13, 2, 'admin', 'admin@example.test', '', '127.0.0.1', '2026-11-13 00:00:00', '2026-11-13 00:00:00', 'A page note', 0, '1', '', 'note', 0, 1);
INSERT INTO wp_commentmeta (meta_id, comment_id, meta_key, meta_value) VALUES (10, 12, '_wp_note_status', 'resolved');
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status, comment_status, ping_status, post_name, to_ping, pinged, post_content_filtered, post_type) VALUES (20, 702, '2026-11-14 10:00:00', '2026-11-14 10:00:00', 'Written by the author', 'By the author', '', 'publish', 'open', 'open', 'by-the-author', '', '', '', 'post');
INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, comment_author_email, comment_author_url, comment_author_IP, comment_date, comment_date_gmt, comment_content, comment_karma, comment_approved, comment_agent, comment_type, comment_parent, user_id) VALUES (14, 20, 'admin', 'admin@example.test', '', '127.0.0.1', '2026-11-14 00:00:00', '2026-11-14 00:00:00', 'A note on the author''s post', 0, '1', '', 'note', 0, 1);
-- cleanup
DELETE FROM wp_posts WHERE ID = 20;
DELETE FROM wp_comments WHERE comment_ID BETWEEN 10 AND 99;
DELETE FROM wp_commentmeta WHERE meta_id BETWEEN 10 AND 99;
