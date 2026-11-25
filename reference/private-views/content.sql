-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Rows reference/private-views/golden.sh adds over reference/rest/users.sql's
-- users: posts and pages of each role in every status a single view can
-- ask for, autosaves of two published posts and a draft, and a revision,
-- an attachment and an autosave each named or placed to tell the
-- autosave lookup apart, and a published post whose name starts with a
-- private one's, which a 404 for it is sent to, as WordPress stores them;
-- pages in the statuses only posts had, an author's trashed post, a
-- subscriber's private post, a private post and a draft with an approved
-- comment each (their comment feeds), a published page whose query block
-- lists posts, and an attachment of a private post.
-- Comments and pings are closed, and no user shows the toolbar on the site
-- (each one's profile setting), as HyScribe prints none on the front end.
UPDATE wp_usermeta SET meta_value = 'false' WHERE user_id = 1 AND meta_key = 'show_admin_bar_front';
INSERT INTO wp_usermeta (user_id, meta_key, meta_value) VALUES
  (701, 'show_admin_bar_front', 'false'), (702, 'show_admin_bar_front', 'false'),
  (703, 'show_admin_bar_front', 'false'), (704, 'show_admin_bar_front', 'false');
INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status,
  comment_status, ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt,
  post_content_filtered, post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES
  (901, 1, '2026-11-02 10:00:00', '0000-00-00 00:00:00', '<!-- wp:paragraph -->\n<p>Admin draft words</p>\n<!-- /wp:paragraph -->', 'Admin draft', '', 'draft', 'closed', 'closed', '', '', '', '',
   '2026-11-02 10:00:00', '2026-11-02 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=901', 0, 'post', '', 0),
  (902, 1, '2026-11-02 11:00:00', '0000-00-00 00:00:00', '<!-- wp:paragraph -->\n<p>Admin pending words</p>\n<!-- /wp:paragraph -->', 'Admin pending', '', 'pending', 'closed', 'closed', '', '', '', '',
   '2026-11-02 11:00:00', '2026-11-02 11:00:00', '', 0, 'http://127.0.0.1:8081/?p=902', 0, 'post', '', 0),
  (903, 1, '2030-01-01 10:00:00', '2030-01-01 10:00:00', '<!-- wp:paragraph -->\n<p>Admin future words</p>\n<!-- /wp:paragraph -->', 'Admin future', '', 'future', 'closed', 'closed', '', 'admin-future', '', '',
   '2030-01-01 10:00:00', '2030-01-01 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=903', 0, 'post', '', 0),
  (904, 1, '2026-11-02 12:00:00', '2026-11-02 12:00:00', '<!-- wp:paragraph -->\n<p>Admin private words</p>\n<!-- /wp:paragraph -->', 'Admin private', '', 'private', 'closed', 'closed', '', 'admin-private', '', '',
   '2026-11-02 12:00:00', '2026-11-02 12:00:00', '', 0, 'http://127.0.0.1:8081/?p=904', 0, 'post', '', 0),
  (905, 1, '2026-11-02 13:00:00', '2026-11-02 13:00:00', '<!-- wp:paragraph -->\n<p>Admin trashed words</p>\n<!-- /wp:paragraph -->', 'Admin trashed', '', 'trash', 'closed', 'closed', '', 'admin-trashed__trashed', '', '',
   '2026-11-02 13:00:00', '2026-11-02 13:00:00', '', 0, 'http://127.0.0.1:8081/?p=905', 0, 'post', '', 0),
  (906, 1, '2026-11-02 14:00:00', '0000-00-00 00:00:00', '<!-- wp:paragraph -->\n<p>Auto Draft words</p>\n<!-- /wp:paragraph -->', 'Auto Draft', '', 'auto-draft', 'closed', 'closed', '', '', '', '',
   '2026-11-02 14:00:00', '2026-11-02 14:00:00', '', 0, 'http://127.0.0.1:8081/?p=906', 0, 'post', '', 0),
  (907, 1, '2026-11-02 15:00:00', '2026-11-02 15:00:00', '<!-- wp:paragraph -->\n<p>Admin locked words</p>\n<!-- /wp:paragraph -->', 'Admin locked', '', 'publish', 'closed', 'closed', 'secret', 'admin-locked', '', '',
   '2026-11-02 15:00:00', '2026-11-02 15:00:00', '', 0, 'http://127.0.0.1:8081/?p=907', 0, 'post', '', 0),
  (908, 1, '2026-11-02 16:00:00', '2026-11-02 16:00:00', '<!-- wp:paragraph -->\n<p>Admin published words</p>\n<!-- /wp:paragraph -->', 'Admin published', '', 'publish', 'closed', 'closed', '', 'admin-published', '', '',
   '2026-11-02 16:00:00', '2026-11-02 16:00:00', '', 0, 'http://127.0.0.1:8081/?p=908', 0, 'post', '', 0),
  (909, 1, '2026-11-02 17:00:00', '2026-11-02 17:00:00', '<!-- wp:paragraph -->\n<p>Admin autosaved words</p>\n<!-- /wp:paragraph -->', 'Admin autosaved', '', 'inherit', 'closed', 'closed', '', '908-autosave-v1', '', '',
   '2026-11-02 17:00:00', '2026-11-02 17:00:00', '', 908, 'http://127.0.0.1:8081/?p=909', 0, 'revision', '', 0),
  (912, 702, '2026-11-03 10:00:00', '2026-11-03 10:00:00', '<!-- wp:paragraph -->\n<p>Author published words</p>\n<!-- /wp:paragraph -->', 'Author published', '', 'publish', 'closed', 'closed', '', 'author-published', '', '',
   '2026-11-03 10:00:00', '2026-11-03 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=912', 0, 'post', '', 0),
  (913, 702, '2026-11-03 11:00:00', '2026-11-03 11:00:00', '<!-- wp:paragraph -->\n<p>Author autosaved words</p>\n<!-- /wp:paragraph -->', 'Author autosaved', '', 'inherit', 'closed', 'closed', '', '912-autosave-v1', '', '',
   '2026-11-03 11:00:00', '2026-11-03 11:00:00', '', 912, 'http://127.0.0.1:8081/?p=913', 0, 'revision', '', 0),
  (914, 702, '2026-11-03 12:00:00', '0000-00-00 00:00:00', '<!-- wp:paragraph -->\n<p>Author draft words</p>\n<!-- /wp:paragraph -->', 'Author draft', '', 'draft', 'closed', 'closed', '', '', '', '',
   '2026-11-03 12:00:00', '2026-11-03 12:00:00', '', 0, 'http://127.0.0.1:8081/?p=914', 0, 'post', '', 0),
  (915, 702, '2026-11-03 13:00:00', '0000-00-00 00:00:00', '<!-- wp:paragraph -->\n<p>Author pending words</p>\n<!-- /wp:paragraph -->', 'Author pending', '', 'pending', 'closed', 'closed', '', '', '', '',
   '2026-11-03 13:00:00', '2026-11-03 13:00:00', '', 0, 'http://127.0.0.1:8081/?p=915', 0, 'post', '', 0),
  (916, 702, '2026-11-03 14:00:00', '2026-11-03 14:00:00', '<!-- wp:paragraph -->\n<p>Author private words</p>\n<!-- /wp:paragraph -->', 'Author private', '', 'private', 'closed', 'closed', '', 'author-private', '', '',
   '2026-11-03 14:00:00', '2026-11-03 14:00:00', '', 0, 'http://127.0.0.1:8081/?p=916', 0, 'post', '', 0),
  (917, 702, '2030-01-02 10:00:00', '2030-01-02 10:00:00', '<!-- wp:paragraph -->\n<p>Author future words</p>\n<!-- /wp:paragraph -->', 'Author future', '', 'future', 'closed', 'closed', '', 'author-future', '', '',
   '2030-01-02 10:00:00', '2030-01-02 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=917', 0, 'post', '', 0),
  (918, 703, '2026-11-04 10:00:00', '0000-00-00 00:00:00', '<!-- wp:paragraph -->\n<p>Contributor draft words</p>\n<!-- /wp:paragraph -->', 'Contributor draft', '', 'draft', 'closed', 'closed', '', '', '', '',
   '2026-11-04 10:00:00', '2026-11-04 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=918', 0, 'post', '', 0),
  (919, 703, '2026-11-04 11:00:00', '0000-00-00 00:00:00', '<!-- wp:paragraph -->\n<p>Contributor pending words</p>\n<!-- /wp:paragraph -->', 'Contributor pending', '', 'pending', 'closed', 'closed', '', '', '', '',
   '2026-11-04 11:00:00', '2026-11-04 11:00:00', '', 0, 'http://127.0.0.1:8081/?p=919', 0, 'post', '', 0),
  (920, 701, '2026-11-04 12:00:00', '2026-11-04 12:00:00', '<!-- wp:paragraph -->\n<p>Editor private words</p>\n<!-- /wp:paragraph -->', 'Editor private', '', 'private', 'closed', 'closed', '', 'editor-private', '', '',
   '2026-11-04 12:00:00', '2026-11-04 12:00:00', '', 0, 'http://127.0.0.1:8081/?p=920', 0, 'post', '', 0),
  (921, 1, '2026-11-04 13:00:00', '2026-11-04 13:00:00', '<!-- wp:paragraph -->\n<p>Admin private page words</p>\n<!-- /wp:paragraph -->', 'Admin private page', '', 'private', 'closed', 'closed', '', 'admin-private-page', '', '',
   '2026-11-04 13:00:00', '2026-11-04 13:00:00', '', 0, 'http://127.0.0.1:8081/?page_id=921', 0, 'page', '', 0),
  (922, 1, '2026-11-04 14:00:00', '0000-00-00 00:00:00', '<!-- wp:paragraph -->\n<p>Admin draft page words</p>\n<!-- /wp:paragraph -->', 'Admin draft page', '', 'draft', 'closed', 'closed', '', '', '', '',
   '2026-11-04 14:00:00', '2026-11-04 14:00:00', '', 0, 'http://127.0.0.1:8081/?page_id=922', 0, 'page', '', 0),
  (923, 702, '2026-11-04 15:00:00', '2026-11-04 15:00:00', '<!-- wp:paragraph -->\n<p>Author private page words</p>\n<!-- /wp:paragraph -->', 'Author private page', '', 'private', 'closed', 'closed', '', 'author-private-page', '', '',
   '2026-11-04 15:00:00', '2026-11-04 15:00:00', '', 0, 'http://127.0.0.1:8081/?page_id=923', 0, 'page', '', 0),
  (924, 702, '2026-11-04 16:00:00', '2026-11-04 16:00:00', '<!-- wp:paragraph -->\n<p>Author held words</p>\n<!-- /wp:paragraph -->', 'Author held', '', 'held', 'closed', 'closed', '', 'author-held', '', '',
   '2026-11-04 16:00:00', '2026-11-04 16:00:00', '', 0, 'http://127.0.0.1:8081/?p=924', 0, 'post', '', 0),
  (925, 1, '2026-11-04 17:00:00', '2026-11-04 17:00:00', '<!-- wp:paragraph -->\n<p>Locked private words</p>\n<!-- /wp:paragraph -->', 'Locked private', '', 'private', 'closed', 'closed', 'secret', 'locked-private', '', '',
   '2026-11-04 17:00:00', '2026-11-04 17:00:00', '', 0, 'http://127.0.0.1:8081/?p=925', 0, 'post', '', 0),
  (926, 703, '2026-11-04 18:00:00', '2026-11-04 18:00:00', '<!-- wp:paragraph -->\n<p>Contributor private words</p>\n<!-- /wp:paragraph -->', 'Contributor private', '', 'private', 'closed', 'closed', '', 'contributor-private', '', '',
   '2026-11-04 18:00:00', '2026-11-04 18:00:00', '', 0, 'http://127.0.0.1:8081/?p=926', 0, 'post', '', 0),
  (930, 1, '2026-11-02 18:00:00', '2026-11-02 18:00:00', '<!-- wp:paragraph -->\n<p>Admin revised words</p>\n<!-- /wp:paragraph -->', 'Admin revised', '', 'inherit', 'closed', 'closed', '', '908-revision-v1', '', '',
   '2026-11-02 18:00:00', '2026-11-02 18:00:00', '', 908, 'http://127.0.0.1:8081/?p=930', 0, 'revision', '', 0),
  (931, 702, '2026-11-03 12:00:00', '2026-11-03 12:00:00', '<!-- wp:paragraph -->\n<p>Stray attachment words</p>\n<!-- /wp:paragraph -->', 'Stray attachment', '', 'inherit', 'closed', 'closed', '', '912-autosave-v1', '', '',
   '2026-11-03 12:00:00', '2026-11-03 12:00:00', '', 912, 'http://127.0.0.1:8081/?p=931', 0, 'attachment', '', 0),
  (932, 702, '2026-11-03 13:00:00', '2026-11-03 13:00:00', '<!-- wp:paragraph -->\n<p>Misplaced autosave words</p>\n<!-- /wp:paragraph -->', 'Misplaced autosave', '', 'inherit', 'closed', 'closed', '', '908-autosave-v1', '', '',
   '2026-11-03 13:00:00', '2026-11-03 13:00:00', '', 912, 'http://127.0.0.1:8081/?p=932', 0, 'revision', '', 0),
  (933, 702, '2026-11-03 14:00:00', '2026-11-03 14:00:00', '<!-- wp:paragraph -->\n<p>Author draft autosaved words</p>\n<!-- /wp:paragraph -->', 'Author draft autosaved', '', 'inherit', 'closed', 'closed', '', '914-autosave-v1', '', '',
   '2026-11-03 14:00:00', '2026-11-03 14:00:00', '', 914, 'http://127.0.0.1:8081/?p=933', 0, 'revision', '', 0),
  (934, 1, '2026-10-28 10:00:00', '2026-10-28 10:00:00', '<!-- wp:paragraph -->\n<p>Admin private notes words</p>\n<!-- /wp:paragraph -->', 'Admin private notes', '', 'publish', 'closed', 'closed', '', 'admin-private-notes', '', '',
   '2026-10-28 10:00:00', '2026-10-28 10:00:00', '', 0, 'http://127.0.0.1:8081/?p=934', 0, 'post', '', 0),
  (935, 1, '2026-11-05 10:00:00', '0000-00-00 00:00:00', '<!-- wp:paragraph -->\n<p>Admin pending page words</p>\n<!-- /wp:paragraph -->', 'Admin pending page', '', 'pending', 'closed', 'closed', '', '', '', '',
   '2026-11-05 10:00:00', '2026-11-05 10:00:00', '', 0, 'http://127.0.0.1:8081/?page_id=935', 0, 'page', '', 0),
  (936, 1, '2030-01-03 10:00:00', '2030-01-03 10:00:00', '<!-- wp:paragraph -->\n<p>Admin future page words</p>\n<!-- /wp:paragraph -->', 'Admin future page', '', 'future', 'closed', 'closed', '', 'admin-future-page', '', '',
   '2030-01-03 10:00:00', '2030-01-03 10:00:00', '', 0, 'http://127.0.0.1:8081/?page_id=936', 0, 'page', '', 0),
  (937, 1, '2026-11-05 11:00:00', '2026-11-05 11:00:00', '<!-- wp:paragraph -->\n<p>Admin trashed page words</p>\n<!-- /wp:paragraph -->', 'Admin trashed page', '', 'trash', 'closed', 'closed', '', 'admin-trashed-page__trashed', '', '',
   '2026-11-05 11:00:00', '2026-11-05 11:00:00', '', 0, 'http://127.0.0.1:8081/?page_id=937', 0, 'page', '', 0),
  (938, 1, '2026-11-05 12:00:00', '0000-00-00 00:00:00', '<!-- wp:paragraph -->\n<p>Auto Draft page words</p>\n<!-- /wp:paragraph -->', 'Auto Draft', '', 'auto-draft', 'closed', 'closed', '', '', '', '',
   '2026-11-05 12:00:00', '2026-11-05 12:00:00', '', 0, 'http://127.0.0.1:8081/?page_id=938', 0, 'page', '', 0),
  (939, 702, '2026-11-05 13:00:00', '2026-11-05 13:00:00', '<!-- wp:paragraph -->\n<p>Author trashed words</p>\n<!-- /wp:paragraph -->', 'Author trashed', '', 'trash', 'closed', 'closed', '', 'author-trashed__trashed', '', '',
   '2026-11-05 13:00:00', '2026-11-05 13:00:00', '', 0, 'http://127.0.0.1:8081/?p=939', 0, 'post', '', 0),
  (940, 704, '2026-11-05 14:00:00', '2026-11-05 14:00:00', '<!-- wp:paragraph -->\n<p>Subscriber private words</p>\n<!-- /wp:paragraph -->', 'Subscriber private', '', 'private', 'closed', 'closed', '', 'subscriber-private', '', '',
   '2026-11-05 14:00:00', '2026-11-05 14:00:00', '', 0, 'http://127.0.0.1:8081/?p=940', 0, 'post', '', 0),
  (941, 1, '2026-11-05 15:00:00', '2026-11-05 15:00:00', '<!-- wp:paragraph -->\n<p>Admin discussed words</p>\n<!-- /wp:paragraph -->', 'Admin discussed', '', 'private', 'closed', 'closed', '', 'admin-discussed', '', '',
   '2026-11-05 15:00:00', '2026-11-05 15:00:00', '', 0, 'http://127.0.0.1:8081/?p=941', 0, 'post', '', 1),
  (942, 1, '2026-11-05 16:00:00', '2026-11-05 16:00:00', '<!-- wp:query {"queryId":7,"query":{"perPage":40,"pages":0,"offset":0,"postType":"post","order":"desc","orderBy":"date","inherit":false}} -->\n<div class="wp-block-query"><!-- wp:post-template -->\n<!-- wp:post-title {"isLink":true} /-->\n\n<!-- wp:post-excerpt /-->\n<!-- /wp:post-template --></div>\n<!-- /wp:query -->\n\n<!-- wp:latest-posts {"postsToShow":40} /-->', 'Listing page', '', 'publish', 'closed', 'closed', '', 'listing-page', '', '',
   '2026-11-05 16:00:00', '2026-11-05 16:00:00', '', 0, 'http://127.0.0.1:8081/?page_id=942', 0, 'page', '', 0),
  (943, 1, '2026-11-05 17:00:00', '2026-11-05 17:00:00', '<!-- wp:paragraph -->\n<p>Private attachment words</p>\n<!-- /wp:paragraph -->', 'Private attachment', '', 'inherit', 'closed', 'closed', '', 'private-attachment', '', '',
   '2026-11-05 17:00:00', '2026-11-05 17:00:00', '', 904, 'http://127.0.0.1:8081/?p=943', 0, 'attachment', 'image/png', 0),
  (944, 1, '2026-11-05 18:00:00', '0000-00-00 00:00:00', '<!-- wp:paragraph -->\n<p>Admin discussed draft words</p>\n<!-- /wp:paragraph -->', 'Admin discussed draft', '', 'draft', 'closed', 'closed', '', '', '', '',
   '2026-11-05 18:00:00', '2026-11-05 18:00:00', '', 0, 'http://127.0.0.1:8081/?p=944', 0, 'post', '', 1);
INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, comment_author_email, comment_author_url, comment_author_IP, comment_date, comment_date_gmt, comment_content, comment_karma, comment_approved, comment_agent, comment_type, comment_parent, user_id) VALUES
  (951, 941, 'Insider', 'insider@example.test', '', '', '2026-11-05 15:30:00', '2026-11-05 15:30:00', 'Said in private.', 0, '1', '', 'comment', 0, 0),
  (952, 944, 'Reviewer', 'reviewer@example.test', '', '', '2026-11-05 18:30:00', '2026-11-05 18:30:00', 'Said on a draft.', 0, '1', '', 'comment', 0, 0);
