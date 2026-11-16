// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use super::*;

#[test]
fn runs_jobs_and_answers() {
    let pool = Pool::new(2, 4);
    assert_eq!(pool.run(|| 6 * 7), Some(42));
    // A job that panics answers None and leaves the worker working.
    assert_eq!(pool.run(|| -> u8 { panic!("job") }), None);
    assert_eq!(pool.run(|| "after"), Some("after"));
    assert_eq!(Pool::new(0, 0).run(|| 1), Some(1));
}

#[test]
fn runs_no_more_jobs_at_once_than_workers() {
    let pool = Arc::new(Pool::new(2, 8));
    let running = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(6));
    let callers: Vec<_> = (0..6)
        .map(|_| {
            let (pool, running, most, start) = (
                Arc::clone(&pool),
                Arc::clone(&running),
                Arc::clone(&most),
                Arc::clone(&start),
            );
            thread::spawn(move || {
                start.wait();
                pool.run(move || {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    thread::sleep(std::time::Duration::from_millis(20));
                    running.fetch_sub(1, Ordering::SeqCst);
                })
            })
        })
        .collect();
    for caller in callers {
        assert_eq!(caller.join().unwrap(), Some(()));
    }
    assert_eq!(most.load(Ordering::SeqCst), 2);
}
