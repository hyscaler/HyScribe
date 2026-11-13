-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Attachments for reference/settings/golden.sh, whose months the media
-- library's filter offers: one in the library, one in the trash.
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (701, 1, '2025-03-04 10:00:00', '2025-03-04 10:00:00', '', 'Picture', '', 'inherit', 'open', 'closed', '',
   'picture', '', '', '2025-03-04 10:00:00', '2025-03-04 10:00:00', '', 0, 'http://127.0.0.1:8081/picture.png', 0,
   'attachment', 'image/png', 0),
  (702, 1, '2024-12-01 10:00:00', '2024-12-01 10:00:00', '', 'Old', '', 'trash', 'open', 'closed', '',
   'old', '', '', '2024-12-01 10:00:00', '2024-12-01 10:00:00', '', 0, 'http://127.0.0.1:8081/old.png', 0,
   'attachment', 'image/png', 0);
