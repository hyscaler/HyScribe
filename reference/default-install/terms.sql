-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Terms the default install lacks, for the term archive tests: a tag on
-- the first post, a tag on no post, and the first post's aside format.
-- db.sh loads it into the wordpress_terms database; reference/head/golden.sh
-- adds it to the PHP oracle's database while it records term pages.
INSERT INTO wp_terms (term_id, name, slug, term_group) VALUES
  (2, 'Foo', 'foo', 0),
  (3, 'post-format-aside', 'post-format-aside', 0),
  (4, 'Empty', 'empty', 0);
INSERT INTO wp_term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, parent, count) VALUES
  (2, 2, 'post_tag', 'Posts about "foo".', 0, 1),
  (3, 3, 'post_format', '', 0, 1),
  (4, 4, 'post_tag', '', 0, 0);
INSERT INTO wp_term_relationships (object_id, term_taxonomy_id, term_order) VALUES
  (1, 2, 0),
  (1, 3, 0);
