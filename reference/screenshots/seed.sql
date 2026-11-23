-- Added to the fixture of both servers before the screenshot run: the
-- editor and the subscriber it logs in as, with the administrator's
-- password hash, so every role signs in with the same password, and the
-- result of the compression test (hs-admin never compresses its loaders,
-- docs/ADMIN.md, and answers the test's Ajax requests 501).
INSERT INTO wp_options (option_name, option_value, autoload) VALUES ('can_compress_scripts', '0', 'on');
INSERT INTO wp_users (ID, user_login, user_pass, user_nicename, user_email, user_url, user_registered, user_activation_key, user_status, display_name)
SELECT 2, 'editor', user_pass, 'editor', 'editor@example.test', '', '2026-01-01 00:00:00', '', 0, 'Edith Editor' FROM wp_users WHERE ID = 1;
INSERT INTO wp_users (ID, user_login, user_pass, user_nicename, user_email, user_url, user_registered, user_activation_key, user_status, display_name)
SELECT 3, 'subscriber', user_pass, 'subscriber', 'subscriber@example.test', '', '2026-01-01 00:00:00', '', 0, 'Sam Subscriber' FROM wp_users WHERE ID = 1;
INSERT INTO wp_usermeta (user_id, meta_key, meta_value) VALUES
(2, 'nickname', 'editor'), (2, 'first_name', 'Edith'), (2, 'last_name', 'Editor'),
(2, 'wp_capabilities', 'a:1:{s:6:"editor";b:1;}'), (2, 'wp_user_level', '7'),
(3, 'nickname', 'subscriber'), (3, 'first_name', 'Sam'), (3, 'last_name', 'Subscriber'),
(3, 'wp_capabilities', 'a:1:{s:10:"subscriber";b:1;}'), (3, 'wp_user_level', '0');
