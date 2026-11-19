<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin of reference/rest/block-patterns.list: HyScribe makes no
// calls to wordpress.org, so the oracle gives the registered patterns alone,
// as it does when the pattern directory cannot be reached
// (WP_HTTP_BLOCK_EXTERNAL), but without the site transient PHP caches a failed
// directory request in.
add_filter( 'should_load_remote_block_patterns', '__return_false' );
