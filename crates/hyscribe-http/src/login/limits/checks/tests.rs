// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::thread;

use super::*;

/// A start for the test clocks.
fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Clocks that only move when their test moves them, by milliseconds.
static MOVED: AtomicU64 = AtomicU64::new(0);
static LATE: AtomicU64 = AtomicU64::new(0);

fn moved() -> Instant {
    epoch() + Duration::from_millis(MOVED.load(Ordering::SeqCst))
}

fn late() -> Instant {
    epoch() + Duration::from_millis(LATE.load(Ordering::SeqCst))
}

/// Sets its flag when dropped, also by a failing test.
struct Done<'a>(&'a AtomicBool);

impl Drop for Done<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Once `patience` is over, and after the test until no check waits (ten
/// seconds at most), moves `clock` past every wait: a broken line fails.
fn watchdog(checks: &Checks, clock: &AtomicU64, done: &AtomicBool, patience: Duration) {
    let start = Instant::now();
    let mut ended = None;
    while ended.is_none_or(|end: Instant| checks.line().waiting > 0 && end.elapsed().as_secs() < 10)
    {
        if done.load(Ordering::SeqCst) {
            ended = ended.or(Some(Instant::now()));
        }
        if ended.is_some() || start.elapsed() > patience {
            clock.fetch_add(3_600_000, Ordering::SeqCst);
            checks.freed.notify_all();
        }
        thread::sleep(Duration::from_millis(5));
    }
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
    let done = AtomicBool::new(false);
    thread::scope(|scope| {
        scope.spawn(|| watchdog(&checks, &MOVED, &done, Duration::from_secs(60)));
        let _done = Done(&done);
        let held = [checks.start(), checks.start()];
        assert!(held.iter().all(Option::is_some));
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
        assert!(logins.into_iter().all(|login| !login.join().unwrap()));
        assert_eq!(checks.line().waiting, 0);
        drop(held);
        assert_eq!(checks.line().running, 0);
    });
}

/// Waiting logins run as checks end, at once, never more than the cap.
#[test]
fn waiting_logins_run_as_checks_end() {
    let checks = Checks::new(3, late);
    let (running, most) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let done = AtomicBool::new(false);
    thread::scope(|scope| {
        // Woken as checks end, they are done long before; else they give up.
        scope.spawn(|| watchdog(&checks, &LATE, &done, Duration::from_secs(5)));
        let _done = Done(&done);
        let held: Vec<_> = (0..3).map(|_| checks.start()).collect();
        assert!(held.iter().all(Option::is_some));
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
