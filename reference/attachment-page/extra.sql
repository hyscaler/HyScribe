-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- What reference/attachment-page/golden.sh adds to the default install, over
-- reference/attachment-edit/content.sql, and deletes after: a block that is
-- no page a visitor can reach, with an image attached to it; an image whose
-- parent does not exist; an image that is its own parent; an image with no
-- file and no address; a file of a type with its own template name; and an imported file, whose type
-- has no slash.
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (9001, 1, '2026-06-01 10:00:00', '2026-06-01 10:00:00', '<!-- wp:paragraph --><p>Block</p><!-- /wp:paragraph -->', 'A block', '', 'publish', 'closed', 'closed', '', 'a-block', '', '', '2026-06-01 10:00:00', '2026-06-01 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=9001', 0, 'wp_block', '', 0),
  (9002, 1, '2026-06-01 10:00:00', '2026-06-01 10:00:00', 'On a block', 'On a block', '', 'inherit', 'open', 'closed', '', 'on-a-block', '', '', '2026-06-01 10:00:00', '2026-06-01 10:00:00', '', 9001, 'http://127.0.0.1:8081/wp-content/uploads/2026/06/9002.png', 0, 'attachment', 'image/png', 0),
  (9003, 1, '2026-06-01 10:00:00', '2026-06-01 10:00:00', '', 'Orphan', '', 'inherit', 'open', 'closed', '', 'orphan', '', '', '2026-06-01 10:00:00', '2026-06-01 10:00:00', '', 99999, 'http://127.0.0.1:8081/wp-content/uploads/2026/06/9003.png', 0, 'attachment', 'image/png', 0),
  (9004, 1, '2026-06-01 10:00:00', '2026-06-01 10:00:00', '', 'Itself', '', 'inherit', 'open', 'closed', '', 'itself', '', '', '2026-06-01 10:00:00', '2026-06-01 10:00:00', '', 9004, 'http://127.0.0.1:8081/wp-content/uploads/2026/06/9004.png', 0, 'attachment', 'image/png', 0),
  (9005, 1, '2026-06-01 10:00:00', '2026-06-01 10:00:00', '', 'Nowhere', '', 'inherit', 'open', 'closed', '', 'nowhere', '', '', '2026-06-01 10:00:00', '2026-06-01 10:00:00', '', 0, '', 0, 'attachment', 'image/png', 0),
  (9006, 1, '2026-06-01 10:00:00', '2026-06-01 10:00:00', 'A zip', 'Archive', '', 'inherit', 'open', 'closed', '', 'archive', '', '', '2026-06-01 10:00:00', '2026-06-01 10:00:00', '', 1, 'http://127.0.0.1:8081/wp-content/uploads/2026/06/archive.zip', 0, 'attachment', 'application/zip', 0),
  (9007, 1, '2026-06-01 10:00:00', '2026-06-01 10:00:00', '', 'Twin', '', 'inherit', 'open', 'closed', '', 'twin', '', '', '2026-06-01 10:00:00', '2026-06-01 10:00:00', '', 0, 'http://127.0.0.1:8081/wp-content/uploads/2026/06/twin.png', 0, 'attachment', 'image/png', 0),
  (9008, 1, '2026-06-01 10:00:00', '2026-06-01 10:00:00', '', 'Imported', '', 'inherit', 'open', 'closed', '', 'imported', '', '', '2026-06-01 10:00:00', '2026-06-01 10:00:00', '', 0, 'http://127.0.0.1:8081/wp-content/uploads/2026/06/imp.png', 0, 'attachment', 'import', 0);
INSERT INTO wp_postmeta (meta_id, post_id, meta_key, meta_value) VALUES
  (9001, 9002, '_wp_attached_file', '2026/06/9002.png'),
  (9002, 9003, '_wp_attached_file', '2026/06/9003.png'),
  (9003, 9004, '_wp_attached_file', '2026/06/9004.png'),
  (9004, 9006, '_wp_attached_file', '2026/06/archive.zip'),
  (9005, 9007, '_wp_attached_file', '2026/06/twin.png'),
  (9006, 9008, '_wp_attached_file', '2026/06/imp.png');
  
