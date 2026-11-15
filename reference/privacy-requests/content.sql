-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- What reference/privacy-requests/golden.sh adds to the default install and
-- deletes after, as wp_create_user_request() and the request screens store
-- them: personal data export and erasure requests in each status, one a
-- registered user's, with the confirmation and completion times their meta
-- keeps. The pending requests were modified just now, so viewing them does
-- not mark them failed; one was made an hour ago, and one confirmed two
-- hours ago.
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (861, 0, '2026-01-02 03:04:05', '2026-01-02 03:04:05', '[]', 'pending@example.test', '', 'request-pending', 'closed',
    'closed', '$generic$key', 'export_personal_data', '', '', UTC_TIMESTAMP(), UTC_TIMESTAMP(), '', 0, '', 0,
    'user_request', '', 0),
  (862, 1, '2026-01-03 13:14:15', '2026-01-03 13:14:15', '[]', 'admin@default-install.test', '', 'request-confirmed',
    'closed', 'closed', '', 'export_personal_data', '', '', '2026-01-04 00:00:00', '2026-01-04 00:00:00', '', 0, '', 0,
    'user_request', '', 0),
  (863, 0, '2025-12-24 23:00:00', '2025-12-24 23:00:00', '[]', 'done@example.test', '', 'request-completed', 'closed',
    'closed', '', 'export_personal_data', '', '', '2025-12-26 00:00:00', '2025-12-26 00:00:00', '', 0, '', 0,
    'user_request', '', 0),
  (864, 0, '2025-11-30 08:00:00', '2025-11-30 08:00:00', '[]', 'failed@example.test', '', 'request-failed', 'closed',
    'closed', '', 'export_personal_data', '', '', '2025-12-01 08:00:00', '2025-12-01 08:00:00', '', 0, '', 0,
    'user_request', '', 0),
  (865, 0, DATE_SUB(UTC_TIMESTAMP(), INTERVAL 1 HOUR), DATE_SUB(UTC_TIMESTAMP(), INTERVAL 1 HOUR), '[]',
    'Another.Pending@example.test', '', 'request-pending', 'closed', 'closed', '$generic$key', 'export_personal_data', '', '', UTC_TIMESTAMP(), UTC_TIMESTAMP(), '', 0, '', 0,
    'user_request', '', 0),
  (871, 0, '2026-01-05 10:00:00', '2026-01-05 10:00:00', '[]', 'erase-pending@example.test', '', 'request-pending',
    'closed', 'closed', '$generic$key', 'remove_personal_data', '', '', UTC_TIMESTAMP(), UTC_TIMESTAMP(), '', 0, '', 0,
    'user_request', '', 0),
  (872, 0, '2026-01-06 10:00:00', '2026-01-06 10:00:00', '[]', 'erase-confirmed@example.test', '', 'request-confirmed',
    'closed', 'closed', '', 'remove_personal_data', '', '', '2026-01-07 00:00:00', '2026-01-07 00:00:00', '', 0, '', 0,
    'user_request', '', 0),
  (873, 0, '2026-01-07 10:00:00', '2026-01-07 10:00:00', '[]', 'erase-done@example.test', '', 'request-completed',
    'closed', 'closed', '', 'remove_personal_data', '', '', '2026-01-08 00:00:00', '2026-01-08 00:00:00', '', 0, '', 0,
    'user_request', '', 0),
  (874, 0, '2026-01-08 10:00:00', '2026-01-08 10:00:00', '[]', 'erase-failed@example.test', '', 'request-failed',
    'closed', 'closed', '', 'remove_personal_data', '', '', '2026-01-09 00:00:00', '2026-01-09 00:00:00', '', 0, '', 0,
    'user_request', '', 0);
INSERT INTO wp_postmeta (meta_id, post_id, meta_key, meta_value) VALUES
  (861, 862, '_wp_user_request_confirmed_timestamp', UNIX_TIMESTAMP() - 7200),
  (862, 863, '_wp_user_request_confirmed_timestamp', '1766620800'),
  (863, 863, '_wp_user_request_completed_timestamp', '1766707200'),
  (864, 872, '_wp_user_request_confirmed_timestamp', '1767744000'),
  (865, 873, '_wp_user_request_confirmed_timestamp', '1767830400'),
  (866, 873, '_wp_user_request_completed_timestamp', '1767873600');
