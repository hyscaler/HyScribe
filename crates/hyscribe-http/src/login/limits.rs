// SPDX-License-Identifier: AGPL-3.0-or-later

//! Login rate limits (docs/SECURITY_MODEL.md): failed logins counted per
//! client address and per account, in memory only, so that a limited guess
//! is answered as a wrong password before its hash is checked.

use std::collections::HashMap;
use std::hash::Hash;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Mutex, MutexGuard, PoisonError};

use hyscribe_config::LoginLimits;

/// The longest an address is refused, and how long its refusals are
/// remembered for the next one to double.
const MAX_LOCKOUT: i64 = 86_400;

/// How many addresses, accounts and trusted logins each map keeps.
pub(crate) const TRACKED: usize = 65_536;

/// One address's or account's failures in its window, and its refusals.
#[derive(Clone, Copy, Debug, Default)]
struct Counter {
    failures: u32,
    /// When the window began.
    since: i64,
    /// Refusals in a row, each twice as long as the one before.
    strikes: u32,
    /// Refused until then.
    until: i64,
}

impl Counter {
    fn refused(&self, now: i64) -> bool {
        now < self.until
    }

    /// A failure at `now`: the `limit`th in a window refuses for the window,
    /// doubled for each refusal of the day before, up to `cap`.
    fn fail(&mut self, now: i64, (limit, window, cap): (u32, i64, i64)) {
        if self.refused(now) {
            return;
        }
        if now - self.until >= MAX_LOCKOUT {
            self.strikes = 0;
        }
        if now - self.since >= window {
            (self.failures, self.since) = (0, now);
        }
        self.failures += 1;
        if self.failures >= limit {
            self.until = now + (window << self.strikes.min(16)).min(cap);
            self.strikes += 1;
        }
    }

    /// Nothing worth keeping: the window and a day after the last refusal
    /// are over.
    fn spent(&self, now: i64, window: i64) -> bool {
        now - self.since >= window && now - self.until >= MAX_LOCKOUT
    }
}

#[derive(Debug, Default)]
struct State {
    addresses: HashMap<IpAddr, Counter>,
    accounts: HashMap<u64, Counter>,
    /// When an account last logged in from an address.
    trusted: HashMap<(u64, IpAddr), i64>,
}

/// The value of `key` in `map`; a new key in a full map first drops the
/// `spent` entries and, when it is still full, the one of the lowest `age`.
fn slot<K: Hash + Eq + Copy, V: Default>(
    map: &mut HashMap<K, V>,
    key: K,
    spent: impl Fn(&V) -> bool,
    age: impl Fn(&V) -> i64,
) -> &mut V {
    if map.len() >= TRACKED && !map.contains_key(&key) {
        map.retain(|_, value| !spent(value));
        if map.len() >= TRACKED {
            let oldest = map.iter().min_by_key(|(_, value)| age(value));
            if let Some(oldest) = oldest.map(|(key, _)| *key) {
                map.remove(&oldest);
            }
        }
    }
    map.entry(key).or_default()
}

/// The address a client is counted under: an IPv6 one by its /64, which a
/// single host is commonly given whole.
fn network(client: Option<IpAddr>) -> IpAddr {
    match client.map(|ip| ip.to_canonical()) {
        Some(IpAddr::V6(ip)) => Ipv6Addr::from(u128::from(ip) & !(u128::MAX >> 64)).into(),
        Some(ip) => ip,
        None => Ipv4Addr::UNSPECIFIED.into(),
    }
}

/// The site's login limits and what they have counted.
#[derive(Debug)]
pub(crate) struct Limiter {
    limits: LoginLimits,
    state: Mutex<State>,
}

impl Limiter {
    pub(crate) fn new(limits: LoginLimits) -> Limiter {
        Limiter {
            limits,
            state: Mutex::default(),
        }
    }

    /// The counts, also after a panic elsewhere left the lock poisoned.
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether `user`'s password may be checked for `client`: neither is
    /// refused, unless the account logged in from that address before.
    pub(crate) fn allows(&self, client: Option<IpAddr>, user: u64, now: i64) -> bool {
        let key = network(client);
        let state = self.state();
        let refused = |counter: Option<&Counter>| counter.is_some_and(|c| c.refused(now));
        !refused(state.addresses.get(&key))
            && (!refused(state.accounts.get(&user)) || state.trusted.contains_key(&(user, key)))
    }

    /// A guess from `client`, with the accounts whose password was checked
    /// and whether it matched: a match trusts the address for the account,
    /// else the address and each account checked count a failure.
    pub(crate) fn record(&self, client: Option<IpAddr>, checked: &[(u64, bool)], now: i64) {
        let key = network(client);
        let window = i64::from(self.limits.window);
        let mut state = self.state();
        let matched: Vec<u64> = checked.iter().filter(|c| c.1).map(|c| c.0).collect();
        for user in &matched {
            *slot(&mut state.trusted, (*user, key), |_| false, |at| *at) = now;
        }
        if !matched.is_empty() {
            return;
        }
        let spent = |counter: &Counter| counter.spent(now, window);
        let age = |counter: &Counter| counter.since;
        let address = (self.limits.address, window, MAX_LOCKOUT);
        slot(&mut state.addresses, key, spent, age).fail(now, address);
        for (user, _) in checked {
            let account = (self.limits.account, window, window);
            slot(&mut state.accounts, *user, spent, age).fail(now, account);
        }
    }
}

#[cfg(test)]
mod tests;
