-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- What reference/theme-tags/golden.sh adds to the default install and
-- deletes after, as the Customizer and Appearance > Menus would store it
-- for Hello Elementor: a custom logo of 500 by 150 pixels, a header menu
-- of two items in menu-1 and an empty menu in menu-2.
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (961, 1, '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 'logo', '', 'inherit', 'open', 'closed', '', 'logo', '', '',
    '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 0, 'http://127.0.0.1:8081/wp-content/uploads/2026/11/logo.png',
    0, 'attachment', 'image/png', 0),
  (962, 1, '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', '', '', 'publish', 'closed', 'closed', '', '962', '', '',
    '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=962', 1, 'nav_menu_item', '', 0),
  (963, 1, '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 'Elsewhere', '', 'publish', 'closed', 'closed', '', '963', '', '',
    '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=963', 2, 'nav_menu_item', '', 0);
INSERT INTO wp_postmeta (meta_id, post_id, meta_key, meta_value) VALUES
  (961, 961, '_wp_attached_file', '2026/11/logo.png'),
  (962, 961, '_wp_attachment_metadata', 'a:6:{s:5:"width";i:500;s:6:"height";i:150;s:4:"file";s:16:"2026/11/logo.png";s:8:"filesize";i:9000;s:5:"sizes";a:1:{s:6:"medium";a:4:{s:4:"file";s:15:"logo-300x90.png";s:5:"width";i:300;s:6:"height";i:90;s:9:"mime-type";s:9:"image/png";}}s:10:"image_meta";a:0:{}}'),
  (963, 962, '_menu_item_type', 'post_type'),
  (964, 962, '_menu_item_menu_item_parent', '0'),
  (965, 962, '_menu_item_object_id', '2'),
  (966, 962, '_menu_item_object', 'page'),
  (967, 962, '_menu_item_target', ''),
  (968, 962, '_menu_item_classes', 'a:1:{i:0;s:0:"";}'),
  (969, 962, '_menu_item_xfn', ''),
  (970, 962, '_menu_item_url', ''),
  (971, 963, '_menu_item_type', 'custom'),
  (972, 963, '_menu_item_menu_item_parent', '0'),
  (973, 963, '_menu_item_object_id', '963'),
  (974, 963, '_menu_item_object', 'custom'),
  (975, 963, '_menu_item_target', ''),
  (976, 963, '_menu_item_classes', 'a:1:{i:0;s:0:"";}'),
  (977, 963, '_menu_item_xfn', ''),
  (978, 963, '_menu_item_url', 'https://example.test/');
INSERT INTO wp_terms (term_id, name, slug, term_group) VALUES (961, 'Header', 'header', 0), (962, 'Footer', 'footer', 0);
INSERT INTO wp_term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, parent, count) VALUES
  (961, 961, 'nav_menu', '', 0, 2), (962, 962, 'nav_menu', '', 0, 0);
INSERT INTO wp_term_relationships (object_id, term_taxonomy_id, term_order) VALUES (962, 961, 0), (963, 961, 0);
INSERT INTO wp_options (option_id, option_name, option_value, autoload) VALUES
  (961, 'theme_mods_hello-elementor', 'a:2:{s:11:"custom_logo";i:961;s:18:"nav_menu_locations";a:2:{s:6:"menu-1";i:961;s:6:"menu-2";i:962;}}', 'auto');
