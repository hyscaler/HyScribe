// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_config::Mode;
use hyscribe_testkit::fixture_database;

use super::*;

/// A Settings screen's view fails at the statement that fails while it
/// deletes the notices it showed, and so writes nothing.
#[tokio::test]
async fn a_shown_notice_fails_with_its_statement() {
    let writer = Writer::open(&fixture_database(), Mode::Primary).await;
    let writer = writer.unwrap().unwrap();
    let view = View {
        synced: Synced::default(),
        welcome: None,
        draft: false,
        settings: true,
    };
    let ctx = context(1, "http://127.0.0.1:8081", false);
    for budget in 0.. {
        let mut write = writer.begin().await;
        if let Ok(write) = &mut write {
            write.fail_after(budget);
        }
        match written(write, &ctx, &view, true).await {
            Err(Error::Db(_)) => {}
            written => {
                assert!(written.is_ok());
                break;
            }
        }
    }
}
