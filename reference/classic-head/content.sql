-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- What reference/classic-head/golden.sh adds to the default install and
-- deletes after: a post with an excerpt, which Hello Elementor prints as
-- its description meta tag in wp_head().
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (971, 1, '2026-11-01 10:00:00', '2026-11-01 10:00:00', 'Body.', 'Excerpted', 'A <em>short</em> "summary" & more\n', 'publish',
    'closed', 'closed', '', 'excerpted', '', '', '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 0,
    'http://127.0.0.1:8081/?p=971', 0, 'post', '', 0);
