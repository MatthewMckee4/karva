//! Scheduling for work that must not block the language-server event loop.

use std::io;
use std::num::NonZeroUsize;

use crate::session::Session;
use crate::session::client::Client;

mod task;
mod thread;

pub(super) use task::{BackgroundSchedule, Task};
pub(super) use thread::SEMANTIC_STACK_BYTES;

/// Runs synchronous tasks on the event-loop thread and background tasks on a
/// joined worker pool.
pub(super) struct Scheduler {
    worker_threads: NonZeroUsize,
    background_pool: Option<thread::Pool>,
    latest_pool: thread::LatestPool,
}

impl Scheduler {
    /// Creates scheduler using explicit worker count.
    pub(super) fn new(worker_threads: NonZeroUsize) -> io::Result<Self> {
        Ok(Self {
            worker_threads,
            background_pool: None,
            latest_pool: thread::LatestPool::new()?,
        })
    }

    /// Dispatches task while retaining mutable session access only for sync work.
    pub(super) fn dispatch(&mut self, task: Task, session: &mut Session, client: &Client) {
        match task {
            Task::Sync(task::SyncTask { func }) => func(session, client),
            Task::Background(task::BackgroundTaskBuilder { schedule, builder }) => {
                let function = builder(session);
                let worker_client = client.clone();
                let job = move || function(&worker_client);
                match schedule {
                    BackgroundSchedule::Worker(id) => {
                        let result = self.background_pool().and_then(|pool| pool.spawn(job));
                        if let Err(error) = result {
                            let response = lsp_server::Response::new_err(
                                id,
                                lsp_server::ErrorCode::ServerCancelled as i32,
                                error.to_string(),
                            );
                            if let Err(error) = client.respond(response) {
                                tracing::error!(%error, "failed to queue scheduling error");
                            }
                        }
                    }
                    BackgroundSchedule::Latest => {
                        self.latest_pool.spawn(job);
                    }
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

#[cfg(test)]
mod tests {
    use super::{Scheduler, Task};
    use crate::server::{Action, Event};
    use crate::session::{Session, client::Client};
    use crate::workspace::Workspaces;
    use crossbeam_channel::bounded;
    use lsp_server::{ErrorCode, Request, RequestId};
    use lsp_types::MarkupKind;
    use ruff_python_ast::PythonVersion;
    use std::num::NonZeroUsize;

    #[test]
    fn saturated_requests_leave_sync_work_responsive() -> anyhow::Result<()> {
        let mut scheduler = Scheduler::new(NonZeroUsize::MIN)?;
        let pool = scheduler.background_pool()?;
        let (started, running) = bounded(0);
        let (release, blocked) = bounded::<()>(0);
        pool.spawn(move || {
            let _ = started.send(());
            let _ = blocked.recv();
        })?;
        running.recv()?;
        for _ in 0..super::thread::QUEUED_REQUEST_LIMIT {
            pool.spawn(|| {})?;
        }
        let mut session = Session::new(
            crate::PositionEncoding::UTF16,
            MarkupKind::PlainText,
            false,
            false,
            None,
            Workspaces::new(Vec::new(), PythonVersion::PY313, None)?,
        );
        let (events, received) = bounded(1);
        let (messages, _) = crossbeam_channel::unbounded();
        let client = Client::new(events, messages);
        let id = RequestId::from(1);
        session
            .request_queue_mut()
            .incoming_mut()
            .register(id.clone(), "textDocument/hover".to_owned());
        let request = Request::new(
            id.clone(),
            "textDocument/hover".to_owned(),
            serde_json::json!({
                "textDocument": {"uri": "file:///test.py"}, "position": {"line":0,"character":0}
            }),
        );
        scheduler.dispatch(crate::server::api::request(request), &mut session, &client);
        let Event::Action(Action::SendResponse(response)) = received.recv()? else {
            anyhow::bail!("expected scheduling rejection");
        };
        assert_eq!(response.id, id);
        let error = response.response_result.expect_err("queue full");
        assert_eq!(error.code, ErrorCode::ServerCancelled as i32);
        assert!(error.message.contains("limit 2048, requested 2049"));
        scheduler.dispatch(
            Task::sync(|session, _| session.request_shutdown()),
            &mut session,
            &client,
        );
        assert!(session.is_shutdown_requested());
        drop(release);
        Ok(())
    }
}
