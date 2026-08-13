use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

type Job = Box<dyn FnOnce() + Send + 'static>;

struct State {
    pending: Option<Job>,
    shutdown: bool,
}

/// Single-worker queue retaining only newest pending job.
pub(in crate::server::schedule) struct LatestPool {
    state: Arc<(Mutex<State>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}

impl LatestPool {
    pub(in crate::server::schedule) fn new() -> io::Result<Self> {
        let state = Arc::new((
            Mutex::new(State {
                pending: None,
                shutdown: false,
            }),
            Condvar::new(),
        ));
        let worker_state = Arc::clone(&state);
        let handle = thread::Builder::new()
            .name("karva-language-server-latest-worker".to_owned())
            .spawn(move || worker_loop(&worker_state))?;
        Ok(Self {
            state,
            handle: Some(handle),
        })
    }

    pub(in crate::server::schedule) fn spawn<F>(&self, job: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let (lock, wake) = &*self.state;
        let Ok(mut state) = lock.lock() else {
            tracing::error!("latest-only worker state is poisoned; dropping task");
            return;
        };
        state.pending = Some(Box::new(job));
        wake.notify_one();
    }
}

impl Drop for LatestPool {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        if let Ok(mut state) = lock.lock() {
            state.shutdown = true;
            state.pending.take();
            wake.notify_one();
        }
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            tracing::error!("latest-only worker thread panicked");
        }
    }
}

fn worker_loop(state: &(Mutex<State>, Condvar)) {
    loop {
        let job = {
            let (lock, wake) = state;
            let Ok(mut state) = lock.lock() else {
                tracing::error!("latest-only worker state is poisoned");
                return;
            };
            while state.pending.is_none() && !state.shutdown {
                let waited = wake.wait(state);
                let Ok(next) = waited else {
                    tracing::error!("latest-only worker state is poisoned");
                    return;
                };
                state = next;
            }
            if state.pending.is_none() && state.shutdown {
                return;
            }
            state.pending.take()
        };
        if let Some(job) = job {
            if catch_unwind(AssertUnwindSafe(job)).is_err() {
                tracing::error!("latest-only worker task panicked");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Barrier};

    use super::LatestPool;

    #[test]
    fn replaces_pending_job() -> std::io::Result<()> {
        let pool = LatestPool::new()?;
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let completed = Arc::new(AtomicUsize::new(0));
        let first_started = Arc::clone(&started);
        let first_release = Arc::clone(&release);
        let first_completed = Arc::clone(&completed);
        pool.spawn(move || {
            first_started.wait();
            first_release.wait();
            first_completed.fetch_add(1, Ordering::Relaxed);
        });
        started.wait();
        let second_completed = Arc::clone(&completed);
        pool.spawn(move || {
            second_completed.fetch_add(1, Ordering::Relaxed);
        });
        let (third_done, wait_for_third) = mpsc::channel();
        let third_completed = Arc::clone(&completed);
        pool.spawn(move || {
            third_completed.fetch_add(10, Ordering::Relaxed);
            third_done
                .send(())
                .expect("test should wait for newest job");
        });
        release.wait();
        wait_for_third
            .recv()
            .expect("newest pending job should complete");
        drop(pool);
        assert_eq!(completed.load(Ordering::Relaxed), 11);
        Ok(())
    }
}
