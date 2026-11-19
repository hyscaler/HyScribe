-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The persisted preferences reference/rest/user-preferences.list's writes
-- find stored: an object for the administrator and the first three
-- users, a string, which the meta's schema refuses, for the subscriber.
INSERT INTO wp_usermeta (umeta_id, user_id, meta_key, meta_value) VALUES
  (9001, 1, 'wp_persisted_preferences', 'a:2:{s:4:"core";a:1:{s:10:"editorMode";s:6:"visual";}s:9:"_modified";s:24:"2026-11-18T10:00:00.000Z";}'),
  (9002, 701, 'wp_persisted_preferences', 'a:2:{s:4:"core";a:1:{s:10:"editorMode";s:6:"visual";}s:9:"_modified";s:24:"2026-11-18T10:00:00.000Z";}'),
  (9003, 702, 'wp_persisted_preferences', 'a:2:{s:4:"core";a:1:{s:10:"editorMode";s:6:"visual";}s:9:"_modified";s:24:"2026-11-18T10:00:00.000Z";}'),
  (9004, 703, 'wp_persisted_preferences', 'a:2:{s:4:"core";a:1:{s:10:"editorMode";s:6:"visual";}s:9:"_modified";s:24:"2026-11-18T10:00:00.000Z";}'),
  (9005, 704, 'wp_persisted_preferences', 's:1:"x";');
-- cleanup
DELETE FROM wp_usermeta WHERE umeta_id BETWEEN 9001 AND 9005;
