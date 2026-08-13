use std::collections::HashSet;

use lsp_server::RequestId;

/// Tracks server requests awaiting a client response.
#[derive(Debug, Default)]
pub struct RequestQueue {
    outgoing: HashSet<RequestId>,
}

impl RequestQueue {
    pub(crate) fn register_outgoing(&mut self, id: RequestId) {
        self.outgoing.insert(id);
    }

    pub(crate) fn complete_outgoing(&mut self, id: &RequestId) -> bool {
        self.outgoing.remove(id)
    }
}
