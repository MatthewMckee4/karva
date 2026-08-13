use anyhow::bail;
use lsp_server::{ErrorCode, Message, Response};
use lsp_types::{ExitNotification, Notification as _};

use crate::Server;
use crate::session::client::Client;

impl Server {
    pub(super) fn main_loop(&mut self) -> anyhow::Result<()> {
        let client = Client::new(self.connection.sender.clone());
        for message in &self.connection.receiver {
            match message {
                Message::Request(request) => {
                    let response = if self.session.is_shutdown_requested() {
                        Response::new_err(
                            request.id,
                            ErrorCode::InvalidRequest as i32,
                            "shutdown already requested".to_owned(),
                        )
                    } else {
                        super::api::request(request, &mut self.session)
                    };
                    self.connection.sender.send(Message::Response(response))?;
                }
                Message::Notification(notification) => {
                    if notification.method == ExitNotification::METHOD.as_str() {
                        if !self.session.is_shutdown_requested() {
                            bail!("received exit notification before shutdown request");
                        }
                        return Ok(());
                    }

                    super::api::notification(notification, &mut self.session, &client);
                }
                Message::Response(response) => {
                    if !self
                        .session
                        .request_queue_mut()
                        .complete_outgoing(&response.id)
                    {
                        tracing::warn!("received unexpected response with ID {}", response.id);
                    }
                }
            }
        }

        bail!("client exited without completing the shutdown sequence")
    }
}
