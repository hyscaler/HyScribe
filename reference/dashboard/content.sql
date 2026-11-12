-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Rows reference/dashboard/golden.sh adds for its content variant and
-- deletes after: a scheduled post, four drafts by the administrator, and
-- comments in each status, one by the administrator and one on no post.
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (201, 1, '2030-01-01 10:00:00', '2030-01-01 10:00:00', '', 'Later', '', 'future', 'open', 'open', '', 'later', '', '',
   '2030-01-01 10:00:00', '2030-01-01 10:00:00', '', 0, 'http://default-install.test/?p=201', 0, 'post', '', 0),
  (202, 1, '2026-10-28 09:00:00', '2026-10-28 09:00:00', 'One two three four five six seven eight nine ten eleven twelve', 'First draft', '', 'draft', 'open', 'open', '', '', '', '',
   '2026-10-28 09:00:00', '2026-10-28 09:00:00', '', 0, 'http://default-install.test/?p=202', 0, 'post', '', 0),
  (203, 1, '2026-10-29 09:00:00', '2026-10-29 09:00:00', '<b>Bold</b> words', '', '', 'draft', 'open', 'open', '', '', '', '',
   '2026-10-29 09:00:00', '2026-10-29 09:00:00', '', 0, 'http://default-install.test/?p=203', 0, 'post', '', 0),
  (204, 1, '2026-10-30 09:00:00', '2026-10-30 09:00:00', '', 'Third "draft"', '', 'draft', 'open', 'open', '', '', '', '',
   '2026-10-30 09:00:00', '2026-10-30 09:00:00', '', 0, 'http://default-install.test/?p=204', 0, 'post', '', 0),
  (205, 1, '2026-10-31 09:00:00', '2026-10-31 09:00:00', 'Last', 'Fourth draft', '', 'draft', 'open', 'open', '', '', '', '',
   '2026-10-31 09:00:00', '2026-10-31 09:00:00', '', 0, 'http://default-install.test/?p=205', 0, 'post', '', 0);
INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, comment_author_email, comment_author_url, comment_author_IP,
  comment_date, comment_date_gmt, comment_content, comment_karma, comment_approved, comment_agent, comment_type, comment_parent, user_id) VALUES
  (10, 1, 'Pinger', '', 'http://pinger.test/', '', '2026-11-01 10:00:00', '2026-11-01 10:00:00', 'A <em>ping</em> back', 0, '0', '', 'pingback', 0, 0),
  (11, 0, 'Nowhere', 'nowhere@example.test', 'http://', '', '2026-11-02 10:00:00', '2026-11-02 10:00:00', 'On no post', 0, '1', '', 'comment', 0, 0),
  (12, 1, 'Spammer', 'spam@example.test', '', '', '2026-11-03 10:00:00', '2026-11-03 10:00:00', 'Spam', 0, 'spam', '', 'comment', 0, 0),
  (13, 1, 'Trashed', 'trash@example.test', '', '', '2026-11-04 10:00:00', '2026-11-04 10:00:00', 'Trash', 0, 'trash', '', 'comment', 0, 0),
  (14, 1, 'admin', 'admin@default-install.test', 'http://127.0.0.1:8081', '', '2026-11-05 10:00:00', '2026-11-05 10:00:00', 'Mine', 0, '1', '', 'comment', 0, 1),
  (15, 1, 'Note', '', '', '', '2026-11-06 10:00:00', '2026-11-06 10:00:00', 'A note', 0, '0', '', 'note', 0, 0);
