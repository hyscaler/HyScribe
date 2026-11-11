<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints icon blocks. Lines are
//   hex(markup)<TAB>hex(html)
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$markups = array(
	'<!-- wp:icon /-->',
	'<!-- wp:icon {"icon":""} /-->',
	'<!-- wp:icon {"icon":"core/missing"} /-->',
	'<!-- wp:icon {"icon":"arrow-down"} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down"} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down","ariaLabel":"Down <b>here</b>"} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down","ariaLabel":"0"} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down","textColor":"accent-1","backgroundColor":"base"} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down","style":{"color":{"text":"#f00","background":"#00f"}}} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down","textColor":"accent-1","style":{"color":{"text":"#f00"}}} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down","borderColor":"contrast","style":{"border":{"radius":"4px","style":"dashed","width":"2px"}}} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down","style":{"border":{"color":"#0f0","radius":{"topLeft":"3px","bottomRight":"5px"},"top":{"color":"#111","width":"1px","style":"solid"},"left":{"width":"4px"}}}} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down","style":{"spacing":{"padding":{"top":"2px","left":"var:preset|spacing|30"},"margin":{"top":"9px"}},"dimensions":{"width":"48px"}}} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down","style":{"spacing":{"padding":"8px"}}} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down","flipHorizontal":true,"flipVertical":true,"rotation":90} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down","rotation":-45.7,"style":{"color":{"text":"#f00"}}} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down","rotation":"30","flipVertical":"yes"} /-->',
	'<!-- wp:icon {"icon":"core/arrow-down","align":"center","className":"x y","anchor":"z","fontSize":"small"} /-->',
);
foreach ( glob( ABSPATH . WPINC . '/images/icon-library/*.svg' ) as $file ) {
	$markups[] = '<!-- wp:icon {"icon":"core/' . basename( $file, '.svg' ) . '","ariaLabel":"L"} /-->';
}
foreach ( $markups as $markup ) {
	echo bin2hex( $markup ), "\t", bin2hex( render_block( parse_blocks( $markup )[0] ) ), "\n";
}
