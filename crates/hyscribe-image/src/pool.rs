// SPDX-License-Identifier: AGPL-3.0-or-later

//! The bounded worker pool image jobs run in (ADR 0015): as many workers
//! as asked, normally the CPU count, each running one job at a time, and a
//! queue of at most `queue` waiting jobs, so the memory in use stays the
//! workers' count times one job's budget. A request's thread hands its job
//! over and waits for the answer; it never resizes itself. A job that
//! panics ends only its own run, so no build may abort on a panic.

#[cfg(panic = "abort")]
compile_error!("the image pool catches a decoder's panic: build with panic = \"unwind\"");

use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

type Job = Box<dyn FnOnce() + Send>;

/// The workers and their queue.
pub struct Pool {
    jobs: SyncSender<Job>,
    workers: Vec<JoinHandle<()>>,
}

impl Pool {
    /// A pool of `workers` threads (at least one) and a queue of `queue`
    /// jobs.
    pub fn new(workers: usize, queue: usize) -> Pool {
        let (jobs, waiting) = sync_channel::<Job>(queue);
        let waiting = Arc::new(Mutex::new(waiting));
        let workers = (0..workers.max(1))
            .map(|_| {
                let waiting = Arc::clone(&waiting);
                thread::spawn(move || work(&waiting))
            })
            .collect();
        Pool { jobs, workers }
    }

    /// Runs `job` on a worker, waiting for a place in the queue and then
    /// for the answer; `None` if the job panicked.
    pub fn run<T: Send + 'static>(&self, job: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let (answer, answered) = sync_channel(1);
        let job: Job = Box::new(move || {
            // The caller may have stopped waiting; its answer is then dropped.
            let _ = answer.send(job());
        });
        // A job the pool cannot take drops its answer's sender: None.
        let _ = self.jobs.send(job);
        answered.recv().ok()
    }
}

/// A worker: the next waiting job, run, until the pool is gone. A job that
/// panics ends only its own run; a stack overflow still aborts the process.
fn work(waiting: &Mutex<Receiver<Job>>) {
    while let Ok(Ok(job)) = waiting.lock().map(|waiting| waiting.recv()) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
    }
}

impl Drop for Pool {
    /// Closes the queue and waits for the workers to finish their jobs.
    fn drop(&mut self) {
        self.jobs = sync_channel(0).0;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests;
