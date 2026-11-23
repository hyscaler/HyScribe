-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The Theme Test Data's two RSS blocks read the feeds of make.wordpress.org
-- and wordpress.org, which pages must not depend on. The page
-- goldens run over the fixture with them pointing at the test site of
-- reference/url-details instead (feed-themes.xml, feed-news.xml), which PHP
-- and HyScribe both fetch from, HyScribe with pages.test the one host in
-- RSS_ALLOWED_HOSTS.
UPDATE wp_posts SET post_content = REPLACE(post_content, 'https://make.wordpress.org/themes/feed/', 'http://pages.test:8080/feed/themes.xml');
UPDATE wp_posts SET post_content = REPLACE(post_content, 'https://wordpress.org/news/feed/', 'http://pages.test:8080/feed/news.xml');
