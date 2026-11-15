-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- What reference/site-icon/golden.sh adds to the default install and
-- deletes after, as the Site Icon screen would store it: a 512 pixel icon
-- with the site icon sizes and core's square ones, a 40 pixel image with no
-- sizes, and two PDFs, with a preview image and without.
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (951, 1, '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 'icon', '', 'inherit', 'open', 'closed', '', 'icon', '', '',
    '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 0, 'http://127.0.0.1:8081/wp-content/uploads/2026/11/icon.png',
    0, 'attachment', 'image/png', 0),
  (952, 1, '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 'tiny', '', 'inherit', 'open', 'closed', '', 'tiny', '', '',
    '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 0, 'http://127.0.0.1:8081/wp-content/uploads/2026/11/tiny.png',
    0, 'attachment', 'image/png', 0),
  (953, 1, '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 'doc', '', 'inherit', 'open', 'closed', '', 'doc', '', '',
    '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 0, 'http://127.0.0.1:8081/wp-content/uploads/2026/11/doc.pdf',
    0, 'attachment', 'application/pdf', 0),
  (954, 1, '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 'bare', '', 'inherit', 'open', 'closed', '', 'bare', '', '',
    '2026-11-01 10:00:00', '2026-11-01 10:00:00', '', 0, 'http://127.0.0.1:8081/wp-content/uploads/2026/11/bare.pdf',
    0, 'attachment', 'application/pdf', 0);
INSERT INTO wp_postmeta (meta_id, post_id, meta_key, meta_value) VALUES
  (951, 951, '_wp_attached_file', '2026/11/icon.png'),
  (952, 951, '_wp_attachment_metadata', 'a:6:{s:5:"width";i:512;s:6:"height";i:512;s:4:"file";s:16:"2026/11/icon.png";s:8:"filesize";i:9000;s:5:"sizes";a:6:{s:6:"medium";a:4:{s:4:"file";s:16:"icon-300x300.png";s:5:"width";i:300;s:6:"height";i:300;s:9:"mime-type";s:9:"image/png";}s:9:"thumbnail";a:4:{s:4:"file";s:16:"icon-150x150.png";s:5:"width";i:150;s:6:"height";i:150;s:9:"mime-type";s:9:"image/png";}s:13:"site_icon-270";a:4:{s:4:"file";s:16:"icon-270x270.png";s:5:"width";i:270;s:6:"height";i:270;s:9:"mime-type";s:9:"image/png";}s:13:"site_icon-192";a:4:{s:4:"file";s:16:"icon-192x192.png";s:5:"width";i:192;s:6:"height";i:192;s:9:"mime-type";s:9:"image/png";}s:13:"site_icon-180";a:4:{s:4:"file";s:16:"icon-180x180.png";s:5:"width";i:180;s:6:"height";i:180;s:9:"mime-type";s:9:"image/png";}s:12:"site_icon-32";a:4:{s:4:"file";s:14:"icon-32x32.png";s:5:"width";i:32;s:6:"height";i:32;s:9:"mime-type";s:9:"image/png";}}s:10:"image_meta";a:0:{}}'),
  (953, 952, '_wp_attached_file', '2026/11/tiny.png'),
  (954, 952, '_wp_attachment_metadata', 'a:6:{s:5:"width";i:40;s:6:"height";i:40;s:4:"file";s:16:"2026/11/tiny.png";s:8:"filesize";i:100;s:5:"sizes";a:0:{}s:10:"image_meta";a:0:{}}'),
  (955, 953, '_wp_attached_file', '2026/11/doc.pdf'),
  (956, 953, '_wp_attachment_metadata', 'a:2:{s:8:"filesize";i:5000;s:5:"sizes";a:2:{s:4:"full";a:4:{s:4:"file";s:11:"doc-pdf.jpg";s:5:"width";i:1088;s:6:"height";i:1408;s:9:"mime-type";s:10:"image/jpeg";}s:9:"thumbnail";a:4:{s:4:"file";s:19:"doc-pdf-116x150.jpg";s:5:"width";i:116;s:6:"height";i:150;s:9:"mime-type";s:10:"image/jpeg";}}}'),
  (957, 954, '_wp_attached_file', '2026/11/bare.pdf'),
  (958, 954, '_wp_attachment_metadata', 'a:1:{s:8:"filesize";i:5000;}');
