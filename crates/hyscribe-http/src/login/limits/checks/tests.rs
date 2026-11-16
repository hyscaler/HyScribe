// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::thread;

use super::*;

/// A start for the test clocks.
fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// A clock that only moves when the test moves it.
static MOVED: AtomicU64 = AtomicU64::new(0);

fn moved() -> Instant {
    epoch() + Duration::from_millis(MOVED.load(Ordering::SeqCst))
}

/// A clock that runs back an hour each time it is read, so that a check
/// waits until another ends.
fn receding() -> Instant {
    static READS: AtomicU64 = AtomicU64::new(0);
    let hours = 1_000_000 - READS.fetch_add(1, Ordering::SeqCst);
    epoch() + Duration::from_secs(3600 * hours)
}

/// Until `ready` holds of the line, for at most a minute.
fn until(checks: &Checks, ready: impl Fn(&Line) -> bool) {
    let start = Instant::now();
    while !ready(&checks.line()) {
        assert!(start.elapsed() < Duration::from_secs(60), "never ready");
        thread::sleep(Duration::from_millis(5));
    }
}

/// Logins beyond the cap wait in a line of eight per check; past it, or
/// once the wait is over, theirs fail at once.
#[test]
fn many_logins_at_once_wait_their_turn_then_give_up() {
    let checks = Checks::new(2, moved);
    let held = [checks.start(), checks.start()];
    assert!(held.iter().all(Option::is_some));
    let started = thread::scope(|scope| {
        let logins: Vec<_> = (0..40)
            .map(|_| scope.spawn(|| checks.start().is_some()))
            .collect();
        until(&checks, |line| line.waiting == 16);
        // The line is full: the others fail without waiting.
        let start = Instant::now();
        while logins.iter().filter(|login| login.is_finished()).count() < 24 {
            assert!(start.elapsed() < Duration::from_secs(60), "never failed");
            thread::sleep(Duration::from_millis(5));
        }
        assert!(checks.start().is_none());
        MOVED.fetch_add(WAIT.as_millis() as u64, Ordering::SeqCst);
        checks.freed.notify_all();
        let joined = logins.into_iter().map(|login| login.join().unwrap());
        joined.filter(|started| *started).count()
    });
    assert_eq!(started, 0);
    assert_eq!(checks.line().waiting, 0);
    drop(held);
    assert_eq!(checks.line().running, 0);
}

/// Waiting logins run as checks end, never more than the cap at once.
#[test]
fn waiting_logins_run_as_checks_end() {
    let checks = Checks::new(3, receding);
    let held: Vec<_> = (0..3).map(|_| checks.start()).collect();
    let (running, most) = (AtomicUsize::new(0), AtomicUsize::new(0));
    thread::scope(|scope| {
        let logins: Vec<_> = (0..24)
            .map(|_| {
                scope.spawn(|| {
                    let check = checks.start();
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(2));
                    running.fetch_sub(1, Ordering::SeqCst);
                    check.is_some()
                })
            })
            .collect();
        until(&checks, |line| line.waiting == 24);
        drop(held);
        assert!(logins.into_iter().all(|login| login.join().unwrap()));
    });
    assert!(most.load(Ordering::SeqCst) <= 3);
    let line = checks.line();
    assert_eq!((line.running, line.waiting), (0, 0));
}

/// The cap is the machine's CPUs, and a poisoned lock still counts.
#[test]
fn the_cap_is_per_cpu() {
    let checks = Checks::per_cpu();
    let cpus = thread::available_parallelism().unwrap().get();
    let held: Vec<_> = (0..cpus).map(|_| checks.start()).collect();
    assert!(held.iter().all(Option::is_some));
    assert_eq!(checks.line().running, cpus);
    let poisoned = thread::scope(|scope| {
        scope
            .spawn(|| {
                let _line = checks.line();
                panic!("poisons the lock");
            })
            .join()
    });
    assert!(poisoned.is_err());
    drop(held);
    assert_eq!(checks.line().running, 0);
}
