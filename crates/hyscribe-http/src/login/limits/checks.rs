// SPDX-License-Identifier: AGPL-3.0-or-later

//! The site wide cap on password checks at once (ADR 0018): a check over
//! it waits in a bounded line for a short while, else is not run.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// How long a check waits for its turn.
pub(crate) const WAIT: Duration = Duration::from_secs(2);

/// How many checks wait at most, for each that may run.
const LINE: usize = 8;

#[derive(Debug, Default)]
struct Line {
    running: usize,
    waiting: usize,
}

/// The checks running and waiting, at most `cap` running.
#[derive(Debug)]
pub(crate) struct Checks {
    cap: usize,
    line: Mutex<Line>,
    freed: Condvar,
    clock: fn() -> Instant,
}

/// One running check, until dropped.
pub(crate) struct Check<'a>(&'a Checks);

impl Drop for Check<'_> {
    fn drop(&mut self) {
        self.0.line().running -= 1;
        self.0.freed.notify_one();
    }
}

impl Checks {
    pub(crate) fn new(cap: usize, clock: fn() -> Instant) -> Checks {
        Checks {
            cap,
            line: Mutex::default(),
            freed: Condvar::new(),
            clock,
        }
    }

    /// As many at once as the machine has CPUs.
    pub(crate) fn per_cpu() -> Checks {
        let cpus = std::thread::available_parallelism().map_or(1, usize::from);
        Checks::new(cpus, Instant::now)
    }

    fn line(&self) -> MutexGuard<'_, Line> {
        self.line.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A check's turn to run: at once under the cap, else once one ends
    /// within `WAIT`; none when the line is full or the wait is over.
    pub(crate) fn start(&self) -> Option<Check<'_>> {
        let mut line = self.line();
        if line.running >= self.cap && line.waiting >= self.cap * LINE {
            return None;
        }
        let deadline = (self.clock)() + WAIT;
        line.waiting += 1;
        while line.running >= self.cap {
            let left = deadline.saturating_duration_since((self.clock)());
            if left.is_zero() {
                line.waiting -= 1;
                return None;
            }
            let woken = self.freed.wait_timeout(line, left);
            line = woken.unwrap_or_else(PoisonError::into_inner).0;
        }
        line.waiting -= 1;
        line.running += 1;
        Some(Check(self))
    }
}

#[cfg(test)]
mod tests;
