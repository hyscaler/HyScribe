-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The state reference/rest/golden.sh sets for reference/rest/theme-writes.list: the next
-- post ID, so the global styles post the reads create has the same ID in PHP's database,
-- which a navigation post of its page views has taken IDs in, as in the tests'.
ALTER TABLE wp_posts AUTO_INCREMENT = 7001;
