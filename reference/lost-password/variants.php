<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/lost-password/golden.sh: every mail is
// taken as sent, as HyScribe answers until its transport sends them.
add_filter( 'pre_wp_mail', '__return_true' );
