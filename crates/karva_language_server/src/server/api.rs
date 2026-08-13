use lsp_server::{ErrorCode, Request, Response};
use lsp_types::{LspRequestMethod, Request as _, ShutdownRequest};

use crate::session::Session;

mod requests;
mod traits;

use self::traits::SyncRequestHandler;

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
