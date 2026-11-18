-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The state reference/upload-routes/golden.sh records over: the default
-- install as the tests load it, with an untitled draft of user 1 (900) to
-- upload to, so a new attachment gets the ID it gets there. The navigation
-- menu PHP's fallback creates on php.sh's first page view is taken
-- out, and the posts' counter put back; db.sh resets the database after.
DELETE FROM wp_posts WHERE ID = 4 AND post_type = 'wp_navigation';
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt,
  post_status, comment_status, ping_status, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, post_type)
  VALUES (900, 1, '2026-01-05 10:00:00', '2026-01-05 10:00:00', '', '', '', 'draft', 'open', 'open', '',
  '', '', '2026-01-05 10:00:00', '2026-01-05 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=900', 'post');
ALTER TABLE wp_posts AUTO_INCREMENT = 1;
