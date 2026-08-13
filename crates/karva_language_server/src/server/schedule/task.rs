use crate::session::Session;
use crate::session::client::Client;

type LocalFunction = Box<dyn FnOnce(&mut Session, &Client)>;
type BackgroundFunction = Box<dyn FnOnce(&Client) + Send + 'static>;
type BackgroundFunctionBuilder = Box<dyn FnOnce(&mut Session) -> BackgroundFunction>;

/// Selects worker priority for background work.
#[derive(Clone, Copy, Debug, Default)]
pub(in crate::server) enum BackgroundSchedule {
    /// Run on regular-priority workers.
    #[default]
    Worker,
}

/// Work waiting for dispatch by [`super::Scheduler`].
#[must_use]
pub(in crate::server) enum Task {
    /// Work requiring mutable event-loop state.
    Sync(SyncTask),

    /// Work built from an owned session snapshot.
    Background(BackgroundTaskBuilder),
}

/// Builder for work that captures owned state before queueing.
pub(in crate::server) struct BackgroundTaskBuilder {
    pub(super) schedule: BackgroundSchedule,
    pub(super) builder: BackgroundFunctionBuilder,
}

/// Work executed synchronously by the event loop.
pub(in crate::server) struct SyncTask {
    pub(super) func: LocalFunction,
}

impl Task {
    /// Creates background work. Builder runs synchronously to create a `'static`
    /// closure before worker dispatch.
    pub(in crate::server) fn background<F>(schedule: BackgroundSchedule, function: F) -> Self
    where
        F: FnOnce(&mut Session) -> BackgroundFunction + 'static,
    {
        Self::Background(BackgroundTaskBuilder {
            schedule,
            builder: Box::new(function),
        })
    }

    /// Creates work that runs on event-loop thread.
    pub(in crate::server) fn sync<F>(function: F) -> Self
    where
        F: FnOnce(&mut Session, &Client) + 'static,
    {
        Self::Sync(SyncTask {
            func: Box::new(function),
        })
    }

    /// Creates work that immediately queues a protocol response.
    pub(in crate::server) fn immediate(response: lsp_server::Response) -> Self {
        Self::sync(move |_, client| {
            if let Err(error) = client.respond(response) {
                tracing::error!(%error, "failed to queue immediate response");
            }
        })
    }

    /// Creates no-op event-loop work.
    pub(in crate::server) fn nothing() -> Self {
        Self::sync(|_, _| {})
    }
}
