//! End-to-end language-server tests over an in-memory LSP connection.
//!
//! [`TestServer`] deliberately models a client rather than assuming that each request is the
//! next message on the wire. Servers may publish diagnostics or ask for configuration while a
//! request is in flight, so the harness queues unrelated messages and matches by method or ID.

mod completion;
mod config_reload;
mod definition;
mod diagnostics;
mod document_highlight;
mod document_symbols;
mod document_sync;
mod hover;
mod implementation;
mod initialize;
mod references;
mod rename;
mod runnables;

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use crossbeam_channel::RecvTimeoutError;
use lsp_server::{Connection, Message, RequestId, Response};
use lsp_types::{
    ClientCapabilities, ExitNotification, InitializeParams, InitializeRequest, InitializeResult,
    InitializedNotification, InitializedParams, Notification, Request, ShutdownRequest, Uri,
    WorkspaceFolder, WorkspaceFolders, WorkspaceFoldersInitializeParams,
};
use serde_json::Value;
use tempfile::{TempDir, tempdir};

use karva_language_server::{ConnectionInitializer, Server};

// Receipt: the full focused suite's slowest test took 70 ms on 2026-08-13. These tripwires leave
// more than one order of magnitude for slower CI while still making a stalled server fail quickly.
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(10);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Errors reported when no matching server message arrives.
#[derive(Debug, thiserror::Error)]
enum ReceiveError {
    #[error("waiting for server message timed out after {0:?}")]
    Timeout(Duration),

    #[error("language server disconnected while waiting for a message")]
    Disconnected,
}

/// Test client for an in-process language server.
pub(crate) struct TestServer {
    connection: Option<Connection>,
    server_thread: Option<JoinHandle<anyhow::Result<()>>>,
    next_request_id: i32,
    initialization_result: InitializeResult,
    responses: HashMap<RequestId, Vec<Response>>,
    notifications: VecDeque<lsp_server::Notification>,
    requests: VecDeque<lsp_server::Request>,
}

impl TestServer {
    pub(crate) fn new(capabilities: ClientCapabilities) -> Self {
        TestServerBuilder::new()
            .with_client_capabilities(capabilities)
            .build()
    }

    pub(crate) fn with_workspace(
        capabilities: ClientCapabilities,
        folder: WorkspaceFolder,
    ) -> Self {
        TestServerBuilder::new()
            .with_client_capabilities(capabilities)
            .with_workspace(folder)
            .build()
    }

    fn from_builder(builder: TestServerBuilder) -> Self {
        let (server_connection, client_connection) = ConnectionInitializer::memory();
        let server_thread = std::thread::spawn(move || Server::new(server_connection)?.run());
        let mut server = Self {
            connection: Some(client_connection),
            server_thread: Some(server_thread),
            next_request_id: 0,
            initialization_result: InitializeResult::default(),
            responses: HashMap::new(),
            notifications: VecDeque::new(),
            requests: VecDeque::new(),
        };

        let mut params = builder.initialize_params;
        params.capabilities = builder.client_capabilities;
        params.workspace_folders_initialize_params = WorkspaceFoldersInitializeParams {
            workspace_folders: Some(WorkspaceFolders::WorkspaceFolderList(
                builder.workspace_folders,
            )),
        };

        let initialization_result = server.request::<InitializeRequest>(params);
        server.notify::<InitializedNotification>(InitializedParams::default());
        server.initialization_result = initialization_result;
        server
    }

    pub(crate) fn initialization_result(&self) -> &InitializeResult {
        &self.initialization_result
    }

    pub(crate) fn send_request<R: Request>(&mut self, params: R::Params) -> RequestId {
        self.send_request_raw(R::METHOD.as_str(), params)
    }

    pub(crate) fn send_request_raw<T: serde::Serialize>(
        &mut self,
        method: &str,
        params: T,
    ) -> RequestId {
        let id = RequestId::from(self.next_request_id);
        self.next_request_id += 1;
        self.send(Message::Request(lsp_server::Request::new(
            id.clone(),
            method.to_owned(),
            params,
        )));
        id
    }

    #[track_caller]
    pub(crate) fn request<R: Request>(&mut self, params: R::Params) -> R::Result {
        let id = self.send_request::<R>(params);
        let response = self.receive_response(&id);
        let value = response
            .response_result
            .unwrap_or_else(|error| panic!("request {} failed: {error:?}", R::METHOD));
        serde_json::from_value(value).unwrap_or_else(|error| {
            panic!("response for {} had invalid result: {error}", R::METHOD)
        })
    }

