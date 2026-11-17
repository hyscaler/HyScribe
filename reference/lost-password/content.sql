-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Users reference/lost-password/golden.sh adds and deletes after, each
-- with a password reset key of its own, stored as WordPress stores them:
-- a key good until 2100, one asked for in 2001 (expired), a fast hash with
-- no time, a plain key as WordPress before 4.3 stored it, a portable
-- phpass hash as WordPress before 6.8 stored it, and a key good until 2100
-- for the reset that really changes a password; then the hash of an empty
-- key and a user whose login is "0", which WordPress never finds, each good
-- until 2100. Their plain keys, in that order: freshkey0123456789AB,
-- expiredkey0123456789, barekey0123456789ABC, plainkey0123456789AB,
-- phpasskey0123456789A, resetkey0123456789AB, the empty key and
-- zerokey0123456789ABC.
INSERT INTO wp_users (ID, user_login, user_pass, user_nicename, user_email, user_url, user_registered,
  user_activation_key, user_status, display_name) VALUES
  (701, 'lost-fresh', '', 'lost-fresh', 'fresh@example.test', '', '2026-11-01 00:00:00',
    '4102444800:$generic$Yu79T6tn6ak4fFir9NQwWuuq_AgGQlEIAtzovkd_', 0, 'lost-fresh'),
  (702, 'lost-expired', '', 'lost-expired', 'expired@example.test', '', '2026-11-01 00:00:00',
    '1000000000:$generic$uMnNBwAUYOS0ygF0SbHx5o1ji7t8s_Rdgs9-R_2F', 0, 'lost-expired'),
  (703, 'lost-bare', '', 'lost-bare', 'bare@example.test', '', '2026-11-01 00:00:00',
    '$generic$T0uTqW9uW5QmTYF5TnRS5cerKZPg1NkFpxK9wItU', 0, 'lost-bare'),
  (704, 'lost-plain', '', 'lost-plain', 'plain@example.test', '', '2026-11-01 00:00:00',
    'plainkey0123456789AB', 0, 'lost-plain'),
  (705, 'lost-phpass', '', 'lost-phpass', 'phpass@example.test', '', '2026-11-01 00:00:00',
    '4102444800:$P$BXZTjMZRGicF3VBEDYuPWmZ4OVOK0m0', 0, 'lost-phpass'),
  (706, 'lost-reset', '', 'lost-reset', 'reset@example.test', '', '2026-11-01 00:00:00',
    '4102444800:$generic$2OXFKFqn5zqNAyOyU3N8wD1O8ES-1FwtFHRdHfnk', 0, 'lost-reset'),
  (707, 'lost-empty', '', 'lost-empty', 'empty@example.test', '', '2026-11-01 00:00:00',
    '4102444800:$generic$XJQGK2eUryTa4vHbU0bDv1Ml3wGPwaC1ATczq0o_', 0, 'lost-empty'),
  (708, '0', '', '0', 'zero@example.test', '', '2026-11-01 00:00:00',
    '4102444800:$generic$naxB3YusCvTEc0rGzlDSOkRzD_89vlAMSMBnNh0X', 0, '0');
