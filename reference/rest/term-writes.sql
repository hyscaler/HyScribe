-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The state reference/rest/golden.sh records reference/rest/term-writes.list
-- over: the default install with a tag, `Tagged` (2), on post 1, and a
-- category under Uncategorized, `Sub` (3), as wp_insert_term() leaves
-- them; db.sh resets the database after.
INSERT INTO wp_terms (term_id, name, slug, term_group) VALUES (2, 'Tagged', 'tagged', 0), (3, 'Sub', 'sub', 0);
INSERT INTO wp_term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, parent, count) VALUES
  (2, 2, 'post_tag', '', 0, 1), (3, 3, 'category', '', 1, 0);
INSERT INTO wp_term_relationships (object_id, term_taxonomy_id, term_order) VALUES (1, 2, 0);