    #[track_caller]
    pub(crate) fn request_raw(&mut self, method: &str, params: Value) -> Response {
        let id = self.send_request_raw(method, params);
        self.receive_response(&id)
    }

    pub(crate) fn cancel(&self, request_id: &RequestId) {
        let id = serde_json::from_value(
            serde_json::to_value(request_id).expect("request ID should serialize"),
        )
        .expect("request ID should match LSP cancellation ID");
        self.notify::<lsp_types::CancelNotification>(lsp_types::CancelParams { id });
    }

    pub(crate) fn notify<N: Notification>(&self, params: N::Params) {
        self.send(Message::Notification(lsp_server::Notification::new(
            N::METHOD.as_str().to_owned(),
            params,
        )));
    }

    fn send(&self, message: Message) {
        let Some(connection) = self.connection.as_ref() else {
            panic!("test client connection already closed")
        };
        connection
            .sender
            .send(message)
            .unwrap_or_else(|error| panic!("failed to send message to language server: {error}"));
    }

    #[track_caller]
    pub(crate) fn receive_response(&mut self, expected_id: &RequestId) -> Response {
        self.try_receive_response(expected_id, None)
            .unwrap_or_else(|error| {
                panic!("failed to receive response for request {expected_id}: {error}")
            })
    }

    fn try_receive_response(
        &mut self,
        expected_id: &RequestId,
        timeout: Option<Duration>,
    ) -> Result<Response> {
        let timeout = timeout.unwrap_or(RECEIVE_TIMEOUT);
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(mut responses) = self.responses.remove(expected_id) {
                if responses.len() != 1 {
                    return Err(anyhow!(
                        "received {} responses for request {expected_id}",
                        responses.len()
                    ));
                }
                return responses.pop().context("response queue unexpectedly empty");
            }
            self.receive(deadline, timeout)
                .map_err(|error| anyhow!(error))?;
        }
    }

    #[track_caller]
    pub(crate) fn receive_request<R: Request>(&mut self) -> (RequestId, R::Params) {
        self.try_receive_request::<R>(None).unwrap_or_else(|error| {
            panic!("failed to receive server request {}: {error}", R::METHOD)
        })
    }

    fn try_receive_request<R: Request>(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<(RequestId, R::Params)> {
        let timeout = timeout.unwrap_or(RECEIVE_TIMEOUT);
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(index) = self
                .requests
                .iter()
                .position(|request| request.method == R::METHOD.as_str())
            {
                let request = self
                    .requests
                    .remove(index)
                    .context("request queue unexpectedly empty")?;
                let params = serde_json::from_value(request.params)
                    .with_context(|| format!("invalid parameters for {}", R::METHOD))?;
                return Ok((request.id, params));
            }
            self.receive(deadline, timeout)
                .map_err(|error| anyhow!(error))?;
        }
    }

    #[track_caller]
    pub(crate) fn receive_notification<N: Notification>(&mut self) -> N::Params {
        self.try_receive_notification::<N>(None)
            .unwrap_or_else(|error| {
                panic!(
                    "failed to receive server notification {}: {error}",
                    N::METHOD
                )
            })
    }

    fn try_receive_notification<N: Notification>(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<N::Params> {
        let timeout = timeout.unwrap_or(RECEIVE_TIMEOUT);
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(index) = self
                .notifications
                .iter()
                .position(|notification| notification.method == N::METHOD.as_str())
            {
                let notification = self
                    .notifications
                    .remove(index)
                    .context("notification queue unexpectedly empty")?;
                return serde_json::from_value(notification.params)
                    .with_context(|| format!("invalid parameters for {}", N::METHOD));
            }
            self.receive(deadline, timeout)
                .map_err(|error| anyhow!(error))?;
        }
    }

    fn receive(
        &mut self,
        deadline: Instant,
        timeout: Duration,
    ) -> std::result::Result<(), ReceiveError> {
        let Some(connection) = self.connection.as_ref() else {
            return Err(ReceiveError::Disconnected);
        };
        let receiver = connection.receiver.clone();
        let message = receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => ReceiveError::Timeout(timeout),
                RecvTimeoutError::Disconnected => ReceiveError::Disconnected,
            })?;
        self.queue_message(message);
        while let Ok(message) = receiver.try_recv() {
            self.queue_message(message);
        }
        Ok(())
    }

    fn queue_message(&mut self, message: Message) {
        match message {
            Message::Request(request) => self.requests.push_back(request),
            Message::Response(response) => {
                self.responses
                    .entry(response.id.clone())
                    .or_default()
                    .push(response);
            }
            Message::Notification(notification) => self.notifications.push_back(notification),
        }
    }

    fn assert_no_pending_messages(&self) {
        assert!(
            self.responses.is_empty(),
            "language server left unclaimed responses: {:?}",
            self.responses.keys().collect::<Vec<_>>()
        );
        assert!(
            self.requests.is_empty(),
            "language server left unclaimed requests: {:?}",
            self.requests
                .iter()
                .map(|request| request.method.as_str())
                .collect::<Vec<_>>()
        );
        assert!(
            self.notifications.is_empty(),
            "language server left unclaimed notifications: {:?}",
            self.notifications
                .iter()
                .map(|notification| notification.method.as_str())
                .collect::<Vec<_>>()
        );
    }

    pub(crate) fn respond<R: Request>(&self, id: RequestId, result: R::Result) {
        self.send(Message::Response(Response::new_ok(id, result)));
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.connection.take();
            return;
        }

        let shutdown_result = if self.server_thread.is_some() {
            let id = self.send_request::<ShutdownRequest>(());
            let result = self.try_receive_response(&id, None).map(|_| ());
            if result.is_ok() {
                self.notify::<ExitNotification>(());
            }
            result
        } else {
            Ok(())
        };

        let server_messages = self.connection.take().map(|connection| {
            drop(connection.sender);
            connection.receiver
        });
        let Some(server_thread) = self.server_thread.take() else {
            if let Some(receiver) = server_messages {
                while let Ok(message) = receiver.try_recv() {
                    self.queue_message(message);
                }
            }
            self.assert_no_pending_messages();
            return;
        };
        let (join_sender, join_receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = server_thread.join();
            let _ = join_sender.send(result);
        });
        match join_receiver.recv_timeout(SHUTDOWN_TIMEOUT) {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => panic!("language server shutdown failed: {error}"),
            Ok(Err(_)) => panic!("language server thread panicked during shutdown"),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("language server did not shut down within {SHUTDOWN_TIMEOUT:?}")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("shutdown waiter disconnected before joining server thread")
            }
        }

        if let Some(receiver) = server_messages {
            while let Ok(message) = receiver.try_recv() {
                self.queue_message(message);
            }
        }
        self.assert_no_pending_messages();

        if let Err(error) = shutdown_result {
            panic!("language server shutdown request failed: {error:#}");
        }
    }
}

