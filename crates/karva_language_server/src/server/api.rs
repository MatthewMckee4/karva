//! Typed protocol routing into scheduled language-server tasks.

use lsp_server::{ErrorCode, Notification, Request, Response};
use lsp_types::{
    CancelNotification, CompletionRequest, DefinitionRequest, DidChangeTextDocumentNotification,
    DidChangeWatchedFilesNotification, DidChangeWorkspaceFoldersNotification,
    DidCloseTextDocumentNotification, DidOpenTextDocumentNotification, HoverRequest,
    LspNotificationMethod, LspRequestMethod, Notification as _, PrepareRenameRequest,
    ReferencesRequest, RenameRequest, Request as _, ShutdownRequest,
};

use crate::server::schedule::{BackgroundSchedule, Task};
use crate::session::Session;
use crate::session::client::Client;

mod diagnostics;
mod notifications;
mod requests;
mod traits;

use self::traits::{BackgroundRequestHandler, SyncNotificationHandler, SyncRequestHandler};

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(in crate::server::api) struct RequestError {
    code: i32,
    message: String,
}

impl RequestError {
    pub(in crate::server::api) fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::InvalidParams as i32,
            message: message.into(),
        }
    }
}

pub(super) fn request(request: Request) -> Task {
    match LspRequestMethod::from(request.method.as_str()) {
        ShutdownRequest::METHOD => sync_request_task::<requests::Shutdown>(request),
        CompletionRequest::METHOD => background_request_task::<requests::Completion>(request),
        DefinitionRequest::METHOD => background_request_task::<requests::Definition>(request),
        HoverRequest::METHOD => background_request_task::<requests::Hover>(request),
        PrepareRenameRequest::METHOD => background_request_task::<requests::PrepareRename>(request),
        ReferencesRequest::METHOD => background_request_task::<requests::References>(request),
        RenameRequest::METHOD => background_request_task::<requests::Rename>(request),
        method => Task::immediate(Response::new_err(
            request.id,
            ErrorCode::MethodNotFound as i32,
            format!("unknown request: {method}"),
        )),
    }
}

pub(super) fn notification(notification: Notification) -> Task {
    match LspNotificationMethod::from(notification.method.as_str()) {
        CancelNotification::METHOD => sync_notification_task::<notifications::Cancel>(notification),
        DidOpenTextDocumentNotification::METHOD => {
            sync_notification_task::<notifications::DidOpen>(notification)
        }
        DidChangeTextDocumentNotification::METHOD => {
            sync_notification_task::<notifications::DidChange>(notification)
        }
        DidCloseTextDocumentNotification::METHOD => {
            sync_notification_task::<notifications::DidClose>(notification)
        }
        DidChangeWorkspaceFoldersNotification::METHOD => {
            sync_notification_task::<notifications::DidChangeWorkspaceFolders>(notification)
        }
        DidChangeWatchedFilesNotification::METHOD => {
            sync_notification_task::<notifications::DidChangeWatchedFiles>(notification)
        }
        method => {
            tracing::debug!(%method, "ignoring unsupported notification");
            Task::nothing()
        }
    }
}

fn sync_request_task<H>(request: Request) -> Task
where
    H: SyncRequestHandler,
{
    let id = request.id;
    let params = match serde_json::from_value(request.params) {
        Ok(params) => params,
        Err(error) => {
            return Task::immediate(Response::new_err(
                id,
                ErrorCode::InvalidParams as i32,
                error.to_string(),
            ));
        }
    };

    Task::sync(move |session, client| {
        let response = result_response(id, H::run(session, client, params));
        if let Err(error) = client.respond(response) {
            tracing::error!(%error, "failed to queue request response");
        }
    })
}

fn background_request_task<H>(request: Request) -> Task
where
    H: BackgroundRequestHandler,
    <<H as traits::RequestHandler>::RequestType as lsp_types::Request>::Params: Send + 'static,
{
    let id = request.id;
    let params = match serde_json::from_value(request.params) {
        Ok(params) => params,
        Err(error) => {
            return Task::immediate(Response::new_err(
                id,
                ErrorCode::InvalidParams as i32,
                error.to_string(),
            ));
        }
    };
    let task_id = id.clone();

    Task::background(BackgroundSchedule::Worker, move |session| {
        let cancellation = session
            .request_queue()
            .incoming()
            .cancellation_token(&task_id);
        let snapshot = if cancellation.is_some() {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                H::prepare(session, &params)
            })) {
                Ok(snapshot) => snapshot,
                Err(_) => Err(anyhow::anyhow!(
                    "background request snapshot preparation panicked"
                )),
            }
        } else {
            Err(anyhow::anyhow!(
                "background request was not registered before scheduling"
            ))
        };
        let document_version = snapshot.as_ref().ok().and_then(H::document_version);

        Box::new(move |client| {
            let Some(cancellation) = cancellation else {
                return;
            };
            if cancellation.is_cancelled() {
                return;
            }
            let response = if let Ok(response) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let result = snapshot
                        .and_then(|snapshot| H::run(snapshot, client, params, &cancellation));
                    result_response(id.clone(), result)
                })) {
                response
            } else {
                tracing::error!(%id, "background request handler panicked");
                Response::new_err(
                    id,
                    ErrorCode::InternalError as i32,
                    "background request handler panicked".to_owned(),
                )
            };
            if cancellation.is_cancelled() {
                return;
            }
            let send_result = if let Some(version) = document_version {
                client.respond_versioned(response, version)
            } else {
                client.respond(response)
            };
            if let Err(error) = send_result {
                tracing::error!(%error, "failed to queue background request response");
            }
        })
    })
}

fn result_response<R: serde::Serialize>(
    id: lsp_server::RequestId,
    result: anyhow::Result<R>,
) -> Response {
    match result {
        Ok(result) => match serde_json::to_value(result) {
            Ok(result) => Response {
                id,
                response_result: Ok(result),
            },
            Err(error) => Response::new_err(id, ErrorCode::InternalError as i32, error.to_string()),
        },
        Err(error) => {
            if let Some(error) = error.downcast_ref::<RequestError>() {
                Response::new_err(id, error.code, error.message.clone())
            } else {
                Response::new_err(id, ErrorCode::InternalError as i32, error.to_string())
            }
        }
    }
}

fn sync_notification_task<H>(notification: Notification) -> Task
where
    H: SyncNotificationHandler,
{
    let params = match serde_json::from_value(notification.params) {
        Ok(params) => params,
        Err(error) => {
            tracing::warn!(%error, "invalid notification parameters");
            return Task::nothing();
        }
    };

    Task::sync(move |session: &mut Session, client: &Client| {
        if let Err(error) = H::run(session, client, params) {
            tracing::warn!(%error, "failed to handle notification");
        }
    })
}
