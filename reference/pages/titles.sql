-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Rows for reference/pages/titles.list: published posts and pages with an
-- empty title, with quotes, apostrophes, dashes and an ampersand in the
-- title, and a page for the posts page, as WordPress would store them.
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (701, 1, '2026-11-08 10:00:00', '2026-11-08 10:00:00', 'Body', '', '', 'publish', 'open', 'open', '', '701-2', '', '',
   '2026-11-08 10:00:00', '2026-11-08 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=701', 0, 'post', '', 0),
  (702, 1, '2026-11-09 10:00:00', '2026-11-09 10:00:00', 'Filed', 'Tagged &amp; "filed" it''s -- done', '', 'publish', 'open', 'open', '', 'tagged-filed', '', '',
   '2026-11-09 10:00:00', '2026-11-09 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=702', 0, 'post', '', 0),
  (703, 1, '2026-11-10 10:00:00', '2026-11-10 10:00:00', 'Page body', '', '', 'publish', 'closed', 'closed', '', '703-2', '', '',
   '2026-11-10 10:00:00', '2026-11-10 10:00:00', '', 0, 'http://127.0.0.1:8081/?page_id=703', 0, 'page', '', 0),
  (704, 1, '2026-11-10 11:00:00', '2026-11-10 11:00:00', 'Posts page', 'The "blog" - wp', '', 'publish', 'closed', 'closed', '', 'blog', '', '',
   '2026-11-10 11:00:00', '2026-11-10 11:00:00', '', 0, 'http://127.0.0.1:8081/?page_id=704', 0, 'page', '', 0);
