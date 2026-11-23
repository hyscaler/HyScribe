// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::tests::rolled_back;

#[tokio::test]
async fn the_first_error_stops_the_action() {
    rolled_back(async |write| {
        write.fail_after(1);
        let mut steps = Steps::<'_, '_, crate::Error>::new(write);
        // One closure type, so one instance of `run` takes every path.
        let blogname = || async |w: &mut Write<'_>| w.option_value("blogname").await;
        let name = steps.run(blogname()).await;
        assert_eq!(name.unwrap(), b"HyScribe Default Install");
        assert_eq!(steps.run(blogname()).await, None);
        // Skipped, and the first error kept.
        assert_eq!(steps.run(blogname()).await, None);
        steps.keep(Err::<(), _>(crate::Error::Refused("later")));
        let err = steps.finish(Ok(())).unwrap_err();
        assert!(matches!(err, crate::Error::Db(_)), "{err:?}");
    })
    .await;
}
