#![expect(
    clippy::redundant_pub_crate,
    reason = "server consumes this client across a private sibling module"
)]

use crossbeam_channel::Sender;
use lsp_server::{Message, RequestId};
use lsp_types::Request;

/// Typed access to messages sent from the server to the editor client.
#[derive(Clone)]
pub(crate) struct Client {
    sender: Sender<Message>,
}

impl Client {
    pub(crate) fn new(sender: Sender<Message>) -> Self {
        Self { sender }
    }

    pub(crate) fn send_request<R: Request>(
        &self,
        id: RequestId,
        params: R::Params,
    ) -> anyhow::Result<()> {
        let request = lsp_server::Request {
            id,
            method: R::METHOD.as_str().to_owned(),
            params: serde_json::to_value(params)?,
        };
        self.sender.send(Message::Request(request))?;
        Ok(())
    }
}
