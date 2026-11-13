-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Rows reference/comments-list/golden.sh adds and deletes after: comments
-- in every state (pending with markup, a pingback, the admin's reply, spam,
-- trashed, anonymous on a page, one on a protected post, one on a deleted
-- post), as WordPress would store them.
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (501, 1, '2026-11-01 09:00:00', '2026-11-01 09:00:00', 'Hidden', 'Locked away', '', 'publish', 'open', 'open', 'secret',
   'locked-away', '', '', '2026-11-01 09:00:00', '2026-11-01 09:00:00', '', 0, 'http://127.0.0.1:8081/?p=501', 0, 'post', '', 1);
INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, comment_author_email, comment_author_url, comment_author_IP,
  comment_date, comment_date_gmt, comment_content, comment_karma, comment_approved, comment_agent, comment_type, comment_parent, user_id) VALUES
  (501, 1, 'Pending Pat', 'pat@example.test', 'http://www.example.test/', '192.0.2.1', '2026-11-01 10:00:00', '2026-11-01 10:00:00',
   'Pending <b>bold</b> and <script>x</script> see http://example.test/link', 0, '0', '', 'comment', 0, 0),
  (502, 1, 'Some Blog', '', 'https://blog.example.test/a/very/long/path/that/goes/on/and/on/beyond-fifty/', '',
   '2026-11-02 10:00:00', '2026-11-02 10:00:00', 'Linked to you', 0, '1', '', 'pingback', 0, 0),
  (503, 1, 'admin', 'admin@default-install.test', '', '127.0.0.1', '2026-11-03 10:00:00', '2026-11-03 10:00:00',
   'Thanks!', 0, '1', '', 'comment', 1, 1),
  (504, 1, 'Spammer', 'spam@example.test', 'http://spam.example.test', '', '2026-11-04 10:00:00', '2026-11-04 10:00:00',
   'Buy now', 0, 'spam', '', 'comment', 0, 0),
  (505, 1, 'Trashed', 'gone@example.test', '', '', '2026-11-04 11:00:00', '2026-11-04 11:00:00',
   'Gone', 0, 'trash', '', 'comment', 0, 0),
  (506, 2, '', '', 'http://', '', '2026-11-05 10:00:00', '2026-11-05 10:00:00', 'Who am I', 0, '1', '', 'comment', 0, 0),
  (507, 501, 'Visitor', 'v@example.test', '', '', '2026-11-06 10:00:00', '2026-11-06 10:00:00',
   'Locked words', 0, '1', '', 'comment', 0, 0),
  (508, 999, 'Lost', 'lost@example.test', '', '', '2026-11-07 10:00:00', '2026-11-07 10:00:00',
   'On a post since deleted', 0, '1', '', 'comment', 0, 0);
UPDATE wp_posts SET comment_count = 3 WHERE ID = 1;
UPDATE wp_posts SET comment_count = 1 WHERE ID = 2;
