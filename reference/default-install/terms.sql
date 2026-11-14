-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Terms and posts the default install lacks, for the term archive and
-- sitemap tests: a tag on the first post, a tag on no post, the first
-- post's aside format, a link category, a later post, a reusable block, and
-- for the dashboard the administrator's stored editor preferences, their
-- Quick Draft auto-draft, a comment of theirs on no post, and an approved
-- comment since marked as spam, and two revisions of the first post.
-- db.sh loads it into the wordpress_terms database; reference/head/golden.sh
-- adds it to the PHP oracle's database while it records term pages.
INSERT INTO wp_terms (term_id, name, slug, term_group) VALUES
  (2, 'Foo', 'foo', 0),
  (3, 'post-format-aside', 'post-format-aside', 0),
  (4, 'Empty', 'empty', 0),
  (5, 'Links', 'links', 0);
INSERT INTO wp_term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, parent, count) VALUES
  (2, 2, 'post_tag', 'Posts about "foo".', 0, 1),
  (3, 3, 'post_format', '', 0, 1),
  (4, 4, 'post_tag', '', 0, 0),
  (5, 5, 'link_category', '', 0, 1);
INSERT INTO wp_term_relationships (object_id, term_taxonomy_id, term_order) VALUES
  (1, 2, 0),
  (1, 3, 0);
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (101, 1, '2026-10-28 00:00:00', '2026-10-28 00:00:00', '', 'Later', '', 'publish', 'closed', 'closed', '', 'later', '', '',
   '2026-10-28 00:00:00', '2026-10-28 00:00:00', '', 0, 'http://default-install.test/?p=101', 0, 'post', '', 0),
  (102, 1, '2026-10-28 00:00:00', '2026-10-28 00:00:00', '', 'Block', '', 'publish', 'closed', 'closed', '', 'block', '', '',
   '2026-10-28 00:00:00', '2026-10-28 00:00:00', '', 0, 'http://default-install.test/?p=102', 0, 'wp_block', '', 0);
INSERT INTO wp_usermeta (umeta_id, user_id, meta_key, meta_value) VALUES
  (101, 1, 'wp_persisted_preferences', 'a:2:{s:4:"core";a:1:{s:10:"editorMode";s:6:"visual";}s:9:"_modified";s:24:"2026-10-28T00:00:00.000Z";}');
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (103, 1, '2026-10-28 00:00:00', '0000-00-00 00:00:00', '', 'Auto Draft', '', 'auto-draft', 'open', 'open', '', '', '', '',
   '2026-10-28 00:00:00', '0000-00-00 00:00:00', '', 0, 'http://default-install.test/?p=103', 0, 'post', '', 0);
INSERT INTO wp_usermeta (umeta_id, user_id, meta_key, meta_value) VALUES
  (102, 1, 'wp_dashboard_quick_press_last_post_id', '103');
INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, comment_author_email, comment_author_url, comment_author_IP,
  comment_date, comment_date_gmt, comment_content, comment_karma, comment_approved, comment_agent, comment_type, comment_parent, user_id) VALUES
  (2, 0, 'admin', 'admin@default-install.test', '', '', '2026-10-28 00:00:00', '2026-10-28 00:00:00', 'On no post', 0, '1', '', 'comment', 0, 1),
  (3, 1, 'Spammer', 'spam@example.test', '', '', '2026-10-28 00:00:00', '2026-10-28 00:00:00', 'Buy now', 0, 'spam', '', 'comment', 0, 0);
INSERT INTO wp_commentmeta (meta_id, comment_id, meta_key, meta_value) VALUES
  (1, 3, '_wp_trash_meta_status', '1'), (2, 3, '_wp_trash_meta_time', '1793145600');
-- Two revisions of the first post, as saving it twice in a minute stores
-- them, for the reads of a post's revisions.
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (104, 1, '2026-10-28 00:00:00', '2026-10-28 00:00:00', '', 'Hello world!', '', 'inherit', 'closed', 'closed', '', '1-revision-v1', '', '',
   '2026-10-28 00:00:00', '2026-10-28 00:00:00', '', 1, 'http://default-install.test/?p=104', 0, 'revision', '', 0),
  (105, 1, '2026-10-28 00:00:00', '2026-10-28 00:00:00', '', 'Hello world!', '', 'inherit', 'closed', 'closed', '', '1-revision-v1', '', '',
   '2026-10-28 00:00:00', '2026-10-28 00:00:00', '', 1, 'http://default-install.test/?p=105', 0, 'revision', '', 0);
