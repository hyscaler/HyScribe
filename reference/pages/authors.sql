-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Rows for reference/pages/authors.list, as WordPress would store them: an
-- author whose nicename differs from the login, with three published
-- posts, a private and a draft post and a page; an author with no posts;
-- and one with only a draft and a private post.
INSERT INTO wp_users (ID, user_login, user_pass, user_nicename, user_email, user_url, user_registered,
  user_activation_key, user_status, display_name) VALUES
  (750, 'jane.doe', '', 'jane-doe', 'jane@example.test', '', '2026-11-01 00:00:00', '', 0, 'Jane Doe'),
  (751, 'quiet', '', 'quiet', 'quiet@example.test', '', '2026-11-01 00:00:00', '', 0, 'Quiet &amp; One'),
  (752, 'drafter', '', 'drafter', 'drafter@example.test', '', '2026-11-01 00:00:00', '', 0, 'Drafter');
INSERT INTO wp_usermeta (umeta_id, user_id, meta_key, meta_value) VALUES
  (750, 750, 'nickname', 'jane.doe'), (751, 750, 'first_name', 'Jane'), (752, 750, 'last_name', 'Doe'),
  (753, 750, 'description', 'Writes about things.'),
  (754, 750, 'wp_capabilities', 'a:1:{s:6:"author";b:1;}'), (755, 750, 'wp_user_level', '2'),
  (756, 751, 'nickname', 'quiet'), (757, 751, 'wp_capabilities', 'a:1:{s:6:"author";b:1;}'),
  (758, 751, 'wp_user_level', '2'),
  (759, 752, 'nickname', 'drafter'), (760, 752, 'wp_capabilities', 'a:1:{s:6:"author";b:1;}'),
  (761, 752, 'wp_user_level', '2');
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (750, 750, '2026-11-11 10:00:00', '2026-11-11 10:00:00', '<!-- wp:paragraph -->\n<p>Body of first by jane.</p>\n<!-- /wp:paragraph -->', 'First by Jane', '', 'publish', 'open', 'open', '',
   'first-by-jane', '', '', '2026-11-11 10:00:00', '2026-11-11 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=750', 0, 'post', '', 0),
  (751, 750, '2026-11-12 10:00:00', '2026-11-12 10:00:00', '<!-- wp:paragraph -->\n<p>Body of second by jane.</p>\n<!-- /wp:paragraph -->', 'Second by Jane', '', 'publish', 'open', 'open', '',
   'second-by-jane', '', '', '2026-11-12 10:00:00', '2026-11-12 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=751', 0, 'post', '', 0),
  (752, 750, '2026-11-13 10:00:00', '2026-11-13 10:00:00', '<!-- wp:paragraph -->\n<p>Body of third by jane.</p>\n<!-- /wp:paragraph -->', 'Third by Jane', '', 'publish', 'open', 'open', '',
   'third-by-jane', '', '', '2026-11-13 10:00:00', '2026-11-13 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=752', 0, 'post', '', 0),
  (753, 750, '2026-11-14 10:00:00', '2026-11-14 10:00:00', '<!-- wp:paragraph -->\n<p>Body of private by jane.</p>\n<!-- /wp:paragraph -->', 'Private by Jane', '', 'private', 'open', 'open', '',
   'private-by-jane', '', '', '2026-11-14 10:00:00', '2026-11-14 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=753', 0, 'post', '', 0),
  (754, 750, '2026-11-15 10:00:00', '0000-00-00 00:00:00', '<!-- wp:paragraph -->\n<p>Body of draft by jane.</p>\n<!-- /wp:paragraph -->', 'Draft by Jane', '', 'draft', 'open', 'open', '',
   '', '', '', '2026-11-15 10:00:00', '2026-11-15 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=754', 0, 'post', '', 0),
  (755, 750, '2026-11-16 10:00:00', '2026-11-16 10:00:00', '<!-- wp:paragraph -->\n<p>Body of page by jane.</p>\n<!-- /wp:paragraph -->', 'Page by Jane', '', 'publish', 'closed', 'closed', '',
   'page-by-jane', '', '', '2026-11-16 10:00:00', '2026-11-16 10:00:00', '', 0, 'http://127.0.0.1:8081/?page_id=755', 0, 'page', '', 0),
  (756, 752, '2026-11-17 10:00:00', '0000-00-00 00:00:00', '<!-- wp:paragraph -->\n<p>Body of draft by drafter.</p>\n<!-- /wp:paragraph -->', 'Draft by Drafter', '', 'draft', 'open', 'open', '',
   '', '', '', '2026-11-17 10:00:00', '2026-11-17 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=756', 0, 'post', '', 0),
  (757, 752, '2026-11-18 10:00:00', '2026-11-18 10:00:00', '<!-- wp:paragraph -->\n<p>Body of private by drafter.</p>\n<!-- /wp:paragraph -->', 'Private by Drafter', '', 'private', 'open', 'open', '',
   'private-by-drafter', '', '', '2026-11-18 10:00:00', '2026-11-18 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=757', 0, 'post', '', 0);
