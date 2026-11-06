use std::io;
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};

use super::SEMANTIC_STACK_BYTES;

/// Receipt: a local 1,000-request stdio burst submitted in 20.5 ms and drained
/// in 176.6 ms. Keep room for twice that entire burst, independent of CPU count.
pub(in crate::server::schedule) const QUEUED_REQUEST_LIMIT: usize = 2048;

struct Job(Box<dyn FnOnce() + Send + 'static>);

impl Job {
    fn run(self) {
        (self.0)();
    }
}

/// Small worker pool whose threads finish all queued work during drop.
pub(in crate::server::schedule) struct Pool {
    sender: Option<Sender<Job>>,
    handles: Vec<JoinHandle<()>>,
}

impl Pool {
    pub(in crate::server::schedule) fn new(worker_threads: NonZeroUsize) -> io::Result<Self> {
        let (sender, receiver) = bounded(QUEUED_REQUEST_LIMIT);
        let mut handles = Vec::with_capacity(worker_threads.get());

        for index in 0..worker_threads.get() {
            let worker_receiver = receiver.clone();
            let result = thread::Builder::new()
                .name(format!("karva-language-server-worker-{index}"))
                .stack_size(SEMANTIC_STACK_BYTES)
                .spawn(move || worker_loop(&worker_receiver));
            match result {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    drop(receiver);
                    drop(sender);
                    join_handles(handles);
                    return Err(error);
                }
            }
        }

        Ok(Self {
            sender: Some(sender),
            handles,
        })
    }

    pub(in crate::server::schedule) fn spawn<F>(&self, function: F) -> io::Result<()>
    where
        F: FnOnce() + Send + 'static,
    {
        let Some(sender) = self.sender.as_ref() else {
            return Err(io::Error::other("language-server worker pool is closed"));
        };
        sender.try_send(Job(Box::new(function))).map_err(|error| match error {
            TrySendError::Full(_) => io::Error::other(format!(
                "language-server queued request budget: limit {QUEUED_REQUEST_LIMIT}, requested {}; {error}",
                QUEUED_REQUEST_LIMIT + 1
            )),
            TrySendError::Disconnected(_) => io::Error::other("language-server worker pool stopped"),
        })
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.sender.take();
        join_handles(std::mem::take(&mut self.handles));
    }
}

fn worker_loop(receiver: &Receiver<Job>) {
    while let Ok(job) = receiver.recv() {
        if catch_unwind(AssertUnwindSafe(|| job.run())).is_err() {
            tracing::error!("language-server background task panicked");
        }
    }
}

fn join_handles(handles: Vec<JoinHandle<()>>) {
    for handle in handles {
        if handle.join().is_err() {
            tracing::error!("language-server worker thread panicked");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::Pool;

    #[test]
    fn dispatches_jobs_and_joins_workers() -> std::io::Result<()> {
        let completed = Arc::new(AtomicUsize::new(0));
        let pool = Pool::new(std::num::NonZeroUsize::MIN)?;

        for _ in 0..3 {
            let completed = Arc::clone(&completed);
            pool.spawn(move || {
                completed.fetch_add(1, Ordering::Relaxed);
            })?;
        }

        drop(pool);
        assert_eq!(completed.load(Ordering::Relaxed), 3);
        Ok(())
    }
}

#[cfg(test)]
mod saturation_tests {
    use super::{Pool, QUEUED_REQUEST_LIMIT};
    use crossbeam_channel::bounded;
    use std::num::NonZeroUsize;

    #[test]
    fn saturation_rejects_without_waiting_and_other_workers_progress() -> std::io::Result<()> {
        let pool = Pool::new(NonZeroUsize::MIN)?;
        let (started, running) = bounded(0);
        let (release, blocked) = bounded::<()>(0);
        pool.spawn(move || {
            let _ = started.send(());
            let _ = blocked.recv();
        })?;
        running.recv().expect("worker running");
        for _ in 0..QUEUED_REQUEST_LIMIT {
            pool.spawn(|| {})?;
        }
        let error = pool.spawn(|| {}).expect_err("queue must be bounded");
        assert!(error.to_string().contains(&format!(
            "limit {QUEUED_REQUEST_LIMIT}, requested {}",
            QUEUED_REQUEST_LIMIT + 1
        )));
        // Diagnostics and interactive work never share their worker or queue.
        let latest = super::super::LatestPool::new()?;
        let (done, completed) = bounded(0);
        latest.spawn(move || {
            let _ = done.send(());
        });
        completed
            .recv()
            .expect("independent worker completes despite saturation");
        drop(release);
        drop(pool);
        Ok(())
    }
}
