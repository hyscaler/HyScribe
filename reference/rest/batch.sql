-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The state reference/rest/golden.sh records reference/rest/batch.list
-- over: the default install as the tests load it, so a new post gets the ID
-- it gets there. The navigation menu PHP's fallback creates on php.sh's
-- first page view is taken out, and the posts' counter put back;
-- db.sh resets the database after.
DELETE FROM wp_posts WHERE ID = 4 AND post_type = 'wp_navigation';
ALTER TABLE wp_posts AUTO_INCREMENT = 1;
