//! End-to-end language-server tests over an in-memory LSP connection.

mod completion;
mod config_reload;
mod definition;
mod diagnostics;
mod document_sync;
mod hover;
mod initialize;

use std::thread::JoinHandle;

use lsp_server::{Connection, Message, RequestId, Response};
use lsp_types::{
    CancelNotification, CancelParams, ClientCapabilities, ExitNotification, InitializeParams,
    InitializeRequest, InitializeResult, InitializedNotification, InitializedParams, Notification,
    Request, ShutdownRequest, WorkspaceFolder, WorkspaceFolders,
};

use karva_language_server::{ConnectionInitializer, Server};

struct TestServer {
    connection: Option<Connection>,
    server_thread: Option<JoinHandle<anyhow::Result<()>>>,
    next_request_id: i32,
    initialization_result: InitializeResult,
}

impl TestServer {
    fn new(capabilities: ClientCapabilities) -> Self {
        Self::with_initialize_params(InitializeParams {
            capabilities,
            ..InitializeParams::default()
        })
    }

    fn with_workspace(capabilities: ClientCapabilities, folder: WorkspaceFolder) -> Self {
        let mut params = InitializeParams {
            capabilities,
            ..InitializeParams::default()
        };
        params.workspace_folders_initialize_params.workspace_folders =
            Some(WorkspaceFolders::WorkspaceFolderList(vec![folder]));
        Self::with_initialize_params(params)
    }

    fn with_initialize_params(params: InitializeParams) -> Self {
        let (server_connection, client_connection) = ConnectionInitializer::memory();
        let server_thread = std::thread::spawn(move || Server::new(server_connection)?.run());
        let mut server = Self {
            connection: Some(client_connection),
            server_thread: Some(server_thread),
            next_request_id: 0,
            initialization_result: InitializeResult::default(),
        };

        let initialization_result = server.request::<InitializeRequest>(params);
        server.notify::<InitializedNotification>(InitializedParams::default());
        server.initialization_result = initialization_result;
        server
    }

    fn initialization_result(&self) -> &InitializeResult {
        &self.initialization_result
    }

    fn send_request<R: Request>(&mut self, params: R::Params) -> RequestId {
        self.send_request_raw(R::METHOD.as_str(), params)
    }

    fn send_request_raw<T: serde::Serialize>(&mut self, method: &str, params: T) -> RequestId {
        let id = RequestId::from(self.next_request_id);
        self.next_request_id += 1;
        self.send(Message::Request(lsp_server::Request::new(
            id.clone(),
            method.to_owned(),
            params,
        )));
        id
    }

    fn request<R: Request>(&mut self, params: R::Params) -> R::Result {
        let id = self.send_request::<R>(params);
        let response = self.receive_response(&id);
        let value = response
            .response_result
            .unwrap_or_else(|error| panic!("request failed: {error:?}"));
        serde_json::from_value(value).expect("response should match the request result type")
    }

    fn request_raw(&mut self, method: &str, params: serde_json::Value) -> Response {
        let id = self.send_request_raw(method, params);
        self.receive_response(&id)
    }

    fn cancel(&self, request_id: &RequestId) {
        let id = serde_json::from_value(
            serde_json::to_value(request_id).expect("request ID should serialize"),
        )
        .expect("request ID should match the LSP cancellation ID");
        self.notify::<CancelNotification>(CancelParams { id });
    }

    fn notify<N: Notification>(&self, params: N::Params) {
        self.send(Message::Notification(lsp_server::Notification::new(
            N::METHOD.as_str().to_owned(),
            params,
        )));
    }

    fn send(&self, message: Message) {
        self.connection
            .as_ref()
            .expect("test client should be connected")
            .sender
            .send(message)
            .expect("test client should send a message");
    }

    fn receive_response(&self, expected_id: &RequestId) -> Response {
        loop {
            let message = self
                .connection
                .as_ref()
                .expect("test client should be connected")
                .receiver
                .recv()
                .expect("language server should respond");
            match message {
                Message::Response(response) => {
                    assert_eq!(&response.id, expected_id);
                    return response;
                }
                Message::Notification(_) => {}
                Message::Request(request) => {
                    panic!("expected response, received request {request:?}");
                }
            }
        }
    }

    fn receive_request<R: Request>(&self) -> (RequestId, R::Params) {
        let message = self
            .connection
            .as_ref()
            .expect("test client should be connected")
            .receiver
            .recv()
            .expect("language server should send a request");
        let Message::Request(request) = message else {
            panic!("expected request, received {message:?}");
        };
        assert_eq!(request.method, R::METHOD.as_str());
        let params = serde_json::from_value(request.params)
            .expect("request parameters should match their protocol type");
        (request.id, params)
    }

    fn receive_notification<N: Notification>(&self) -> N::Params {
        let message = self
            .connection
            .as_ref()
            .expect("test client should be connected")
            .receiver
            .recv()
            .expect("language server should send a notification");
        let Message::Notification(notification) = message else {
            panic!("expected notification, received {message:?}");
        };
        assert_eq!(notification.method, N::METHOD.as_str());
        serde_json::from_value(notification.params)
            .expect("notification parameters should match their protocol type")
    }

    fn respond<R: Request>(&self, id: RequestId, result: R::Result) {
        self.send(Message::Response(Response::new_ok(id, result)));
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.request::<ShutdownRequest>(());
        self.notify::<ExitNotification>(());
        drop(self.connection.take());

        let result = self
            .server_thread
            .take()
            .expect("server thread should exist")
            .join()
            .expect("server thread should not panic");
        result.expect("server should complete the shutdown sequence");
    }
}
