use crossbeam_channel::Sender;
use lsp_server::{ErrorCode, Message, RequestId, Response, ResponseError};
use lsp_types::{Notification, Request};

use crate::server::{Action, Event, MainLoopSender};
use crate::session::Session;

/// Typed access to messages sent from the server to the editor client.
#[derive(Clone)]
pub(crate) struct Client {
    main_loop_sender: MainLoopSender,
    connection_sender: Sender<Message>,
}

impl Client {
    pub(crate) fn new(
        main_loop_sender: MainLoopSender,
        connection_sender: Sender<Message>,
    ) -> Self {
        Self {
            main_loop_sender,
            connection_sender,
        }
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
        self.connection_sender.send(Message::Request(request))?;
        Ok(())
    }

    pub(crate) fn send_notification<N: Notification>(&self, params: N::Params) -> anyhow::Result<()> {
        let notification = lsp_server::Notification {
            method: N::METHOD.as_str().to_owned(),
            params: serde_json::to_value(params)?,
        };
        self.connection_sender
            .send(Message::Notification(notification))?;
        Ok(())
    }

    /// Queues a response for cancellation-aware delivery on the main loop.
    pub(crate) fn respond(&self, response: Response) -> anyhow::Result<()> {
        self.main_loop_sender
            .send(Event::Action(Action::SendResponse(response)))?;
        Ok(())
    }

    /// Cancels a pending editor request and sends its sole response directly.
    pub(crate) fn cancel(&self, session: &mut Session, id: RequestId) -> anyhow::Result<()> {
        let method = session.request_queue_mut().incoming_mut().cancel(&id);
        let Some(method) = method else {
            return Ok(());
        };

        tracing::debug!(%id, %method, "cancelled request");
        self.connection_sender.send(Message::Response(Response {
            id,
            response_result: Err(ResponseError {
                code: ErrorCode::RequestCanceled as i32,
                message: "request was cancelled by client".to_owned(),
                data: None,
            }),
        }))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Context, bail};
    use crossbeam_channel::{TryRecvError, unbounded};
    use lsp_server::{ErrorCode, Message, RequestId, Response};
    use ruff_python_ast::PythonVersion;

    use super::Client;
    use crate::PositionEncoding;
    use crate::server::{Action, Event};
    use crate::session::Session;
    use crate::workspace::Workspaces;

    #[test]
    fn cancellation_sends_one_response_and_filters_late_result() -> anyhow::Result<()> {
        let (event_sender, event_receiver) = unbounded();
        let (connection_sender, connection_receiver) = unbounded();
        let client = Client::new(event_sender, connection_sender);
        let mut session = Session::new(
            PositionEncoding::UTF16,
            false,
            Workspaces::new(Vec::new(), PythonVersion::PY312, None)?,
        );
        let id = RequestId::from(7);
        session
            .request_queue_mut()
            .incoming_mut()
            .register(id.clone(), "example/request".to_owned());
        let cancellation = session
            .request_queue()
            .incoming()
            .cancellation_token(&id)
            .context("pending request should expose cancellation")?;

        client.cancel(&mut session, id.clone())?;

        let Message::Response(cancelled) = connection_receiver.recv()? else {
            bail!("cancellation should send a response");
        };
        assert_eq!(cancelled.id, id);
        assert_eq!(
            cancelled.response_result.map_err(|error| error.code),
            Err(ErrorCode::RequestCanceled as i32)
        );
        assert!(cancellation.is_cancelled());

        client.respond(Response {
            id: id.clone(),
            response_result: Ok(serde_json::Value::Null),
        })?;
        let Event::Action(Action::SendResponse(late)) = event_receiver.recv()?;
        assert_eq!(late.id, id);
        assert!(
            session
                .request_queue_mut()
                .incoming_mut()
                .complete(&late.id)
                .is_none()
        );
        assert!(matches!(
            connection_receiver.try_recv(),
            Err(TryRecvError::Empty)
        ));
        Ok(())
    }
}
