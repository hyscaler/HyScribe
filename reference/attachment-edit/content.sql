-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- What reference/attachment-edit/golden.sh adds to the default install and
-- deletes after, as WordPress would store it: an editor, user 2, who holds
-- the edit locks of the variants, and a subscriber without a display
-- name, user 3; an image of user 2's attached to Hello world with a
-- caption, alt text, description and a comment; a PDF of the
-- administrator's on Hello world; a file without an extension of user 3's
-- attached to a private post of user 2's; an image without a slug whose
-- author is gone, attached to an untitled draft; an SVG image; and a CSV
-- file of the administrator's attached to their pending post; and the
-- administrator's images in the Trash, private, scaled from a larger
-- original, and attached to a trashed draft and a trashed post without its
-- earlier status.
INSERT INTO wp_users (ID, user_login, user_pass, user_nicename, user_email, user_url, user_registered,
  user_activation_key, user_status, display_name) VALUES
  (2, 'locker', '', 'locker', 'locker@example.test', '', '2026-11-01 00:00:00', '', 0, 'Lock &amp; Holder'),
  (3, 'sub', '', 'sub', 'sub@example.test', '', '2026-11-01 00:00:00', '', 0, '');
INSERT INTO wp_usermeta (umeta_id, user_id, meta_key, meta_value) VALUES
  (801, 2, 'nickname', 'locker'), (802, 2, 'wp_capabilities', 'a:1:{s:6:"editor";b:1;}'), (803, 2, 'wp_user_level', '7'),
  (804, 3, 'nickname', 'sub'), (805, 3, 'wp_capabilities', 'a:1:{s:10:"subscriber";b:1;}'), (806, 3, 'wp_user_level', '0');
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (801, 2, '2026-03-04 05:06:07', '2026-03-04 05:06:07', 'Description &amp; <b>bold</b> "quoted"', 'Sunset "dusk" &amp; sea',
    'A caption with <em>markup</em> &amp; more', 'inherit', 'open', 'open', '', 'sunset-dusk-sea', '', '',
    '2026-03-04 05:06:07', '2026-03-04 05:06:07', '', 1, 'http://default-install.test/wp-content/uploads/2026/03/sunset.jpg',
    0, 'attachment', 'image/jpeg', 1),
  (802, 1, '2025-12-31 23:59:00', '2025-12-31 23:59:00', '', 'Report', '', 'inherit', 'closed', 'closed', '', 'report', '',
    '', '2025-12-31 23:59:00', '2025-12-31 23:59:00', '', 1, 'http://default-install.test/wp-content/uploads/2025/12/report.pdf',
    0, 'attachment', 'application/pdf', 0),
  (803, 3, '2025-12-01 08:00:00', '2025-12-01 08:00:00', '', 'Notes', '', 'inherit', 'open', 'closed', '', 'notes', '',
    '', '2025-12-01 08:00:00', '2025-12-01 08:00:00', '', 804, 'http://default-install.test/wp-content/uploads/2025/12/notes',
    0, 'attachment', 'text/plain', 0),
  (804, 2, '2025-12-01 07:00:00', '2025-12-01 07:00:00', 'Hidden', 'Secret &amp; plans', '', 'private', 'open', 'open', '',
    'secret-plans', '', '', '2025-12-01 07:00:00', '2025-12-01 07:00:00', '', 0, 'http://default-install.test/?p=804',
    0, 'post', '', 0),
  (805, 99, '2024-01-02 03:04:05', '2024-01-02 03:04:05', '', '', '', 'inherit', 'closed', 'closed', '', '', '',
    '', '2024-01-02 03:04:05', '2024-01-02 03:04:05', '', 806, 'http://default-install.test/wp-content/uploads/2024/01/dot.png',
    0, 'attachment', 'image/png', 0),
  (806, 1, '2024-01-01 00:00:00', '0000-00-00 00:00:00', '', '', '', 'draft', 'open', 'open', '', '', '', '',
    '2024-01-01 00:00:00', '2024-01-01 00:00:00', '', 0, 'http://default-install.test/?p=806', 0, 'post', '', 0),
  (807, 1, '2024-02-03 04:05:06', '2024-02-03 04:05:06', '', 'Logo', '', 'inherit', 'open', 'closed', '', 'logo', '', '',
    '2024-02-03 04:05:06', '2024-02-03 04:05:06', '', 0, 'http://default-install.test/wp-content/uploads/2024/02/logo.svg',
    0, 'attachment', 'image/svg+xml', 0),
  (808, 1, '2024-03-04 05:06:07', '2024-03-04 05:06:07', '', 'Data', '', 'inherit', 'open', 'open', '', 'data', '', '',
    '2024-03-04 05:06:07', '2024-03-04 05:06:07', '', 809, 'http://default-install.test/wp-content/uploads/2024/03/data.csv',
    0, 'attachment', 'text/csv', 0),
  (809, 1, '2024-03-04 05:00:00', '0000-00-00 00:00:00', '', 'Waiting', '', 'pending', 'open', 'open', '', '', '', '',
    '2024-03-04 05:00:00', '2024-03-04 05:00:00', '', 0, 'http://default-install.test/?p=809', 0, 'post', '', 0),
  (810, 1, '2026-04-01 10:00:00', '2026-04-01 10:00:00', '', 'Gone', '', 'trash', 'open', 'closed', '', 'gone', '', '',
    '2026-04-01 10:00:00', '2026-04-01 10:00:00', '', 0, 'http://default-install.test/wp-content/uploads/2026/04/gone.png',
    0, 'attachment', 'image/png', 0),
  (811, 1, '2026-04-01 10:00:00', '2026-04-01 10:00:00', '', 'Private shot', '', 'private', 'open', 'closed', '', 'private-shot',
    '', '', '2026-04-01 10:00:00', '2026-04-01 10:00:00', '', 0,
    'http://default-install.test/wp-content/uploads/2026/04/shot.png', 0, 'attachment', 'image/png', 0),
  (812, 1, '2026-04-01 10:00:00', '2026-04-01 10:00:00', '', 'Big', '', 'inherit', 'open', 'closed', '', 'big', '', '',
    '2026-04-01 10:00:00', '2026-04-01 10:00:00', '', 0, 'http://default-install.test/wp-content/uploads/2026/05/big.jpg',
    0, 'attachment', 'image/jpeg', 0),
  (813, 1, '2026-04-01 10:00:00', '2026-04-01 10:00:00', '', 'Binned draft image', '', 'inherit', 'open', 'closed', '', 'binned',
    '', '', '2026-04-01 10:00:00', '2026-04-01 10:00:00', '', 814,
    'http://default-install.test/wp-content/uploads/2026/05/binned.png', 0, 'attachment', 'image/png', 0),
  (814, 1, '2026-05-02 09:00:00', '2026-05-02 09:00:00', '', 'Binned', '', 'trash', 'open', 'open', '', '__trashed', '', '',
    '2026-05-02 09:00:00', '2026-05-02 09:00:00', '', 0, 'http://default-install.test/?p=814', 0, 'post', '', 0),
  (815, 1, '2026-04-01 10:00:00', '2026-04-01 10:00:00', '', 'Binned post image', '', 'inherit', 'open', 'closed', '', 'binned-2',
    '', '', '2026-04-01 10:00:00', '2026-04-01 10:00:00', '', 816,
    'http://default-install.test/wp-content/uploads/2026/05/binned-2.png', 0, 'attachment', 'image/png', 0),
  (816, 1, '2026-05-02 09:00:00', '2026-05-02 09:00:00', '', 'Binned', '', 'trash', 'open', 'open', '', '__trashed-2', '', '',
    '2026-05-02 09:00:00', '2026-05-02 09:00:00', '', 0, 'http://default-install.test/?p=816', 0, 'post', '', 0);
