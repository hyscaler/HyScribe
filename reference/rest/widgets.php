<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The must-use plugin reference/rest/golden.sh adds for
// reference/rest/widgets.list: wp_hash() signs with the salt `test-salt`, as
// the tests sign the widgets' instances.

add_filter( 'salt', static fn( $salt, $scheme ) => 'auth' === $scheme ? 'test-salt' : $salt, 10, 2 );
