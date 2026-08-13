//! End-to-end language-server tests over an in-memory LSP connection.

mod initialize;

use std::thread::JoinHandle;

use lsp_server::{Connection, Message, RequestId, Response};
use lsp_types::{
    ClientCapabilities, ExitNotification, InitializeParams, InitializeRequest, InitializeResult,
    InitializedNotification, InitializedParams, Notification, Request, ShutdownRequest,
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
        let (server_connection, client_connection) = ConnectionInitializer::memory();
        let server_thread = std::thread::spawn(move || Server::new(server_connection)?.run());
        let mut server = Self {
            connection: Some(client_connection),
            server_thread: Some(server_thread),
            next_request_id: 0,
            initialization_result: InitializeResult::default(),
        };

        let initialization_result = server.request::<InitializeRequest>(InitializeParams {
            capabilities,
            ..InitializeParams::default()
        });
        server.notify::<InitializedNotification>(InitializedParams::default());
        server.initialization_result = initialization_result;
        server
    }

    fn initialization_result(&self) -> &InitializeResult {
        &self.initialization_result
    }

    fn request<R: Request>(&mut self, params: R::Params) -> R::Result {
        let id = RequestId::from(self.next_request_id);
        self.next_request_id += 1;
        self.send(Message::Request(lsp_server::Request::new(
            id.clone(),
            R::METHOD.as_str().to_owned(),
            params,
        )));
        let response = self.receive_response(&id);
        let value = response
            .response_result
            .unwrap_or_else(|error| panic!("request failed: {error:?}"));
        serde_json::from_value(value).expect("response should match the request result type")
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
        let message = self
            .connection
            .as_ref()
            .expect("test client should be connected")
            .receiver
            .recv()
            .expect("language server should respond");
        let Message::Response(response) = message else {
            panic!("expected response, received {message:?}");
        };
        assert_eq!(&response.id, expected_id);
        response
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
