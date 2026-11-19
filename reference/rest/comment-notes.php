<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/rest/comment-notes.list: every request
// comes from 127.0.0.1 with no user agent, as HyScribe's tests send them,
// so the comments' addresses and agents do not depend on the network.
$_SERVER['REMOTE_ADDR'] = '127.0.0.1';
unset( $_SERVER['HTTP_USER_AGENT'] );
