-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The state reference/rest/golden.sh sets for reference/rest/navigation-fallback.list: no
-- wp_navigation post (php.sh's first page view writes one the fixture lacks), and the next
-- post ID, so the post each request makes has the same ID in PHP's database and the tests'.
DELETE FROM wp_posts WHERE post_type = 'wp_navigation';
ALTER TABLE wp_posts AUTO_INCREMENT = 7001;
