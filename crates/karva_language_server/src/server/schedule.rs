//! Scheduling for work that must not block the language-server event loop.

use std::io;
use std::num::NonZeroUsize;

use crate::session::Session;
use crate::session::client::Client;

mod task;
mod thread;

pub(super) use task::{BackgroundSchedule, Task};

/// Runs synchronous tasks on the event-loop thread and background tasks on a
/// joined worker pool.
pub(super) struct Scheduler {
    worker_threads: NonZeroUsize,
    background_pool: Option<thread::Pool>,
}

impl Scheduler {
    /// Creates scheduler using explicit worker count.
    pub(super) fn new(worker_threads: NonZeroUsize) -> Self {
        Self {
            worker_threads,
            background_pool: None,
        }
    }

    /// Dispatches task while retaining mutable session access only for sync work.
    pub(super) fn dispatch(&mut self, task: Task, session: &mut Session, client: Client) {
        match task {
            Task::Sync(task::SyncTask { func }) => func(session, &client),
            Task::Background(task::BackgroundTaskBuilder { schedule, builder }) => {
                let function = builder(session);
                let job = move || function(&client);
                match schedule {
                    BackgroundSchedule::Worker => match self.background_pool() {
                        Ok(pool) => pool.spawn(job),
                        Err(error) => {
                            tracing::error!(%error, "failed to initialize worker pool; running task on event loop");
                            job();
                        }
                    },
                }
            }
        }
    }

    fn background_pool(&mut self) -> io::Result<&thread::Pool> {
        if self.background_pool.is_none() {
            self.background_pool = Some(thread::Pool::new(self.worker_threads)?);
        }
        self.background_pool
            .as_ref()
            .ok_or_else(|| io::Error::other("language-server worker pool did not initialize"))
    }
}
