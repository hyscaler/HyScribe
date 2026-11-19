-- The comments of reference/pages/comments.list: five approved comments on
-- the Hello world! post (one a reply) after the fixture's own, held ones of
-- two commenters (one a reply, one on the sample page), a spam and a
-- trashed one. Dates are fixed, and so are the IDs.
INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, comment_author_email, comment_author_url, comment_author_IP, comment_date, comment_date_gmt, comment_content, comment_karma, comment_approved, comment_agent, comment_type, comment_parent, user_id) VALUES
(101, 1, 'Ann', 'ann@example.test', '', '', '2026-10-27 09:00:00', '2026-10-27 09:00:00', 'Approved one.', 0, '1', '', 'comment', 0, 0),
(102, 1, 'Bob', 'bob@example.test', 'https://bob.example/', '', '2026-10-27 09:10:00', '2026-10-27 09:10:00', 'Approved two, a reply to one.', 0, '1', '', 'comment', 101, 0),
(103, 1, 'Cat', 'cat@example.test', '', '', '2026-10-27 09:20:00', '2026-10-27 09:20:00', 'Approved three.', 0, '1', '', 'comment', 0, 0),
(104, 1, 'Dee', 'dee@example.test', '', '', '2026-10-27 09:30:00', '2026-10-27 09:30:00', 'Approved four.', 0, '1', '', 'comment', 0, 0),
(105, 1, 'Eve', 'eve@example.test', '', '', '2026-10-27 09:40:00', '2026-10-27 09:40:00', 'Approved five.', 0, '1', '', 'comment', 0, 0),
(110, 1, 'Ann held', 'ann@example.test', 'https://ann.example/', '', '2026-10-27 09:05:00', '2026-10-27 09:05:00', 'Held <b>by</b> Ann with a <a href="https://a.example/">link</a>.', 0, '0', '', 'comment', 0, 0),
(111, 1, 'Ann held reply', 'Ann@Example.test', '', '', '2026-10-27 09:25:00', '2026-10-27 09:25:00', 'Held reply by Ann.', 0, '0', '', 'comment', 103, 0),
(112, 1, 'Bob held', 'bob@example.test', '', '', '2026-10-27 09:35:00', '2026-10-27 09:35:00', 'Held by Bob.', 0, '0', '', 'comment', 0, 0),
(113, 1, 'Ann spam', 'ann@example.test', '', '', '2026-10-27 09:15:00', '2026-10-27 09:15:00', 'Spam.', 0, 'spam', '', 'comment', 0, 0),
(114, 1, 'Ann trash', 'ann@example.test', '', '', '2026-10-27 09:12:00', '2026-10-27 09:12:00', 'Trash.', 0, 'trash', '', 'comment', 0, 0),
(115, 2, 'Ann on the page', 'ann@example.test', '', '', '2026-10-27 09:50:00', '2026-10-27 09:50:00', 'Held on the sample page.', 0, '0', '', 'comment', 0, 0);
UPDATE wp_posts SET comment_count = 6 WHERE ID = 1;
