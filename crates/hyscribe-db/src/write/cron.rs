// SPDX-License-Identifier: AGPL-3.0-or-later

//! The statements of WordPress's cron jobs (`wp_scheduled_delete()`,
//! `wp_delete_auto_drafts()`, `delete_expired_transients()` and
//! `_wp_personal_data_cleanup_requests()`), as PHP runs them.

use super::{Table, Write};
use crate::DbError;

impl Write<'_> {
    /// The posts whose `_wp_trash_meta_time` is before `time`, in the
    /// database's order, as `wp_scheduled_delete()` reads them.
    pub async fn posts_trashed_before(&mut self, time: i64) -> Result<Vec<u64>, DbError> {
        let query = self.sql(
            "SELECT post_id FROM {prefix}postmeta \
             WHERE meta_key = '_wp_trash_meta_time' AND meta_value < ?",
        );
        self.ids(query.bind(time)).await
    }

    /// The comments whose `_wp_trash_meta_time` is before `time`.
    pub async fn comments_trashed_before(&mut self, time: i64) -> Result<Vec<u64>, DbError> {
        let query = self.sql(
            "SELECT comment_id FROM {prefix}commentmeta \
             WHERE meta_key = '_wp_trash_meta_time' AND meta_value < ?",
        );
        self.ids(query.bind(time)).await
    }

    /// The auto-drafts dated more than seven days before `now`, read on
    /// the database's clock as PHP's `NOW()` is.
    pub async fn old_auto_draft_ids(&mut self, now: i64) -> Result<Vec<u64>, DbError> {
        let query = self.sql(
            "SELECT ID FROM {prefix}posts WHERE post_status = 'auto-draft' \
             AND DATE_SUB( FROM_UNIXTIME( ? ), INTERVAL 7 DAY ) > post_date",
        );
        self.ids(query.bind(now)).await
    }

    /// The pending privacy requests last changed before `date`, a site
    /// time, newest first as `WP_Query` gives them.
    pub async fn pending_requests_before(&mut self, date: &str) -> Result<Vec<u64>, DbError> {
        let query = self.sql(
            "SELECT ID FROM {prefix}posts WHERE post_type = 'user_request' \
             AND post_status = 'request-pending' AND post_modified < ? \
             ORDER BY post_date DESC",
        );
        self.ids(query.bind(date)).await
    }

    /// `delete_expired_transients()` of a single site: each transient and
    /// site transient whose timeout is before `now`, with its timeout.
    pub async fn delete_expired_transients(&mut self, now: i64) -> Result<(), DbError> {
        // Each kind's names, the pattern of its timeouts, and where a
        // transient's own name starts in its option's.
        for (names, timeouts, timeout, from) in [
            (
                r"\_transient\_%",
                r"\_transient\_timeout\_%",
                "_transient_timeout_",
                12,
            ),
            (
                r"\_site\_transient\_%",
                r"\_site\_transient\_timeout\_%",
                "_site_transient_timeout_",
                17,
            ),
        ] {
            let query = self.sql(
                "DELETE a, b FROM {prefix}options a, {prefix}options b \
                 WHERE a.option_name LIKE ? AND a.option_name NOT LIKE ? \
                 AND b.option_name = CONCAT( ?, SUBSTRING( a.option_name, ? ) ) \
                 AND b.option_value < ?",
            );
            let query = query.bind(names).bind(timeouts).bind(timeout).bind(from);
            self.change(Table::Options, query.bind(now)).await?;
        }
        Ok(())
    }
}
