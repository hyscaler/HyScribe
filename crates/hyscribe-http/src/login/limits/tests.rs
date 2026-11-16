// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

/// A clock reading well past the epoch, so a day before it is too.
const T: i64 = 1_800_000_000;
const WINDOW: i64 = 900;

fn limiter() -> Limiter {
    Limiter::new(LoginLimits::default())
}

fn ip(text: &str) -> Option<IpAddr> {
    Some(text.parse().unwrap())
}

/// `n` wrong guesses for `user` from `client` at `now`.
fn guess(limiter: &Limiter, client: Option<IpAddr>, user: u64, n: u32, now: i64) {
    for _ in 0..n {
        limiter.record(client, &[(user, false)], now);
    }
}

#[test]
fn five_failures_refuse_an_address_for_the_window() {
    let limiter = limiter();
    let client = ip("192.0.2.1");
    guess(&limiter, client, 1, 4, T);
    assert!(limiter.allows(client, 1, T));
    guess(&limiter, client, 1, 1, T + 10);
    // Every account, from that address only.
    assert!(!limiter.allows(client, 1, T + 10));
    assert!(!limiter.allows(client, 2, T + 10));
    assert!(limiter.allows(ip("192.0.2.2"), 1, T + 10));
    // Guesses while refused do not lengthen it.
    guess(&limiter, client, 1, 3, T + 500);
    assert!(!limiter.allows(client, 1, T + 10 + WINDOW - 1));
    assert!(limiter.allows(client, 1, T + 10 + WINDOW));
}

#[test]
fn failures_are_counted_in_their_window() {
    let limiter = limiter();
    let client = ip("192.0.2.1");
    guess(&limiter, client, 1, 4, T);
    // The window began with the first failure.
    guess(&limiter, client, 1, 1, T + WINDOW - 1);
    assert!(!limiter.allows(client, 1, T + WINDOW - 1));
    let limiter = self::limiter();
    guess(&limiter, client, 1, 4, T);
    guess(&limiter, client, 1, 4, T + WINDOW);
    assert!(limiter.allows(client, 1, T + WINDOW));
    guess(&limiter, client, 1, 1, T + WINDOW);
    assert!(!limiter.allows(client, 1, T + WINDOW));
}

#[test]
fn each_refusal_of_an_address_doubles_up_to_a_day() {
    let limiter = limiter();
    let client = ip("192.0.2.1");
    let mut now = T;
    for lockout in [
        900, 1800, 3600, 7200, 14_400, 28_800, 57_600, 86_400, 86_400,
    ] {
        guess(&limiter, client, 1, 5, now);
        assert!(!limiter.allows(client, 1, now + lockout - 1), "{lockout}");
        now += lockout;
        assert!(limiter.allows(client, 1, now), "{lockout}");
    }
    // A day without a refusal forgets them, not a moment less.
    now += MAX_LOCKOUT - 1;
    guess(&limiter, client, 1, 5, now);
    assert!(!limiter.allows(client, 1, now + MAX_LOCKOUT - 1));
    now += 2 * MAX_LOCKOUT;
    guess(&limiter, client, 1, 5, now);
    assert!(!limiter.allows(client, 1, now + WINDOW - 1));
    assert!(limiter.allows(client, 1, now + WINDOW));
}

#[test]
fn an_account_is_refused_from_addresses_it_never_logged_in_from() {
    let limiter = limiter();
    // Ten addresses, one guess each.
    for n in 0..10 {
        let client = ip(&format!("198.51.100.{n}"));
        assert!(limiter.allows(client, 1, T));
        guess(&limiter, client, 1, 1, T);
    }
    let fresh = ip("203.0.113.1");
    assert!(!limiter.allows(fresh, 1, T));
    assert!(limiter.allows(fresh, 2, T));
    assert!(limiter.allows(fresh, 1, T + WINDOW));
    // An account's refusal never grows.
    for n in 0..10 {
        guess(&limiter, ip(&format!("198.51.100.{n}")), 1, 1, T + WINDOW);
    }
    assert!(!limiter.allows(fresh, 1, T + 2 * WINDOW - 1));
    assert!(limiter.allows(fresh, 1, T + 2 * WINDOW));
}

#[test]
fn a_login_trusts_its_address_for_its_account() {
    let limiter = limiter();
    let home = ip("192.0.2.1");
    limiter.record(home, &[(2, false), (1, true)], T);
    for n in 0..10 {
        guess(&limiter, ip(&format!("198.51.100.{n}")), 1, 1, T);
    }
    assert!(limiter.allows(home, 1, T));
    assert!(!limiter.allows(ip("192.0.2.2"), 1, T));
    // A match counts no failure, not even for the account that did not.
    guess(&limiter, home, 2, 4, T);
    assert!(limiter.allows(home, 2, T));
    // Trust does not lift the address's own limit.
    guess(&limiter, home, 2, 1, T);
    assert!(!limiter.allows(home, 1, T));
}

#[test]
fn a_guess_of_no_account_counts_for_its_address() {
    let limiter = limiter();
    let client = ip("192.0.2.1");
    for _ in 0..5 {
        limiter.record(client, &[], T);
    }
    assert!(!limiter.allows(client, 1, T));
}

