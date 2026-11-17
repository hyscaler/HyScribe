-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The link previews' cache for reference/rest/url-details.list: a page
-- cached until 2100 for a path the test site answers 404, and one whose
-- timeout has passed, which the next request deletes and fetches again.
INSERT INTO wp_options (option_name, option_value, autoload) VALUES
('_site_transient_timeout_g_url_details_response_68fb6b339d66b07e2b438f2f08e7ab24', '4102444800', 'off'),
('_site_transient_g_url_details_response_68fb6b339d66b07e2b438f2f08e7ab24', '<html><head><title>Cached &#039;page&#039;</title><meta property="og:image" content="cached.png"></head></html>', 'off'),
('_site_transient_timeout_g_url_details_response_a752d9f881141719d6ebb4aa1fd73009', '1', 'off'),
('_site_transient_g_url_details_response_a752d9f881141719d6ebb4aa1fd73009', '<title>Stale</title>', 'off');
-- cleanup
DELETE FROM wp_options WHERE option_name LIKE '\_site\_transient\_%g\_url\_details\_response\_%';