/// Builder for deterministic in-memory LSP servers.
pub(crate) struct TestServerBuilder {
    client_capabilities: ClientCapabilities,
    workspace_folders: Vec<WorkspaceFolder>,
    initialize_params: InitializeParams,
}

impl TestServerBuilder {
    pub(crate) fn new() -> Self {
        Self {
            client_capabilities: ClientCapabilities::default(),
            workspace_folders: Vec::new(),
            initialize_params: InitializeParams::default(),
        }
    }

    pub(crate) fn with_client_capabilities(mut self, capabilities: ClientCapabilities) -> Self {
        self.client_capabilities = capabilities;
        self
    }

    pub(crate) fn with_workspace(mut self, folder: WorkspaceFolder) -> Self {
        self.workspace_folders.push(folder);
        self
    }

    pub(crate) fn with_initialization_options(mut self, options: Value) -> Self {
        self.initialize_params.initialization_options = Some(options);
        self
    }

    pub(crate) fn build(self) -> TestServer {
        TestServer::from_builder(self)
    }
}

/// Temporary project shared by an e2e test's files, URIs, and snapshots.
pub(crate) struct TestContext {
    directory: TempDir,
    root_uri: Uri,
}

pub(crate) type Workspace = TestContext;

impl TestContext {
    pub(crate) fn new() -> Self {
        let directory = tempdir().expect("temporary workspace should be created");
        fs::create_dir(directory.path().join(".git")).expect("workspace marker should be created");
        let root_uri =
            Uri::from_file_path(directory.path()).expect("workspace URI should be valid");
        Self {
            directory,
            root_uri,
        }
    }

    pub(crate) fn folder(&self) -> WorkspaceFolder {
        WorkspaceFolder {
            uri: self.root_uri.clone(),
            name: "project".to_owned(),
        }
    }

    pub(crate) fn uri(&self, relative: &str) -> Uri {
        Uri::from_file_path(self.directory.path().join(relative))
            .expect("document URI should be valid")
    }

