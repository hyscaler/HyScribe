-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Rows reference/posts-list/golden.sh adds and deletes after: a second
-- author, posts in every status the list shows (one by a deleted user),
-- categories, tags, an aside and comments, as WordPress would store them.
INSERT INTO wp_users (ID, user_login, user_pass, user_nicename, user_email, user_url, user_registered,
  user_activation_key, user_status, display_name) VALUES
  (2, 'writer', '', 'writer', 'writer@default-install.test', '', '2026-11-01 00:00:00', '', 0, 'Another Writer');
INSERT INTO wp_usermeta (user_id, meta_key, meta_value) VALUES
  (2, 'wp_capabilities', 'a:1:{s:6:"author";b:1;}'),
  (2, 'wp_user_level', '2');
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (301, 1, '2026-11-02 10:00:00', '0000-00-00 00:00:00', 'Draft words', 'A draft', '', 'draft', 'open', 'open', '', '', '', '',
   '2026-11-02 10:00:00', '2026-11-02 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=301', 0, 'post', '', 0),
  (302, 2, '2026-11-03 10:00:00', '2026-11-03 10:00:00', 'Waiting', 'Pending review', '', 'pending', 'open', 'closed', '', '', '', '',
   '2026-11-03 10:00:00', '2026-11-03 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=302', 0, 'post', '', 0),
  (303, 1, '2030-01-01 10:00:00', '2030-01-01 10:00:00', 'Soon', 'Later', '', 'future', 'closed', 'open', '', 'later', '', '',
   '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=303', 0, 'post', '', 0),
  (304, 1, '2026-11-04 10:00:00', '2026-11-04 10:00:00', 'Mine only', 'Private thoughts', '', 'private', 'open', 'open', '', 'private-thoughts', '', '',
   '2026-11-04 10:00:00', '2026-11-04 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=304', 0, 'post', '', 0),
  (305, 1, '2026-11-05 10:00:00', '2026-11-05 10:00:00', 'Hidden', 'Locked', '', 'publish', 'open', 'open', 'secret', 'locked', '', '',
   '2026-11-05 10:00:00', '2026-11-05 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=305', 0, 'post', '', 0),
  (306, 1, '2026-11-06 10:00:00', '2026-11-06 10:00:00', 'On top', 'Pinned', '', 'publish', 'open', 'open', '', 'pinned', '', '',
   '2026-11-06 10:00:00', '2026-11-06 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=306', 0, 'post', '', 0),
  (307, 1, '2026-11-07 10:00:00', '2026-11-07 10:00:00', 'Gone', 'Thrown away', '', 'trash', 'open', 'open', '', 'thrown-away__trashed', '', '',
   '2026-11-07 10:00:00', '2026-11-07 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=307', 0, 'post', '', 0),
  (308, 1, '2026-11-08 10:00:00', '2026-11-08 10:00:00', 'Body', '', 'One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen', 'publish', 'open', 'open', '', '308-2', '', '',
   '2026-11-08 10:00:00', '2026-11-08 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=308', 0, 'post', '', 0),
  (309, 1, '2026-11-09 10:00:00', '2026-11-09 10:00:00', 'Filed', 'Tagged &amp; "filed"', '', 'publish', 'open', 'open', '', 'tagged-filed', '', '',
   '2026-11-09 10:00:00', '2026-11-09 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=309', 0, 'post', '', 0),
  (310, 1, '2026-01-01 10:00:00', '2026-01-01 10:00:00', 'Late', 'Missed', '', 'future', 'open', 'open', '', 'missed', '', '',
   '2026-01-01 10:00:00', '2026-01-01 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=310', 0, 'post', '', 0),
  (311, 2, '2026-11-10 10:00:00', '2026-11-10 10:00:00', 'Theirs', 'By someone else', '', 'publish', 'open', 'open', '', 'by-someone-else', '', '',
   '2026-11-10 10:00:00', '2026-11-10 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=311', 0, 'post', '', 2),
  (312, 99, '0000-00-00 00:00:00', '0000-00-00 00:00:00', 'Left', 'Orphan', '', 'draft', 'open', 'open', '', '', '', '',
   '2026-11-11 10:00:00', '2026-11-11 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=312', 0, 'post', '', 0),
  (313, 2, '2026-11-11 10:00:00', '2026-11-11 10:00:00', 'Theirs only', 'Their secret', '', 'private', 'open', 'open', '', 'their-secret', '', '',
   '2026-11-11 10:00:00', '2026-11-11 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=313', 0, 'post', '', 0);
INSERT INTO wp_postmeta (post_id, meta_key, meta_value) VALUES
  (312, '_customize_changeset_uuid', '0c3a1b2c-1111-4222-8333-444455556666'),
  (306, '_wp_page_template', 'wide');
INSERT INTO wp_terms (term_id, name, slug, term_group) VALUES
  (301, 'News', 'news', 0), (302, 'Local', 'local', 0), (303, 'Rust &amp; Go', 'rust-go', 0), (304, 'PHP', 'php', 0),
  (305, 'post-format-aside', 'post-format-aside', 0), (306, 'Empty', 'empty', 0), (307, 'post-format-video', 'post-format-video', 0);
INSERT INTO wp_term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, parent, count) VALUES
  (301, 301, 'category', '', 0, 1), (302, 302, 'category', '', 301, 1), (303, 303, 'post_tag', '', 0, 1),
  (304, 304, 'post_tag', '', 0, 1), (305, 305, 'post_format', '', 0, 1), (306, 306, 'category', '', 0, 0),
  (307, 307, 'post_format', '', 0, 0);
INSERT INTO wp_term_relationships (object_id, term_taxonomy_id, term_order) VALUES
  (301, 1, 0), (302, 1, 0), (303, 1, 0), (304, 1, 0), (305, 1, 0), (306, 1, 0), (307, 1, 0), (308, 1, 0),
  (309, 301, 0), (309, 302, 0), (309, 303, 0), (309, 304, 0), (309, 305, 0), (310, 1, 0), (311, 1, 0),
  (312, 1, 0), (313, 1, 0);
UPDATE wp_term_taxonomy SET count = 5 WHERE term_taxonomy_id = 1;
INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, comment_author_email, comment_author_url, comment_author_IP,
  comment_date, comment_date_gmt, comment_content, comment_karma, comment_approved, comment_agent, comment_type, comment_parent, user_id) VALUES
  (301, 309, 'Visitor', 'visitor@example.test', '', '', '2026-11-11 10:00:00', '2026-11-11 10:00:00', 'Waiting', 0, '0', '', 'comment', 0, 0),
  (302, 311, 'Visitor', 'visitor@example.test', '', '', '2026-11-11 11:00:00', '2026-11-11 11:00:00', 'One', 0, '1', '', 'comment', 0, 0),
  (303, 311, 'Visitor', 'visitor@example.test', '', '', '2026-11-11 12:00:00', '2026-11-11 12:00:00', 'Two', 0, '1', '', 'comment', 0, 0),
  (304, 311, 'Visitor', 'visitor@example.test', '', '', '2026-11-11 13:00:00', '2026-11-11 13:00:00', 'Three', 0, '0', '', 'comment', 0, 0);
