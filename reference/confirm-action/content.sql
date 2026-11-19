-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- What reference/confirm-action/golden.sh adds to the default install and
-- deletes after: personal data requests as wp_create_user_request() and
-- wp_generate_user_request_key() store them, each with the fast hash of a
-- key as its post_password. The keys are those of reference/lost-password:
-- freshkey0123456789AB for 951 to 953, 955, 956 and 959, and
-- expiredkey0123456789 for 954. 951 is a pending export, 952 a pending
-- erasure, 953 a failed export, 954 a pending export asked for in 2001, 955
-- a confirmed one, 956 a completed one, 957 a pending one with no key, 959 a
-- pending export whose admin was told already, 960 a pending request of an
-- action WordPress has no mail for.
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (951, 0, '2026-01-02 03:04:05', '2026-01-02 03:04:05', '[]', 'reader@example.test', '', 'request-pending', 'closed',
    'closed', '$generic$Yu79T6tn6ak4fFir9NQwWuuq_AgGQlEIAtzovkd_', 'export_personal_data', '', '', UTC_TIMESTAMP(),
    UTC_TIMESTAMP(), '', 0, '', 0, 'user_request', '', 0),
  (952, 0, '2026-01-02 03:04:05', '2026-01-02 03:04:05', '[]', 'eraser@example.test', '', 'request-pending', 'closed',
    'closed', '$generic$Yu79T6tn6ak4fFir9NQwWuuq_AgGQlEIAtzovkd_', 'remove_personal_data', '', '', UTC_TIMESTAMP(),
    UTC_TIMESTAMP(), '', 0, '', 0, 'user_request', '', 0),
  (953, 1, '2026-01-02 03:04:05', '2026-01-02 03:04:05', '[]', 'admin@default-install.test', '', 'request-failed',
    'closed', 'closed', '$generic$Yu79T6tn6ak4fFir9NQwWuuq_AgGQlEIAtzovkd_', 'export_personal_data', '', '',
    UTC_TIMESTAMP(), UTC_TIMESTAMP(), '', 0, '', 0, 'user_request', '', 0),
  (954, 0, '2001-01-02 03:04:05', '2001-01-02 03:04:05', '[]', 'late@example.test', '', 'request-pending', 'closed',
    'closed', '$generic$uMnNBwAUYOS0ygF0SbHx5o1ji7t8s_Rdgs9-R_2F', 'export_personal_data', '', '',
    '2001-01-02 03:04:05', '2001-01-02 03:04:05', '', 0, '', 0, 'user_request', '', 0),
  (955, 0, '2026-01-02 03:04:05', '2026-01-02 03:04:05', '[]', 'done@example.test', '', 'request-confirmed', 'closed',
    'closed', '$generic$Yu79T6tn6ak4fFir9NQwWuuq_AgGQlEIAtzovkd_', 'export_personal_data', '', '', UTC_TIMESTAMP(),
    UTC_TIMESTAMP(), '', 0, '', 0, 'user_request', '', 0),
  (956, 0, '2026-01-02 03:04:05', '2026-01-02 03:04:05', '[]', 'closed@example.test', '', 'request-completed',
    'closed', 'closed', '$generic$Yu79T6tn6ak4fFir9NQwWuuq_AgGQlEIAtzovkd_', 'export_personal_data', '', '',
    UTC_TIMESTAMP(), UTC_TIMESTAMP(), '', 0, '', 0, 'user_request', '', 0),
  (957, 0, '2026-01-02 03:04:05', '2026-01-02 03:04:05', '[]', 'nokey@example.test', '', 'request-pending', 'closed',
    'closed', '', 'export_personal_data', '', '', UTC_TIMESTAMP(), UTC_TIMESTAMP(), '', 0, '', 0, 'user_request', '',
    0),
  (959, 0, '2026-01-02 03:04:05', '2026-01-02 03:04:05', '[]', 'told@example.test', '', 'request-pending', 'closed',
    'closed', '$generic$Yu79T6tn6ak4fFir9NQwWuuq_AgGQlEIAtzovkd_', 'export_personal_data', '', '', UTC_TIMESTAMP(),
    UTC_TIMESTAMP(), '', 0, '', 0, 'user_request', '', 0),
  (960, 0, '2026-01-02 03:04:05', '2026-01-02 03:04:05', '[]', 'other@example.test', '', 'request-pending', 'closed',
    'closed', '$generic$Yu79T6tn6ak4fFir9NQwWuuq_AgGQlEIAtzovkd_', 'other_action', '', '', UTC_TIMESTAMP(),
    UTC_TIMESTAMP(), '', 0, '', 0, 'user_request', '', 0);
INSERT INTO wp_postmeta (meta_id, post_id, meta_key, meta_value) VALUES (951, 959, '_wp_admin_notified', '1');
