// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::admin::saves::tests::{SITE, settled, writer};

/// A Settings screen's view fails at the statement that fails while it
/// deletes the notices it showed, and so writes nothing.
#[tokio::test]
async fn a_shown_notice_fails_with_its_statement() {
    let writer = writer().await;
    let view = View {
        synced: Synced::default(),
        welcome: None,
        draft: false,
        settings: true,
    };
    let ctx = context(1, SITE, SITE, Default::default());
    let saved = settled(async |budget| {
        let mut write = writer.begin().await;
        if let Ok(write) = &mut write {
            write.fail_after(budget);
        }
        written(write, &ctx, &view, true).await
    });
    assert!(saved.await.is_ok());
}
