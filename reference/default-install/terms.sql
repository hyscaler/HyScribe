-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Terms and posts the default install lacks, for the term archive and
-- sitemap tests: a tag on the first post, a tag on no post, the first
-- post's aside format, a link category, a later post, a reusable block and
-- the administrator's stored editor preferences.
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
