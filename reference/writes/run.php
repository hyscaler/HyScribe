<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Runs one action of actions.php in PHP WordPress, inside a transaction it
// rolls back, and prints the rows the action changed with generated values
// masked (docs/TESTING.md, "Write oracle"). Without an argument it lists the
// actions. hyscribe-testkit's `writes` module takes the same diff in Rust:
// the two must stay alike.

define( 'DISABLE_WP_CRON', true );
// HyScribe's safer value until the open question's .env keys exist.
define( 'DISALLOW_UNFILTERED_HTML', true );
require '/var/www/html/wp-load.php';

// A function's scope, so no variable here overwrites a WordPress global.
( function ( $argv ) {
	$actions = require __DIR__ . '/actions.php';
	if ( count( $argv ) < 2 ) {
		echo implode( "\n", array_keys( $actions ) ), "\n";
		exit;
	}
	$case = $actions[ $argv[1] ];

	// Table => [auto increment column or null, key columns, column => [table its IDs are of, whole value (true), digit runs (false) or a term map ('map')]].
	$tables = array(
		'wp_posts'              => array( 'ID', array( 'ID' ), array( 'ID' => array( 'wp_posts', true ), 'post_parent' => array( 'wp_posts', true ), 'post_author' => array( 'wp_users', true ), 'guid' => array( 'wp_posts', false ), 'post_name' => array( 'wp_posts', false ) ) ),
		'wp_postmeta'           => array( 'meta_id', array( 'meta_id' ), array( 'meta_id' => array( 'wp_postmeta', true ), 'post_id' => array( 'wp_posts', true ) ) ),
		'wp_comments'           => array( 'comment_ID', array( 'comment_ID' ), array( 'comment_ID' => array( 'wp_comments', true ), 'comment_parent' => array( 'wp_comments', true ), 'comment_post_ID' => array( 'wp_posts', true ), 'user_id' => array( 'wp_users', true ) ) ),
		'wp_commentmeta'        => array( 'meta_id', array( 'meta_id' ), array( 'meta_id' => array( 'wp_commentmeta', true ), 'comment_id' => array( 'wp_comments', true ) ) ),
		'wp_terms'              => array( 'term_id', array( 'term_id' ), array( 'term_id' => array( 'wp_terms', true ), 'slug' => array( 'wp_terms', true ) ) ),
		'wp_term_taxonomy'      => array( 'term_taxonomy_id', array( 'term_taxonomy_id' ), array( 'term_taxonomy_id' => array( 'wp_term_taxonomy', true ), 'term_id' => array( 'wp_terms', true ), 'parent' => array( 'wp_terms', true ) ) ),
		'wp_term_relationships' => array( null, array( 'object_id', 'term_taxonomy_id' ), array( 'object_id' => array( 'wp_posts', true ), 'term_taxonomy_id' => array( 'wp_term_taxonomy', true ) ) ),
		'wp_termmeta'           => array( 'meta_id', array( 'meta_id' ), array( 'meta_id' => array( 'wp_termmeta', true ), 'term_id' => array( 'wp_terms', true ) ) ),
		'wp_options'            => array( 'option_id', array( 'option_name' ), array( 'option_id' => array( 'wp_options', true ), 'option_value' => array( 'wp_terms', 'map' ) ) ),
		'wp_users'              => array( 'ID', array( 'ID' ), array( 'ID' => array( 'wp_users', true ) ) ),
		'wp_usermeta'           => array( 'umeta_id', array( 'umeta_id' ), array( 'umeta_id' => array( 'wp_usermeta', true ), 'user_id' => array( 'wp_users', true ) ) ),
		'wp_links'              => array( 'link_id', array( 'link_id' ), array( 'link_id' => array( 'wp_links', true ), 'link_owner' => array( 'wp_users', true ) ) ),
	);

	// Every row of every table, by the raw key.
	$snapshot = function () use ( $tables ) {
		global $wpdb;
		$all = array();
		foreach ( $tables as $table => list( , $key ) ) {
			$all[ $table ] = array();
			foreach ( $wpdb->get_results( "SELECT * FROM `$table`", ARRAY_A ) as $row ) {
				$all[ $table ][ implode( ',', array_map( fn( $c ) => $row[ $c ], $key ) ) ] = $row;
			}
		}
		return $all;
	};

	global $wpdb;
	wp_set_current_user( 1 );
	$wpdb->query( 'START TRANSACTION' );
	$start  = time();
	$fresh  = $snapshot();
	$state  = isset( $case['setup'] ) ? $case['setup']() : null;
	$before = $snapshot();
	$case['action']( $state );
	$after = $snapshot();
	$end   = time();
	$wpdb->query( 'ROLLBACK' );

	// New IDs in the order of their values, per table: ID => %table:n%.
	$new = array();
	foreach ( $tables as $table => list( $id ) ) {
		$new[ $table ] = array();
		if ( null === $id ) {
			continue;
		}
		$ids = array();
		$old = array_flip( array_column( $fresh[ $table ], $id ) );
		foreach ( array( $before, $after ) as $rows ) {
			foreach ( $rows[ $table ] as $row ) {
				if ( ! isset( $old[ $row[ $id ] ] ) ) {
					$ids[ $row[ $id ] ] = true;
				}
			}
		}
		$ids = array_keys( $ids );
		sort( $ids, SORT_NUMERIC );
		foreach ( $ids as $n => $value ) {
			$new[ $table ][ (string) $value ] = '%' . $table . ':' . ( $n + 1 ) . '%';
		}
	}

	// A `{$taxonomy}_children` map, a:n:{i:<parent>;a:m:{i:<n>;i:<child>;...}...},
	// with its IDs masked; any other value as it is.
	$term_map = function ( $value, $id ) {
		$parents = @unserialize( $value, array( 'allowed_classes' => false ) );
		if ( ! is_array( $parents ) ) {
			return $value;
		}
		$out = 'a:' . count( $parents ) . ':{';
		foreach ( $parents as $parent => $children ) {
			if ( ! is_int( $parent ) || ! is_array( $children ) ) {
				return $value;
			}
			$out .= 'i:' . $id( (string) $parent ) . ';a:' . count( $children ) . ':{';
			foreach ( $children as $n => $child ) {
				if ( ! is_int( $n ) || ! is_int( $child ) ) {
					return $value;
				}
				$out .= "i:$n;i:" . $id( (string) $child ) . ';';
			}
			$out .= '}';
		}
		return $out . '}';
	};

	$mask = function ( $table, $column, $value ) use ( $tables, $new, $start, $end, $term_map ) {
		if ( null === $value ) {
			return null;
		}
		// Such dates sort as text in time order.
		$is_date = preg_match( '/\A\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\z/', $value );
		if ( $is_date && $value >= gmdate( 'Y-m-d H:i:s', $start ) && $value <= gmdate( 'Y-m-d H:i:s', $end ) ) {
			return '%now%';
		}
		$value = preg_replace_callback(
			'/\d+/',
			fn( $m ) => strlen( $m[0] ) === 10 && $m[0] >= $start && $m[0] <= $end ? '%unix%' : $m[0],
			$value
		);
		if ( ! isset( $tables[ $table ][2][ $column ] ) ) {
			return $value;
		}
		list( $of, $whole ) = $tables[ $table ][2][ $column ];
		if ( 'map' === $whole ) {
			return $term_map( $value, fn( $id ) => isset( $new[ $of ][ $id ] ) ? $new[ $of ][ $id ] : $id );
		}
		if ( $whole ) {
			return isset( $new[ $of ][ $value ] ) ? $new[ $of ][ $value ] : $value;
		}
		return preg_replace_callback( '/\d+/', fn( $m ) => isset( $new[ $of ][ $m[0] ] ) ? $new[ $of ][ $m[0] ] : $m[0], $value );
	};

	$escape = function ( $value ) {
		return null === $value ? '\N' : strtr( $value, array( '\\' => '\\\\', "\t" => '\t', "\n" => '\n', "\r" => '\r' ) );
	};

	// Masked rows by masked key.
	$masked = function ( $table, $rows ) use ( $tables, $mask, $escape ) {
		$out = array();
		foreach ( $rows as $row ) {
			foreach ( $row as $column => $value ) {
				$row[ $column ] = $escape( $mask( $table, $column, $value ) );
			}
			$out[ implode( ',', array_map( fn( $c ) => $row[ $c ], $tables[ $table ][1] ) ) ] = $row;
		}
		return $out;
	};

	$lines = array();
	foreach ( $tables as $table => $spec ) {
		$old = $masked( $table, $before[ $table ] );
		$now = $masked( $table, $after[ $table ] );
		foreach ( $now as $key => $row ) {
			foreach ( $row as $column => $value ) {
				if ( ! isset( $old[ $key ] ) ) {
					$lines[] = "$table\t$key\t+\t$column\t$value";
				} elseif ( $old[ $key ][ $column ] !== $value ) {
					$lines[] = "$table\t$key\t~\t$column\t{$old[ $key ][ $column ]}\t$value";
				}
			}
		}
		foreach ( array_diff_key( $old, $now ) as $key => $row ) {
			foreach ( $row as $column => $value ) {
				$lines[] = "$table\t$key\t-\t$column\t$value";
			}
		}
	}
	if ( ! $lines ) {
		fwrite( STDERR, "{$argv[1]} wrote nothing\n" );
		exit( 1 );
	}
	sort( $lines, SORT_STRING );

	echo "# The rows PHP WordPress writes for the action {$argv[1]} (reference/writes/golden.sh).\n";
	echo '# %now% and %unix% stand for times from ', gmdate( 'Y-m-d H:i:s', $start ), ' to ', gmdate( 'Y-m-d H:i:s', $end ), " UTC.\n";
	foreach ( $new as $of => $ids ) {
		foreach ( $ids as $value => $token ) {
			echo "# $token is $value.\n";
		}
	}
	echo implode( "\n", $lines ), "\n";
} )( $argv );
