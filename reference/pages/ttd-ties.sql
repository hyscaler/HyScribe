-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- Terms of one name and taxonomy tie in `ORDER BY t.name`, and MariaDB
-- breaks the tie differently from one load of the fixture to the next, in
-- PHP and in HyScribe alike. The page goldens run over the Theme Test Data
-- with these two told apart, in the order of their IDs.
UPDATE wp_terms SET name = 'Foo A 2' WHERE term_id = 67;
UPDATE wp_terms SET name = 'content περιεχόμενο 2' WHERE term_id = 196;
