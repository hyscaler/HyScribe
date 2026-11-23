-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Users reference/rest/golden.sh adds and deletes after, one for each role
-- below the administrator, as WordPress stores them. Their password is the
-- fixture administrator's, `reference-only-password`.
INSERT INTO wp_users (ID, user_login, user_pass, user_nicename, user_email, user_url, user_registered,
  user_activation_key, user_status, display_name) VALUES
  (701, 'editor', '$wp$2y$12$H0NCQv.BOqgC8HC3RvJ7u.sQwrqr14ruxSEt.fWE6FVneSAfNtBYy', 'editor', 'editor@example.test', '', '2026-11-01 00:00:00', '', 0, 'editor'),
  (702, 'author', '$wp$2y$12$H0NCQv.BOqgC8HC3RvJ7u.sQwrqr14ruxSEt.fWE6FVneSAfNtBYy', 'author', 'author@example.test', '', '2026-11-01 00:00:00', '', 0, 'author'),
  (703, 'contributor', '$wp$2y$12$H0NCQv.BOqgC8HC3RvJ7u.sQwrqr14ruxSEt.fWE6FVneSAfNtBYy', 'contributor', 'contributor@example.test', '', '2026-11-01 00:00:00', '', 0, 'contributor'),
  (704, 'subscriber', '$wp$2y$12$H0NCQv.BOqgC8HC3RvJ7u.sQwrqr14ruxSEt.fWE6FVneSAfNtBYy', 'subscriber', 'subscriber@example.test', '', '2026-11-01 00:00:00', '', 0, 'subscriber');
INSERT INTO wp_usermeta (umeta_id, user_id, meta_key, meta_value) VALUES
  (701, 701, 'wp_capabilities', 'a:1:{s:6:"editor";b:1;}'), (702, 701, 'wp_user_level', '7'),
  (703, 702, 'wp_capabilities', 'a:1:{s:6:"author";b:1;}'), (704, 702, 'wp_user_level', '2'),
  (705, 703, 'wp_capabilities', 'a:1:{s:11:"contributor";b:1;}'), (706, 703, 'wp_user_level', '1'),
  (707, 704, 'wp_capabilities', 'a:1:{s:10:"subscriber";b:1;}'), (708, 704, 'wp_user_level', '0');