INSERT INTO wp_postmeta (meta_id, post_id, meta_key, meta_value) VALUES
  (801, 801, '_wp_attached_file', '2026/03/sunset.jpg'),
  (802, 801, '_wp_attachment_metadata', 'a:6:{s:5:"width";i:1200;s:6:"height";i:600;s:4:"file";s:18:"2026/03/sunset.jpg";s:8:"filesize";i:345678;s:5:"sizes";a:4:{s:6:"medium";a:5:{s:4:"file";s:18:"sunset-300x150.jpg";s:5:"width";i:300;s:6:"height";i:150;s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:12345;}s:5:"large";a:5:{s:4:"file";s:19:"sunset-1024x512.jpg";s:5:"width";i:1024;s:6:"height";i:512;s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:123456;}s:9:"thumbnail";a:5:{s:4:"file";s:18:"sunset-150x150.jpg";s:5:"width";i:150;s:6:"height";i:150;s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:5432;}s:12:"medium_large";a:5:{s:4:"file";s:18:"sunset-768x384.jpg";s:5:"width";i:768;s:6:"height";i:384;s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:65432;}}s:10:"image_meta";a:0:{}}'),
  (803, 801, '_wp_attachment_image_alt', 'Alt "text" &amp; more'),
  (804, 802, '_wp_attached_file', '2025/12/report.pdf'),
  (805, 802, '_wp_attachment_metadata', 'a:2:{s:8:"filesize";i:1536000;s:9:"mime_type";s:17:"application/x-pdf";}'),
  (806, 803, '_wp_attached_file', '2025/12/notes'),
  (807, 803, '_wp_attachment_metadata', 'a:1:{s:8:"filesize";i:512;}'),
  (808, 805, '_wp_attached_file', '2024/01/dot.png'),
  (809, 805, '_wp_attachment_metadata', 'a:6:{s:5:"width";i:40;s:6:"height";i:30;s:4:"file";s:15:"2024/01/dot.png";s:8:"filesize";i:2048;s:5:"sizes";a:0:{}s:10:"image_meta";a:0:{}}'),
  (810, 807, '_wp_attached_file', '2024/02/logo.svg'),
  (811, 807, '_wp_attachment_metadata', 'a:1:{s:8:"filesize";i:3072;}'),
  (812, 808, '_wp_attached_file', '2024/03/data.csv'),
  (813, 808, '_wp_attachment_metadata', 'a:1:{s:8:"filesize";i:100;}'),
  (814, 810, '_wp_attached_file', '2026/04/gone.png'),
  (815, 810, '_wp_attachment_metadata', 'a:6:{s:5:"width";i:10;s:6:"height";i:10;s:4:"file";s:16:"2026/04/gone.png";s:8:"filesize";i:100;s:5:"sizes";a:0:{}s:10:"image_meta";a:0:{}}'),
  (816, 811, '_wp_attached_file', '2026/04/shot.png'),
  (817, 811, '_wp_attachment_metadata', 'a:6:{s:5:"width";i:10;s:6:"height";i:10;s:4:"file";s:16:"2026/04/shot.png";s:8:"filesize";i:100;s:5:"sizes";a:0:{}s:10:"image_meta";a:0:{}}'),
  (818, 812, '_wp_attached_file', '2026/05/big-scaled.jpg'),
  (819, 812, '_wp_attachment_metadata', 'a:7:{s:5:"width";i:2560;s:6:"height";i:1280;s:4:"file";s:22:"2026/05/big-scaled.jpg";s:8:"filesize";i:500000;s:5:"sizes";a:1:{s:9:"thumbnail";a:5:{s:4:"file";s:15:"big-150x150.jpg";s:5:"width";i:150;s:6:"height";i:150;s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:4000;}}s:10:"image_meta";a:0:{}s:14:"original_image";s:7:"big.jpg";}'),
  (820, 813, '_wp_attached_file', '2026/05/binned.png'),
  (821, 813, '_wp_attachment_metadata', 'a:6:{s:5:"width";i:10;s:6:"height";i:10;s:4:"file";s:18:"2026/05/binned.png";s:8:"filesize";i:100;s:5:"sizes";a:0:{}s:10:"image_meta";a:0:{}}'),
  (822, 814, '_wp_trash_meta_status', 'draft'),
  (823, 815, '_wp_attached_file', '2026/05/binned-2.png'),
  (824, 815, '_wp_attachment_metadata', 'a:6:{s:5:"width";i:10;s:6:"height";i:10;s:4:"file";s:20:"2026/05/binned-2.png";s:8:"filesize";i:100;s:5:"sizes";a:0:{}s:10:"image_meta";a:0:{}}');
INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, comment_author_email, comment_author_url, comment_author_IP,
  comment_date, comment_date_gmt, comment_content, comment_karma, comment_approved, comment_agent, comment_type, comment_parent, user_id) VALUES
  (801, 801, 'Visitor', 'visitor@example.test', '', '', '2026-03-05 10:00:00', '2026-03-05 10:00:00', 'Nice', 0, '1', '', 'comment', 0, 0);
-- The j9 variants' media: a song of user 2's on Hello world with its tags
-- and a featured image, a video of the administrator's with a poster, a
-- QuickTime movie and an AAC recording no player plays (the recording's
-- featured image is gone), a small WebM video, and two images for pretty
-- permalinks: one whose slug Hello world already has, attached to Sample
-- Page, and one with a long encoded slug.
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (817, 2, '2026-06-01 12:00:00', '2026-06-01 12:00:00', 'Lyrics', 'Song &amp; dance', 'Sung', 'inherit', 'open', 'closed', '',
    'song-dance', '', '', '2026-06-01 12:00:00', '2026-06-01 12:00:00', '', 1,
    'http://default-install.test/wp-content/uploads/2026/06/song.mp3', 0, 'attachment', 'audio/mpeg', 0),
  (818, 1, '2026-06-02 12:00:00', '2026-06-02 12:00:00', '', 'Clip', '', 'inherit', 'open', 'closed', '', 'clip', '', '',
    '2026-06-02 12:00:00', '2026-06-02 12:00:00', '', 0, 'http://default-install.test/wp-content/uploads/2026/06/clip.mp4',
    0, 'attachment', 'video/mp4', 0),
  (819, 1, '2026-06-03 12:00:00', '2026-06-03 12:00:00', '', 'Movie', '', 'inherit', 'open', 'closed', '', 'movie', '', '',
    '2026-06-03 12:00:00', '2026-06-03 12:00:00', '', 0, 'http://default-install.test/wp-content/uploads/2026/06/movie.mov',
    0, 'attachment', 'video/quicktime', 0),
  (820, 1, '2026-06-04 12:00:00', '2026-06-04 12:00:00', '', 'Voice', '', 'inherit', 'open', 'closed', '', 'voice', '', '',
    '2026-06-04 12:00:00', '2026-06-04 12:00:00', '', 0, 'http://default-install.test/wp-content/uploads/2026/06/voice.aac',
    0, 'attachment', 'audio/aac', 0),
  (821, 1, '2026-06-05 12:00:00', '2026-06-05 12:00:00', '', 'Small', '', 'inherit', 'open', 'closed', '', 'small', '', '',
    '2026-06-05 12:00:00', '2026-06-05 12:00:00', '', 0, 'http://default-install.test/wp-content/uploads/2026/06/small.webm',
    0, 'attachment', 'video/webm', 0),
  (822, 1, '2026-06-06 12:00:00', '2026-06-06 12:00:00', '', 'Hello', '', 'inherit', 'open', 'closed', '', 'hello-world', '',
    '', '2026-06-06 12:00:00', '2026-06-06 12:00:00', '', 2, 'http://default-install.test/wp-content/uploads/2026/06/hello.png',
    0, 'attachment', 'image/png', 0),
  (823, 1, '2026-06-07 12:00:00', '2026-06-07 12:00:00', '', 'Cafe', '', 'inherit', 'open', 'closed', '',
    'a-rather-long-attachment-name-for-caf%c3%a9-photos', '', '', '2026-06-07 12:00:00', '2026-06-07 12:00:00', '', 1,
    'http://default-install.test/wp-content/uploads/2026/06/cafe.png', 0, 'attachment', 'image/png', 0);
