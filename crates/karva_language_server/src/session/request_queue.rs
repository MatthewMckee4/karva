//! Pending requests exchanged with the editor client.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use lsp_server::RequestId;

/// Tracks requests in both protocol directions.
#[derive(Debug, Default)]
pub struct RequestQueue {
    incoming: Incoming,
    outgoing: HashSet<RequestId>,
}

impl RequestQueue {
    pub(crate) fn incoming(&self) -> &Incoming {
        &self.incoming
    }

    pub(crate) fn incoming_mut(&mut self) -> &mut Incoming {
        &mut self.incoming
    }

    pub(crate) fn register_outgoing(&mut self, id: RequestId) {
        self.outgoing.insert(id);
    }

    pub(crate) fn complete_outgoing(&mut self, id: &RequestId) -> bool {
        self.outgoing.remove(id)
    }
}

/// Requests sent by the editor that have not completed or been cancelled.
#[derive(Debug, Default)]
pub struct Incoming {
    pending: HashMap<RequestId, PendingRequest>,
}

impl Incoming {
    pub(crate) fn register(&mut self, id: RequestId, method: String) {
        self.pending.insert(id, PendingRequest::new(method));
    }

    pub(crate) fn cancel(&mut self, id: &RequestId) -> Option<String> {
        self.pending.remove(id).map(|pending| {
            if let Some(token) = pending.cancellation_token.get() {
                token.cancel();
            }
            pending.method
        })
    }

    pub(crate) fn cancellation_token(&self, id: &RequestId) -> Option<RequestCancellationToken> {
        let pending = self.pending.get(id)?;
        Some(
            pending
                .cancellation_token
                .get_or_init(RequestCancellationToken::default)
                .clone(),
        )
    }

    pub(crate) fn complete(&mut self, id: &RequestId) -> Option<(Instant, String)> {
        self.pending
            .remove(id)
            .map(|pending| (pending.started, pending.method))
    }
}

#[derive(Debug)]
struct PendingRequest {
    started: Instant,
    method: String,
    cancellation_token: OnceLock<RequestCancellationToken>,
}

impl PendingRequest {
    fn new(method: String) -> Self {
        Self {
            started: Instant::now(),
            method,
            cancellation_token: OnceLock::new(),
        }
    }
}

/// Cooperative cancellation state shared with a background task.
#[derive(Clone, Debug, Default)]
pub struct RequestCancellationToken(Arc<AtomicBool>);

impl RequestCancellationToken {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes_pending_request_once() {
        let id = RequestId::from(1);
        let mut incoming = Incoming::default();
        incoming.register(id.clone(), "example/request".to_owned());

        assert!(incoming.cancellation_token(&id).is_some());
        let completed = incoming.complete(&id);
        assert_eq!(
            completed.as_ref().map(|(_, method)| method.as_str()),
            Some("example/request")
        );
        assert!(incoming.cancellation_token(&id).is_none());
        assert!(incoming.complete(&id).is_none());
        assert!(incoming.cancel(&id).is_none());
    }

    #[test]
    fn cancellation_marks_shared_token_and_completes_request() {
        let id = RequestId::from("request".to_owned());
        let mut incoming = Incoming::default();
        incoming.register(id.clone(), "example/request".to_owned());
        let token = incoming.cancellation_token(&id);

        assert_eq!(incoming.cancel(&id).as_deref(), Some("example/request"));
        assert!(token.is_some_and(|token| token.is_cancelled()));
        assert!(incoming.cancellation_token(&id).is_none());
        assert!(incoming.complete(&id).is_none());
    }
}