#[test]
fn an_ipv6_client_is_counted_by_its_64() {
    let limiter = limiter();
    for n in 1..=5 {
        guess(&limiter, ip(&format!("2001:db8::{n}")), 1, 1, T);
    }
    assert!(!limiter.allows(ip("2001:db8::ffff:1"), 1, T));
    assert!(limiter.allows(ip("2001:db8:0:1::1"), 1, T));
    // An IPv4 address mapped to IPv6 is the IPv4 one.
    guess(&limiter, ip("::ffff:192.0.2.1"), 2, 5, T);
    assert!(!limiter.allows(ip("192.0.2.1"), 3, T));
    assert!(limiter.allows(ip("::"), 3, T));
    // No address is one more.
    guess(&limiter, None, 4, 5, T);
    assert!(!limiter.allows(ip("0.0.0.0"), 3, T));
}

#[test]
fn the_state_stays_bounded_under_many_addresses() {
    let limiter = limiter();
    let attacker = ip("192.0.2.1");
    // Spent entries are only swept from a full map.
    let late = T + 2 + WINDOW + MAX_LOCKOUT;
    guess(&limiter, ip("192.0.2.9"), 9, 1, T);
    guess(&limiter, ip("192.0.2.8"), 9, 1, late);
    assert_eq!(limiter.state().addresses.len(), 2);
    let limiter = self::limiter();
    guess(&limiter, attacker, 1, 5, T);
    limiter.record(ip("192.0.2.2"), &[(1, true)], T);
    let client = |n: usize| Some(IpAddr::from([10, (n >> 16) as u8, (n >> 8) as u8, n as u8]));
    // A flood in one second: a full map drops its oldest sixteenth, and
    // everything that ties with it, but keeps a refusal.
    for n in 0..TRACKED {
        limiter.record(client(n), &[(n as u64 + 10, false)], T + 1);
    }
    {
        let state = limiter.state();
        assert_eq!(state.addresses.len(), 2);
        assert!(state.addresses.contains_key(&network(attacker)));
        assert_eq!(state.accounts.len(), 1);
    }
    assert!(!limiter.allows(attacker, 1, T + 1));
    // Logins a second apart: exactly the oldest sixteenth goes.
    for n in 0..TRACKED {
        limiter.record(client(n), &[(n as u64 + 10, true)], T + 1 + n as i64);
    }
    {
        let state = limiter.state();
        assert_eq!(state.trusted.len(), TRACKED - TRACKED / 16);
        assert!(!state.trusted.contains_key(&1));
    }
    // Spent entries go before any other.
    let limiter = self::limiter();
    for n in 0..TRACKED {
        limiter.record(client(n), &[], T);
    }
    limiter.record(attacker, &[], late);
    assert_eq!(limiter.state().addresses.len(), 1);
}

#[test]
fn an_account_is_trusted_from_its_last_eight_addresses() {
    let limiter = limiter();
    for n in 0..9 {
        limiter.record(ip(&format!("192.0.2.{n}")), &[(1, true)], T + n);
    }
    // Again from the second: it becomes the latest.
    limiter.record(ip("192.0.2.1"), &[(1, true)], T + 9);
    limiter.record(ip("192.0.2.9"), &[(1, true)], T + 10);
    for n in 0..10 {
        guess(&limiter, ip(&format!("198.51.100.{n}")), 1, 1, T + 10);
    }
    let trusted = |n: u8| limiter.allows(ip(&format!("192.0.2.{n}")), 1, T + 10);
    assert_eq!(
        (0..10).map(trusted).collect::<Vec<_>>(),
        [false, true, false, true, true, true, true, true, true, true]
    );
}

#[test]
fn an_address_checks_one_password_at_a_time() {
    let limiter = Arc::new(limiter());
    let turn = limiter.turn(ip("2001:db8::1"));
    assert!(turn.is_some());
    assert!(limiter.turn(ip("2001:db8::2")).is_none());
    assert!(limiter.turn(ip("192.0.2.1")).is_some());
    assert!(limiter.turn(ip("192.0.2.1")).is_some());
    drop(turn);
    assert!(limiter.turn(ip("2001:db8::2")).is_some());
}

#[test]
fn a_counter_is_spent_after_its_window_and_a_day_without_refusal() {
    let counter = Counter {
        failures: 1,
        since: T,
        strikes: 1,
        until: T - MAX_LOCKOUT,
    };
    assert!(!counter.spent(T + WINDOW - 1, WINDOW));
    assert!(counter.spent(T + WINDOW, WINDOW));
    let counter = Counter {
        until: T + 1,
        ..counter
    };
    assert!(!counter.spent(T + MAX_LOCKOUT, WINDOW));
    assert!(counter.spent(T + MAX_LOCKOUT + 1, WINDOW));
}

#[test]
fn a_poisoned_lock_still_counts() {
    let limiter = std::sync::Arc::new(limiter());
    let held = std::sync::Arc::clone(&limiter);
    let panicked = std::thread::spawn(move || {
        let _state = held.state.lock().unwrap();
        panic!("poisoned");
    })
    .join();
    assert!(panicked.is_err() && limiter.state.is_poisoned());
    guess(&limiter, ip("192.0.2.1"), 1, 5, T);
    assert!(!limiter.allows(ip("192.0.2.1"), 1, T));
}
