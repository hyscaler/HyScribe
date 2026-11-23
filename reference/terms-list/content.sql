-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Terms reference/terms-list/golden.sh adds and deletes after: a tree of
-- categories (one under a missing parent), described with markup and lines,
-- and six tags, one under another, as WordPress would store them.
INSERT INTO wp_terms (term_id, name, slug, term_group) VALUES
  (401, 'News', 'news', 0), (402, 'Local', 'local', 0), (403, 'City', 'city', 0), (404, 'World', 'world', 0),
  (405, 'Arts &amp; "Culture"', 'arts-culture', 0), (406, 'Zeta', 'zeta', 0), (407, 'Ünicode', '%c3%bcnicode', 0),
  (411, 'alpha', 'alpha', 0), (412, 'beta', 'beta', 0), (413, 'gamma', 'gamma', 0), (414, 'delta', 'delta', 0),
  (415, 'epsilon', 'epsilon', 0), (416, 'omega', 'omega', 0);
INSERT INTO wp_term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, parent, count) VALUES
  (401, 401, 'category', 'Daily <b>news</b> &amp; views<script>x</script>', 0, 2),
  (402, 402, 'category', '', 401, 0), (403, 403, 'category', '', 402, 1234),
  (404, 404, 'category', '', 401, 1), (405, 405, 'category', 'Line one\nLine two', 0, 0),
  (406, 406, 'category', '', 499, 0), (407, 407, 'category', '', 0, 0),
  (411, 411, 'post_tag', '', 0, 1), (412, 412, 'post_tag', 'Second', 0, 0), (413, 413, 'post_tag', '', 0, 3),
  (414, 414, 'post_tag', '', 0, 0), (415, 415, 'post_tag', '', 0, 0),
  (416, 416, 'post_tag', '', 413, 0);
-- As clean_term_cache() does when a term is added: the hierarchy is rebuilt.
DELETE FROM wp_options WHERE option_name = 'category_children';
