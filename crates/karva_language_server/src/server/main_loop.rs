//! Main-loop message ordering and cancellation-aware response delivery.

use anyhow::bail;
use crossbeam_channel::{Receiver, RecvError, Sender, select_biased};
use lsp_server::{ErrorCode, Message, Response};
use lsp_types::{ExitNotification, Notification as _};

use crate::Server;
use crate::server::schedule::Scheduler;
use crate::session::DocumentSnapshotVersion;
use crate::session::client::Client;

pub type MainLoopSender = Sender<Event>;
pub type MainLoopReceiver = Receiver<Event>;

#[derive(Debug)]
pub enum Event {
    Action(Action),
}

#[derive(Debug)]
pub enum Action {
    SendResponse(Response),

    SendVersionedResponse {
        response: Response,
        version: DocumentSnapshotVersion,
    },

    PublishDiagnostics(super::api::diagnostics::DiagnosticPublication),
}

enum NextEvent {
    Message(Message),
    Action(Action),
}

impl Server {
    pub(super) fn main_loop(&mut self) -> anyhow::Result<()> {
        let mut scheduler = Scheduler::new(std::thread::available_parallelism()?)?;

        loop {
            let Some(event) = self.next_event()? else {
                bail!("client exited without completing the shutdown sequence");
            };
            let client = Client::new(
                self.main_loop_sender.clone(),
                self.connection.sender.clone(),
            );

            match event {
                NextEvent::Message(Message::Request(request)) => {
                    self.session
                        .request_queue_mut()
                        .incoming_mut()
                        .register(request.id.clone(), request.method.clone());

                    let task = if self.session.is_shutdown_requested() {
                        crate::server::schedule::Task::immediate(Response::new_err(
                            request.id,
                            ErrorCode::InvalidRequest as i32,
                            "shutdown already requested".to_owned(),
                        ))
                    } else {
                        super::api::request(request)
                    };
                    scheduler.dispatch(task, &mut self.session, client);
                }
                NextEvent::Message(Message::Notification(notification)) => {
                    if notification.method == ExitNotification::METHOD.as_str() {
                        if !self.session.is_shutdown_requested() {
                            bail!("received exit notification before shutdown request");
                        }
                        return Ok(());
                    }

                    let task = super::api::notification(notification);
                    scheduler.dispatch(task, &mut self.session, client);
                }
                NextEvent::Message(Message::Response(response)) => {
                    let is_file_watcher_registration = response.id
                        == lsp_server::RequestId::from(
                            super::REGISTER_FILE_WATCHERS_REQUEST_ID.to_owned(),
                        );
                    if !self
                        .session
                        .request_queue_mut()
                        .complete_outgoing(&response.id)
                    {
                        tracing::warn!(id = %response.id, "received unexpected response");
                    } else if is_file_watcher_registration && response.response_result.is_ok() {
                        self.session.enable_source_index_cache();
                    } else if is_file_watcher_registration {
                        tracing::warn!("client rejected dynamic file watcher registration");
                    }
                }
                NextEvent::Action(Action::SendResponse(response)) => {
                    self.send_response_if_pending(response)?;
                }
                NextEvent::Action(Action::SendVersionedResponse { response, version }) => {
                    let response = if self.session.is_document_snapshot_current(&version) {
                        response
                    } else {
                        Response::new_err(
                            response.id,
                            ErrorCode::ContentModified as i32,
                            "document or project sources changed before request completed"
                                .to_owned(),
                        )
                    };
                    self.send_response_if_pending(response)?;
                }
                NextEvent::Action(Action::PublishDiagnostics(publication)) => {
                    if self
                        .session
                        .is_source_index_revision_current(&publication.source_index_revision)
                    {
                        super::api::diagnostics::publish_diagnostics(
                            &mut self.session,
                            &client,
                            publication,
                        )?;
                    }
                }
            }
        }
    }

    fn next_event(&self) -> Result<Option<NextEvent>, RecvError> {
        // Document mutations and cancellations already queued by the client
        // take precedence over results computed from older session state.
        select_biased! {
            recv(self.connection.receiver) -> message => {
                Ok(message.ok().map(NextEvent::Message))
            },
            recv(self.main_loop_receiver) -> event => {
                event.map(|event| Some(match event {
                    Event::Action(action) => NextEvent::Action(action),
                }))
            },
        }
    }

    fn send_response_if_pending(&mut self, response: Response) -> anyhow::Result<()> {
        if let Some((started, method)) = self
            .session
            .request_queue_mut()
            .incoming_mut()
            .complete(&response.id)
        {
            tracing::debug!(
                id = %response.id,
                %method,
                elapsed = ?started.elapsed(),
                "completed request"
            );
            self.connection.sender.send(Message::Response(response))?;
        } else {
            tracing::debug!(id = %response.id, "discarding cancelled response");
        }
        Ok(())
    }
}
