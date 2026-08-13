use lsp_server::{ErrorCode, Notification, Request, Response};
use lsp_types::{
    DidChangeTextDocumentNotification, DidChangeWatchedFilesNotification,
    DidChangeWorkspaceFoldersNotification, DidCloseTextDocumentNotification,
    DidOpenTextDocumentNotification, LspNotificationMethod, LspRequestMethod, Notification as _,
    Request as _, ShutdownRequest,
};

use crate::session::Session;

mod notifications;
mod requests;
mod traits;

use self::traits::{SyncNotificationHandler, SyncRequestHandler};

pub(super) fn request(request: Request, session: &mut Session) -> Response {
    match LspRequestMethod::from(request.method.as_str()) {
        ShutdownRequest::METHOD => run::<requests::Shutdown>(request, session),
        method => Response::new_err(
            request.id,
            ErrorCode::MethodNotFound as i32,
            format!("unknown request: {method}"),
        ),
    }
}

pub(super) fn notification(notification: Notification, session: &mut Session) {
    let result = match LspNotificationMethod::from(notification.method.as_str()) {
        DidOpenTextDocumentNotification::METHOD => {
            run_notification::<notifications::DidOpen>(notification, session)
        }
        DidChangeTextDocumentNotification::METHOD => {
            run_notification::<notifications::DidChange>(notification, session)
        }
        DidCloseTextDocumentNotification::METHOD => {
            run_notification::<notifications::DidClose>(notification, session)
        }
        DidChangeWorkspaceFoldersNotification::METHOD => {
            run_notification::<notifications::DidChangeWorkspaceFolders>(notification, session)
        }
        DidChangeWatchedFilesNotification::METHOD => {
            run_notification::<notifications::DidChangeWatchedFiles>(notification, session)
        }
        method => {
            tracing::debug!("ignoring unsupported notification {method}");
            return;
        }
    };

    if let Err(error) = result {
        tracing::warn!("failed to handle notification: {error}");
    }
}

fn run<H>(request: Request, session: &mut Session) -> Response
where
    H: SyncRequestHandler,
{
    let id = request.id;
    let params = match serde_json::from_value(request.params) {
        Ok(params) => params,
        Err(error) => {
            return Response::new_err(id, ErrorCode::InvalidParams as i32, error.to_string());
        }
    };

    match H::run(session, params) {
        Ok(result) => match serde_json::to_value(result) {
            Ok(result) => Response::new_ok(id, result),
            Err(error) => Response::new_err(id, ErrorCode::InternalError as i32, error.to_string()),
        },
        Err(error) => Response::new_err(id, ErrorCode::InternalError as i32, error.to_string()),
    }
}

fn run_notification<H>(notification: Notification, session: &mut Session) -> anyhow::Result<()>
where
    H: SyncNotificationHandler,
{
    let params = serde_json::from_value(notification.params)?;
    H::run(session, params)
}