    pub(crate) fn write(&self, relative: &str, source: &str) {
        let path = self.directory.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("workspace source parent should be created");
        }
        fs::write(path, source).expect("workspace source should be written");
    }

    pub(crate) fn normalize(&self, value: impl serde::Serialize) -> Value {
        let mut value = serde_json::to_value(value).expect("e2e value should serialize");
        normalize_paths(
            &mut value,
            self.root_uri.as_str(),
            &self.directory.path().to_string_lossy(),
        );
        value
    }
}

fn normalize_paths(value: &mut Value, workspace_uri: &str, workspace_path: &str) {
    match value {
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| normalize_paths(value, workspace_uri, workspace_path)),
        Value::Object(values) => {
            let original = std::mem::take(values);
            for (key, mut value) in original {
                normalize_paths(&mut value, workspace_uri, workspace_path);
                let contains_workspace =
                    key.contains(workspace_uri) || key.contains(workspace_path);
                let mut key = key
                    .replace(workspace_uri, "file:///project")
                    .replace(workspace_path, "/project");
                if contains_workspace {
                    key = key.replace('\\', "/");
                }
                values.insert(key, value);
            }
        }
        Value::String(value) => {
            let contains_workspace =
                value.contains(workspace_uri) || value.contains(workspace_path);
            *value = value
                .replace(workspace_uri, "file:///project")
                .replace(workspace_path, "/project");
            if contains_workspace {
                *value = value.replace('\\', "/");
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod harness_tests {
    use lsp_types::{
        ConfigurationParams, ConfigurationRequest, MessageType, ShowMessageNotification,
        ShowMessageParams,
    };

    use super::*;

    #[test]
    fn preserves_mixed_messages_while_correlating_responses() {
        let mut server = detached_server(None);
        let first = RequestId::from(1);
        let second = RequestId::from(2);
        let request = RequestId::from(3);
        server.queue_message(Message::Notification(lsp_server::Notification::new(
            ShowMessageNotification::METHOD.as_str().to_owned(),
            ShowMessageParams {
                kind: MessageType::Info,
                message: "ready".to_owned(),
            },
        )));
        server.queue_message(Message::Request(lsp_server::Request::new(
            request.clone(),
            ConfigurationRequest::METHOD.as_str().to_owned(),
            ConfigurationParams { items: Vec::new() },
        )));
        server.queue_message(Message::Response(Response::new_ok(first.clone(), "first")));
        server.queue_message(Message::Response(Response::new_ok(
            second.clone(),
            "second",
        )));

        assert_eq!(
            server
                .try_receive_response(&second, Some(Duration::ZERO))
                .expect("queued response should be available")
                .id,
            second
        );
        assert_eq!(
            server
                .try_receive_notification::<ShowMessageNotification>(Some(Duration::ZERO))
                .expect("queued notification should be available")
                .message,
            "ready"
        );
        assert_eq!(
            server
                .try_receive_request::<ConfigurationRequest>(Some(Duration::ZERO))
                .expect("queued request should be available")
                .0,
            request
        );
        assert_eq!(
            server
                .try_receive_response(&first, Some(Duration::ZERO))
                .expect("queued response should be preserved")
                .id,
            first
        );
    }

    #[test]
    fn reports_receive_timeout() {
        let (server_connection, client_connection) = Connection::memory();
        let mut server = detached_server(Some(client_connection));

        let error = server
            .try_receive_notification::<ShowMessageNotification>(Some(Duration::ZERO))
            .expect_err("empty connected channel should time out");

        assert!(matches!(
            error.downcast_ref::<ReceiveError>(),
            Some(ReceiveError::Timeout(timeout)) if timeout.is_zero()
        ));
        drop(server_connection);
    }

    #[test]
    fn reports_server_disconnect() {
        let (server_connection, client_connection) = Connection::memory();
        drop(server_connection);
        let mut server = detached_server(Some(client_connection));

        let error = server
            .try_receive_notification::<ShowMessageNotification>(Some(Duration::ZERO))
            .expect_err("disconnected channel should fail");

        assert!(matches!(
            error.downcast_ref::<ReceiveError>(),
            Some(ReceiveError::Disconnected)
        ));
    }

    fn detached_server(connection: Option<Connection>) -> TestServer {
        TestServer {
            connection,
            server_thread: None,
            next_request_id: 0,
            initialization_result: InitializeResult::default(),
            responses: HashMap::new(),
            notifications: VecDeque::new(),
            requests: VecDeque::new(),
        }
    }
}
