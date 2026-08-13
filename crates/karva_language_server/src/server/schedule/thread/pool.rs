use std::io;
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender, unbounded};

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
        let (sender, receiver) = unbounded();
        let mut handles = Vec::with_capacity(worker_threads.get());

        for index in 0..worker_threads.get() {
            let worker_receiver = receiver.clone();
            let result = thread::Builder::new()
                .name(format!("karva-language-server-worker-{index}"))
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

    pub(in crate::server::schedule) fn spawn<F>(&self, function: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let Some(sender) = self.sender.as_ref() else {
            tracing::error!("language-server worker pool is closed; running task inline");
            function();
            return;
        };
        if let Err(error) = sender.send(Job(Box::new(function))) {
            tracing::error!("language-server worker pool stopped; running task inline");
            error.0.run();
        }
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
            });
        }

        drop(pool);
        assert_eq!(completed.load(Ordering::Relaxed), 3);
        Ok(())
    }
}
