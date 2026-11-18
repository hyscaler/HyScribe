-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The state reference/rest/golden.sh records reference/rest/comment-writes.list
-- over: the default install with a held reply of the subscriber (2) and a
-- comment in the trash (3), on post 1; db.sh resets the database after.
INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, comment_author_email, comment_author_url,
  comment_author_IP, comment_date, comment_date_gmt, comment_content, comment_karma, comment_approved,
  comment_agent, comment_type, comment_parent, user_id) VALUES
  (2, 1, 'subscriber', 'subscriber@example.test', '', '127.0.0.1', '2026-11-02 00:00:00', '2026-11-02 00:00:00',
   'A held reply', 0, '0', '', 'comment', 1, 704),
  (3, 1, 'Binned', 'binned@example.test', '', '127.0.0.1', '2026-11-03 00:00:00', '2026-11-03 00:00:00',
   'A comment in the trash', 0, 'trash', '', 'comment', 0, 0);
INSERT INTO wp_commentmeta (meta_id, comment_id, meta_key, meta_value) VALUES
  (1, 3, '_wp_trash_meta_status', '1'), (2, 3, '_wp_trash_meta_time', '1793750400');