INSERT INTO wp_postmeta (meta_id, post_id, meta_key, meta_value) VALUES
  (825, 817, '_wp_attached_file', '2026/06/song.mp3'),
  (826, 817, '_wp_attachment_metadata', 'a:12:{s:8:"filesize";i:4200000;s:9:"mime_type";s:10:"audio/mpeg";s:6:"length";i:215;s:16:"length_formatted";s:4:"3:35";s:7:"bitrate";d:128456.5;s:12:"bitrate_mode";s:3:"cbr";s:6:"artist";s:19:"The "Band" &amp; Co";s:5:"album";s:5:"First";s:4:"year";s:4:"2026";s:5:"genre";s:4:"Rock";s:10:"dataformat";s:3:"mp3";s:5:"audio";a:3:{s:10:"dataformat";s:3:"mp3";s:5:"codec";s:4:"LAME";s:11:"sample_rate";i:44100;}}'),
  (827, 817, '_thumbnail_id', '801'),
  (828, 818, '_wp_attached_file', '2026/06/clip.mp4'),
  (829, 818, '_wp_attachment_metadata', 'a:9:{s:8:"filesize";i:9000000;s:9:"mime_type";s:9:"video/mp4";s:6:"length";i:3723;s:16:"length_formatted";s:7:"1:02:03";s:5:"width";i:1280;s:6:"height";i:720;s:10:"fileformat";s:3:"mp4";s:7:"bitrate";i:2000000;s:5:"audio";a:2:{s:10:"dataformat";s:3:"mp4";s:5:"codec";s:19:"ISO/IEC 14496-3 AAC";}}'),
  (830, 818, '_thumbnail_id', '805'),
  (831, 819, '_wp_attached_file', '2026/06/movie.mov'),
  (832, 819, '_wp_attachment_metadata', 'a:2:{s:8:"filesize";i:1000;s:16:"length_formatted";s:2:"59";}'),
  (833, 820, '_wp_attached_file', '2026/06/voice.aac'),
  (834, 820, '_wp_attachment_metadata', 'a:3:{s:8:"filesize";i:2048;s:16:"length_formatted";s:4:"0:01";s:6:"artist";s:0:"";}'),
  (835, 820, '_thumbnail_id', '9999'),
  (836, 821, '_wp_attached_file', '2026/06/small.webm'),
  (837, 821, '_wp_attachment_metadata', 'a:4:{s:8:"filesize";i:500;s:5:"width";i:320;s:6:"height";i:240;s:16:"length_formatted";s:7:"1:00:01";}'),
  (838, 822, '_wp_attached_file', '2026/06/hello.png'),
  (839, 822, '_wp_attachment_metadata', 'a:6:{s:5:"width";i:10;s:6:"height";i:10;s:4:"file";s:17:"2026/06/hello.png";s:8:"filesize";i:100;s:5:"sizes";a:0:{}s:10:"image_meta";a:0:{}}'),
  (840, 823, '_wp_attached_file', '2026/06/cafe.png'),
  (841, 823, '_wp_attachment_metadata', 'a:6:{s:5:"width";i:10;s:6:"height";i:10;s:4:"file";s:16:"2026/06/cafe.png";s:8:"filesize";i:100;s:5:"sizes";a:0:{}s:10:"image_meta";a:0:{}}');
