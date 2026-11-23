-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Users reference/users-list/golden.sh adds and deletes after, as
-- WordPress would store them: an editor with first and last names and
-- published, private and draft posts, an author with a first name, a
-- subscriber with a last name, a user with no role, one with two roles,
-- one with no capabilities on the site, and one whose names and email hold
-- the entities WordPress's save filters leave and a last name of 0.
INSERT INTO wp_users (ID, user_login, user_pass, user_nicename, user_email, user_url, user_registered,
  user_activation_key, user_status, display_name) VALUES
  (601, 'editor1', '', 'editor1', 'ed@example.test', '', '2026-11-01 00:00:00', '', 0, 'Ed Itor'),
  (602, 'author1', '', 'author1', 'ann@example.test', 'http://ann.example.test', '2026-11-01 00:00:00', '', 0, 'Ann'),
  (603, 'zed', '', 'zed', 'zed@example.test', '', '2026-11-01 00:00:00', '', 0, 'Zimmer'),
  (604, 'nobody', '', 'nobody', 'nobody@example.test', '', '2026-11-01 00:00:00', '', 0, 'nobody'),
  (605, 'both', '', 'both', 'both@example.test', '', '2026-11-01 00:00:00', '', 0, 'Both Roles'),
  (606, 'elsewhere', '', 'elsewhere', 'elsewhere@example.test', '', '2026-11-01 00:00:00', '', 0, 'elsewhere'),
  (607, 'rnd', '', 'rnd', 'r&amp;d@example.test', '', '2026-11-01 00:00:00', '', 0, 'R&amp;D &lt;b&gt;');
INSERT INTO wp_usermeta (umeta_id, user_id, meta_key, meta_value) VALUES
  (601, 601, 'nickname', 'editor1'), (602, 601, 'first_name', 'Ed'), (603, 601, 'last_name', 'Itor'),
  (604, 601, 'wp_capabilities', 'a:1:{s:6:"editor";b:1;}'), (605, 601, 'wp_user_level', '7'),
  (606, 602, 'nickname', 'author1'), (607, 602, 'first_name', 'Ann'), (608, 602, 'last_name', ''),
  (609, 602, 'wp_capabilities', 'a:1:{s:6:"author";b:1;}'), (610, 602, 'wp_user_level', '2'),
  (611, 603, 'nickname', 'zed'), (612, 603, 'first_name', ''), (613, 603, 'last_name', 'Zimmer'),
  (614, 603, 'wp_capabilities', 'a:1:{s:10:"subscriber";b:1;}'), (615, 603, 'wp_user_level', '0'),
  (616, 604, 'nickname', 'nobody'), (617, 604, 'wp_capabilities', 'a:0:{}'), (618, 604, 'wp_user_level', '0'),
  (619, 605, 'nickname', 'both'), (620, 605, 'first_name', 'Both'), (621, 605, 'last_name', 'Roles'),
  (622, 605, 'wp_capabilities', 'a:2:{s:6:"editor";b:1;s:6:"author";b:1;}'), (623, 605, 'wp_user_level', '7'),
  (624, 606, 'nickname', 'elsewhere'),
  (625, 607, 'nickname', 'rnd'), (626, 607, 'first_name', 'R&amp;D'), (627, 607, 'last_name', '0'),
  (628, 607, 'wp_capabilities', 'a:1:{s:10:"subscriber";b:1;}'), (629, 607, 'wp_user_level', '0');
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (601, 601, '2026-11-01 09:00:00', '2026-11-01 09:00:00', '', 'Edited', '', 'publish', 'open', 'open', '',
   'edited', '', '', '2026-11-01 09:00:00', '2026-11-01 09:00:00', '', 0, 'http://127.0.0.1:8081/?p=601', 0, 'post', '', 0),
  (602, 601, '2026-11-02 09:00:00', '2026-11-02 09:00:00', '', 'Private', '', 'private', 'open', 'open', '',
   'private', '', '', '2026-11-02 09:00:00', '2026-11-02 09:00:00', '', 0, 'http://127.0.0.1:8081/?p=602', 0, 'post', '', 0),
  (603, 601, '2026-11-03 09:00:00', '0000-00-00 00:00:00', '', 'Draft', '', 'draft', 'open', 'open', '',
   '', '', '', '2026-11-03 09:00:00', '2026-11-03 09:00:00', '', 0, 'http://127.0.0.1:8081/?p=603', 0, 'post', '', 0),
  (604, 601, '2026-11-04 09:00:00', '2026-11-04 09:00:00', '', 'A page', '', 'publish', 'closed', 'closed', '',
   'a-page', '', '', '2026-11-04 09:00:00', '2026-11-04 09:00:00', '', 0, 'http://127.0.0.1:8081/?page_id=604', 0, 'page', '', 0);
