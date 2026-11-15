-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Rows reference/action-checks/golden.sh adds and deletes after, as
-- WordPress would store them: another user (an author) with a published
-- post, a post of the administrator's in the Trash with a comment, and a
-- tag.
INSERT INTO wp_users (ID, user_login, user_pass, user_nicename, user_email, user_url, user_registered,
  user_activation_key, user_status, display_name) VALUES
  (801, 'other', '', 'other', 'other@example.test', '', '2026-11-01 00:00:00', '', 0, 'other');
INSERT INTO wp_usermeta (umeta_id, user_id, meta_key, meta_value) VALUES
  (801, 801, 'nickname', 'other'), (802, 801, 'wp_capabilities', 'a:1:{s:6:"author";b:1;}'),
  (803, 801, 'wp_user_level', '2');
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt,
  post_status, comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified,
  post_modified_gmt, post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type,
  comment_count) VALUES
  (801, 801, '2026-11-02 00:00:00', '2026-11-02 00:00:00', 'Other text.', 'Other post', '', 'publish',
   'open', 'open', '', 'other-post', '', '', '2026-11-02 00:00:00', '2026-11-02 00:00:00', '', 0,
   'http://default-install.test/?p=801', 0, 'post', '', 0),
  (802, 1, '2026-11-03 00:00:00', '2026-11-03 00:00:00', 'Trashed text.', 'Trashed post', '', 'trash',
   'open', 'open', '', 'trashed-post__trashed', '', '', '2026-11-03 00:00:00', '2026-11-03 00:00:00', '',
   0, 'http://default-install.test/?p=802', 0, 'post', '', 0);
INSERT INTO wp_postmeta (meta_id, post_id, meta_key, meta_value) VALUES
  (801, 802, '_wp_trash_meta_status', 'publish'), (802, 802, '_wp_trash_meta_time', '1793750400');
INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, comment_author_email,
  comment_author_url, comment_author_IP, comment_date, comment_date_gmt, comment_content, comment_karma,
  comment_approved, comment_agent, comment_type, comment_parent, user_id) VALUES
  (801, 802, 'A reader', 'reader@example.test', '', '', '2026-11-03 00:00:00', '2026-11-03 00:00:00',
   'On a trashed post.', 0, 'post-trashed', '', 'comment', 0, 0);
INSERT INTO wp_terms (term_id, name, slug, term_group) VALUES (801, 'Tagged', 'tagged', 0);
INSERT INTO wp_term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, parent, count)
  VALUES (801, 801, 'post_tag', '', 0, 0);
-- cleanup
DELETE FROM wp_users WHERE ID = 801;
DELETE FROM wp_usermeta WHERE user_id = 801;
DELETE FROM wp_posts WHERE ID IN (801, 802);
DELETE FROM wp_postmeta WHERE post_id IN (801, 802);
DELETE FROM wp_comments WHERE comment_ID = 801;
DELETE FROM wp_terms WHERE term_id = 801;
DELETE FROM wp_term_taxonomy WHERE term_taxonomy_id = 801;
