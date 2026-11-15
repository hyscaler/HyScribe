// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

/// Outside the loop every image is lazy; in it the first three are not,
/// and the first large one among them takes the high priority.
#[test]
fn counts_images_in_the_loop() {
    let outside = Loading::default();
    for _ in 0..4 {
        assert_eq!(outside.img(500, 500).loading, Some("lazy"));
    }
    let content = Loading {
        in_the_content: true,
        ..Loading::default()
    };
    assert_eq!(content.img(500, 500), Optimization::default());
    let looped = Loading {
        in_the_loop: true,
        ..Loading::default()
    };
    let small = looped.img(50, 50);
    assert_eq!(
        (small.loading, small.fetchpriority, small.decoding),
        (None, None, Some("async"))
    );
    assert_eq!(looped.img(250, 200).fetchpriority, Some("high"));
    assert_eq!(
        looped.img(250, 200),
        Optimization {
            decoding: Some("async"),
            ..Optimization::default()
        }
    );
    assert_eq!(looped.img(250, 200).loading, Some("lazy"));
    // Clones share the counts, as PHP's statics are per request.
    let fresh = Loading {
        in_the_loop: true,
        ..Loading::default()
    };
    let clone = fresh.clone();
    assert_eq!(clone.img(249, 200).fetchpriority, None);
    assert_eq!(fresh.img(250, 200).fetchpriority, Some("high"));
    assert_eq!(clone.img(50, 50).loading, None);
    assert_eq!(fresh.img(50, 50).loading, Some("lazy"));
}

/// In the viewport, a `fetchpriority` other than `high` or `auto` stays as
/// given and still counts.
#[test]
fn unknown_priorities_count() {
    let looped = Loading::in_the_loop(true);
    let given = Given {
        width: Some(500),
        height: Some(500),
        fetchpriority: Some(b"bogus"),
        ..Given::default()
    };
    let expected = Optimization {
        decoding: Some("async"),
        ..Optimization::default()
    };
    for _ in 0..3 {
        assert_eq!(looped.optimization(true, &given, Filter::Content), expected);
    }
    assert_eq!(
        looped.optimization(true, &given, Filter::Content).loading,
        Some("lazy")
    );
}
