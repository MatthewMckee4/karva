//! Private, typed communication between Karva's controller and workers.
//!
//! Workers connect over loopback TCP so transient coordination never touches
//! the project filesystem. The controller sends each worker's test selection,
//! then workers stream lifecycle and result events back. Keeping the wire
//! format serde-based avoids platform-specific pipe implementations.

use std::collections::{HashMap, HashSet};
use std::io::{BufReader, BufWriter, ErrorKind, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use karva_diagnostic::{RenderedDiagnostic, TestCaseResult};
use karva_python_semantic::TestCacheKey;
use serde::{Deserialize, Serialize};

/// One runtime state change sent from a worker to the controller.
#[derive(Serialize, Deserialize)]
pub enum WorkerEvent {
    /// Test began executing on this worker.
    TestStarted {
        name: String,
        cache_key: TestCacheKey,
    },

    /// Test exceeded the configured slow-test threshold.
    TestSlow,

    /// Test completed with its transport-safe result.
    TestFinished {
        cache_key: TestCacheKey,
        result: Box<TestCaseResult>,
    },

    /// Diagnostic describing the run rather than one test.
    RunDiagnostic(RenderedDiagnostic),

    /// Worker completed normally after sending every result.
    WorkerFinished,
}

#[derive(Serialize, Deserialize)]
enum WireMessage {
    Hello { run_id: String, worker_id: usize },
    TestSelection(WorkerSelection),
    Event(Box<WorkerEvent>),
}

/// Work owned by one worker generation.
#[derive(Serialize, Deserialize)]
pub struct WorkerSelection {
    /// Exact test selectors in execution order.
    pub test_paths: Vec<String>,

    /// Runtime-expanded cases already completed by an earlier generation.
    pub resume_skip: Vec<TestCacheKey>,
}

enum Incoming {
    Connected { worker_id: usize },
    Disconnected { worker_id: usize },
    Event(ControllerEvent),
    Error(String),
}

/// Event attributed to the worker authenticated by the stream handshake.
pub struct ControllerEvent {
    /// Worker number established by the connection handshake.
    pub worker_id: usize,

    /// Runtime state change received from that worker.
    pub event: Box<WorkerEvent>,
}

/// Cloneable worker-side writer shared with execution reporters.
#[derive(Clone)]
pub struct WorkerClient {
    connection: Arc<WorkerConnection>,
}

struct WorkerConnection {
    writer: Arc<Mutex<BufWriter<TcpStream>>>,
    current_test: Arc<Mutex<CurrentTestState>>,
    stop: Arc<AtomicBool>,
    flush_error: Arc<Mutex<Option<String>>>,
    flusher: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Default)]
struct CurrentTestState {
    latest: Option<CurrentTest>,
    sent: Option<CurrentTest>,
}

#[derive(Clone, PartialEq, Eq)]
struct CurrentTest {
    name: String,
    cache_key: TestCacheKey,
}

impl WorkerClient {
    /// Connects to the controller and receives this worker's owned test selection.
    pub fn connect(
        address: SocketAddr,
        run_id: &str,
        worker_id: usize,
    ) -> Result<(Self, WorkerSelection)> {
        let stream = TcpStream::connect(address)
            .with_context(|| format!("failed to connect to Karva controller at {address}"))?;
        stream
            .set_nodelay(true)
            .context("failed to configure Karva controller connection")?;
        let reader = stream
            .try_clone()
            .context("failed to read Karva controller connection")?;
        let writer = Arc::new(Mutex::new(BufWriter::new(stream)));
        let current_test = Arc::new(Mutex::new(CurrentTestState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let flush_error = Arc::new(Mutex::new(None));
        let flusher = spawn_flusher(&writer, &current_test, &stop, &flush_error)?;
        let client = Self {
            connection: Arc::new(WorkerConnection {
                writer,
                current_test,
                stop,
                flush_error,
                flusher: Mutex::new(Some(flusher)),
            }),
        };
        client.write(
            &WireMessage::Hello {
                run_id: run_id.to_string(),
                worker_id,
            },
            true,
        )?;
        let selection = read_test_selection(reader)?;
        Ok((client, selection))
    }

    /// Queues one state change, flushing completed tests before execution advances.
    pub fn send_event(&self, event: WorkerEvent) -> Result<()> {
        let mut current_test = self
            .connection
            .current_test
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva worker current-test lock poisoned"))?;
        if let WorkerEvent::TestStarted { name, cache_key } = &event {
            let next = CurrentTest {
                name: name.clone(),
                cache_key: cache_key.clone(),
            };
            if current_test.sent.as_ref() == Some(&next) {
                return Ok(());
            }
            current_test.latest = Some(next);
            drop(current_test);
            self.flush_current_test()?;
            return self.flush();
        }
        if matches!(event, WorkerEvent::TestFinished { .. }) {
            current_test.latest = None;
            current_test.sent = None;
        }
        let flush = matches!(&event, WorkerEvent::TestFinished { .. });
        self.write(&WireMessage::Event(Box::new(event)), flush)
    }

    /// Marks the worker complete and gracefully closes the connection.
    pub fn complete(self) -> Result<()> {
        self.flush_current_test()?;
        self.write(
            &WireMessage::Event(Box::new(WorkerEvent::WorkerFinished)),
            false,
        )?;
        self.flush()?;
        self.connection.stop.store(true, Ordering::Release);
        let flusher = self
            .connection
            .flusher
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?
            .take();
        if let Some(flusher) = flusher {
            flusher.thread().unpark();
            if flusher.join().is_err() {
                bail!("Karva worker event flusher panicked");
            }
        }
        self.check_flush_error()?;
        self.connection
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?
            .get_ref()
            .shutdown(Shutdown::Both)
            .context("failed to close Karva controller connection")
    }

    fn write(&self, message: &WireMessage, flush: bool) -> Result<()> {
        self.check_flush_error()?;
        let mut writer = self
            .connection
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?;
        serde_json::to_writer(&mut *writer, message)
            .context("failed to serialize Karva worker event")?;
        writer
            .write_all(b"\n")
            .context("failed to frame Karva worker event")?;
        if flush {
            writer
                .flush()
                .context("failed to send Karva worker event")?;
        }
        Ok(())
    }

    fn flush(&self) -> Result<()> {
        self.connection
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?
            .flush()
            .context("failed to send Karva worker event")
    }

    fn flush_current_test(&self) -> Result<()> {
        let mut current_test = self
            .connection
            .current_test
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva worker current-test lock poisoned"))?;
        if current_test.latest != current_test.sent {
            if let Some(test) = current_test.latest.as_ref() {
                self.write(
                    &WireMessage::Event(Box::new(WorkerEvent::TestStarted {
                        name: test.name.clone(),
                        cache_key: test.cache_key.clone(),
                    })),
                    false,
                )?;
            }
            let latest = current_test.latest.clone();
            current_test.sent.clone_from(&latest);
        }
        Ok(())
    }

    fn check_flush_error(&self) -> Result<()> {
        if let Some(error) = self
            .connection
            .flush_error
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?
            .as_ref()
        {
            bail!("failed to send Karva worker event: {error}");
        }
        Ok(())
    }
}

fn read_test_selection(stream: TcpStream) -> Result<WorkerSelection> {
    let mut messages =
        serde_json::Deserializer::from_reader(BufReader::new(stream)).into_iter::<WireMessage>();
    let Some(message) = messages.next() else {
        bail!("Karva controller connection closed before sending test paths");
    };
    match message.context("failed to read Karva worker test paths")? {
        WireMessage::TestSelection(selection) => Ok(selection),
        WireMessage::Hello { .. } | WireMessage::Event(_) => {
            bail!("Karva controller sent an invalid worker startup message")
        }
    }
}

// Receipt: synchronous flushing of every event made the 16,807-case parametrized
// benchmark 40.5% slower. Lifecycle starts remain coalesced; terminal results
// flush immediately so a later fixture teardown crash cannot erase a PASS.
const EVENT_FLUSH_INTERVAL: Duration = Duration::from_millis(10);

fn spawn_flusher(
    writer: &Arc<Mutex<BufWriter<TcpStream>>>,
    current_test: &Arc<Mutex<CurrentTestState>>,
    stop: &Arc<AtomicBool>,
    flush_error: &Arc<Mutex<Option<String>>>,
) -> Result<JoinHandle<()>> {
    let writer = Arc::clone(writer);
    let current_test = Arc::clone(current_test);
    let stop = Arc::clone(stop);
    let flush_error = Arc::clone(flush_error);
    thread::Builder::new()
        .name("karva-event-flusher".to_string())
        .spawn(move || {
            while !stop.load(Ordering::Acquire) {
                thread::park_timeout(EVENT_FLUSH_INTERVAL);
                if stop.load(Ordering::Acquire) {
                    return;
                }
                let result = flush_worker_events(&writer, &current_test);
                if let Err(error) = result {
                    if let Ok(mut flush_error) = flush_error.lock() {
                        *flush_error = Some(error);
                    }
                    return;
                }
            }
        })
        .context("failed to start Karva worker event flusher")
}

fn flush_worker_events(
    writer: &Mutex<BufWriter<TcpStream>>,
    current_test: &Mutex<CurrentTestState>,
) -> Result<(), String> {
    let mut current_test = current_test
        .lock()
        .map_err(|_| "Karva worker current-test lock poisoned".to_string())?;
    let mut writer = writer
        .lock()
        .map_err(|_| "Karva controller connection lock poisoned".to_string())?;
    if current_test.latest != current_test.sent {
        if let Some(test) = current_test.latest.as_ref() {
            serde_json::to_writer(
                &mut *writer,
                &WireMessage::Event(Box::new(WorkerEvent::TestStarted {
                    name: test.name.clone(),
                    cache_key: test.cache_key.clone(),
                })),
            )
            .map_err(|error| error.to_string())?;
            writer.write_all(b"\n").map_err(|error| error.to_string())?;
        }
        let latest = current_test.latest.clone();
        current_test.sent.clone_from(&latest);
    }
    writer.flush().map_err(|error| error.to_string())
}

impl Drop for WorkerConnection {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(flusher) = self.flusher.get_mut()
            && let Some(flusher) = flusher.take()
        {
            flusher.thread().unpark();
            flusher.join().ok();
        }
    }
}

/// Controller-side loopback listener and active worker stream readers.
pub struct ControllerServer {
    listener: TcpListener,
    run_id: String,
    sender: Sender<Incoming>,
    receiver: Receiver<Incoming>,
    readers: Vec<JoinHandle<()>>,
    workers: HashSet<usize>,
    disconnected_workers: HashSet<usize>,
    worker_selections: Arc<Mutex<HashMap<usize, WorkerSelection>>>,
}

impl ControllerServer {
    /// Binds an operating-system-selected loopback port for one test run.
    pub fn bind(run_id: &str) -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .context("failed to bind Karva controller listener")?;
        listener
            .set_nonblocking(true)
            .context("failed to configure Karva controller listener")?;
        let (sender, receiver) = unbounded();
        Ok(Self {
            listener,
            run_id: run_id.to_string(),
            sender,
            receiver,
            readers: Vec::new(),
            workers: HashSet::new(),
            disconnected_workers: HashSet::new(),
            worker_selections: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Registers one worker's owned test selection before spawning it.
    ///
    /// Ownership moves into the connection reader so large selections are not
    /// cloned or collected into an encoded buffer on the controller.
    pub fn register_worker_selection(
        &mut self,
        worker_id: usize,
        selection: WorkerSelection,
    ) -> Result<()> {
        let previous = self
            .worker_selections
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva worker selection lock poisoned"))?
            .insert(worker_id, selection);
        if previous.is_some() {
            bail!("Karva worker {worker_id} selection registered more than once");
        }
        Ok(())
    }

    /// Returns address passed to newly spawned workers.
    pub fn address(&self) -> Result<SocketAddr> {
        self.listener
            .local_addr()
            .context("failed to read Karva controller listener address")
    }

    /// Accepts every connection already queued by the operating system.
    pub fn accept_pending(&mut self) -> Result<()> {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .context("failed to configure Karva worker connection")?;
                    let run_id = self.run_id.clone();
                    let sender = self.sender.clone();
                    let worker_selections = Arc::clone(&self.worker_selections);
                    self.readers.push(thread::spawn(move || {
                        if let Err(error) =
                            read_worker(stream, &run_id, &worker_selections, &sender)
                        {
                            sender.send(Incoming::Error(format!("{error:#}"))).ok();
                        }
                    }));
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(error) => {
                    return Err(error).context("failed to accept Karva worker connection");
                }
            }
        }
    }

    /// Returns next queued worker event without blocking.
    pub fn try_recv(&mut self) -> Result<Option<ControllerEvent>> {
        loop {
            match self.receiver.try_recv() {
                Ok(Incoming::Connected { worker_id }) => {
                    if !self.workers.insert(worker_id) {
                        bail!("Karva worker {worker_id} connected more than once");
                    }
                }
                Ok(Incoming::Disconnected { worker_id }) => {
                    self.disconnected_workers.insert(worker_id);
                }
                Ok(Incoming::Event(event)) => return Ok(Some(event)),
                Ok(Incoming::Error(error)) => bail!(error),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(None),
            }
        }
    }

    /// Whether the worker's event stream reached EOF after every queued event.
    pub fn worker_disconnected(&self, worker_id: usize) -> bool {
        self.disconnected_workers.contains(&worker_id)
    }

    /// Whether the worker began or completed its controller handshake.
    pub fn worker_started(&self, worker_id: usize) -> Result<bool> {
        if self.workers.contains(&worker_id) {
            return Ok(true);
        }
        let pending = self
            .worker_selections
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva worker selection lock poisoned"))?
            .contains_key(&worker_id);
        Ok(!pending)
    }

    /// Joins every accepted reader after worker processes have exited.
    pub fn finish(&mut self) -> Result<()> {
        self.accept_pending()?;
        for reader in self.readers.drain(..) {
            if reader.join().is_err() {
                bail!("Karva worker connection reader panicked");
            }
        }
        Ok(())
    }
}

fn read_worker(
    stream: TcpStream,
    expected_run_id: &str,
    worker_selections: &Mutex<HashMap<usize, WorkerSelection>>,
    sender: &Sender<Incoming>,
) -> Result<()> {
    let response_stream = stream
        .try_clone()
        .context("failed to write Karva worker connection")?;
    let mut messages =
        serde_json::Deserializer::from_reader(BufReader::new(stream)).into_iter::<WireMessage>();
    let Some(first) = messages.next() else {
        bail!("Karva worker connection closed before handshake");
    };
    let WireMessage::Hello { run_id, worker_id } =
        first.context("failed to read Karva worker handshake")?
    else {
        bail!("Karva worker sent an event before its handshake");
    };
    if run_id != expected_run_id {
        bail!("Karva worker connected with run id `{run_id}`, expected `{expected_run_id}`");
    }
    let selection = worker_selections
        .lock()
        .map_err(|_| anyhow::anyhow!("Karva worker selection lock poisoned"))?
        .remove(&worker_id)
        .with_context(|| {
            format!("Karva worker {worker_id} connected without a registered selection")
        })?;
    if sender.send(Incoming::Connected { worker_id }).is_err() {
        return Ok(());
    }
    let mut writer = BufWriter::new(response_stream);
    let selection_result =
        serde_json::to_writer(&mut writer, &WireMessage::TestSelection(selection))
            .context("failed to serialize Karva worker selection")
            .and_then(|()| {
                writer
                    .write_all(b"\n")
                    .context("failed to frame Karva worker selection")?;
                writer
                    .flush()
                    .context("failed to send Karva worker selection")
            });
    if selection_result.is_err() {
        sender.send(Incoming::Disconnected { worker_id }).ok();
        return Ok(());
    }

    for message in messages {
        let message = match message {
            Ok(message) => message,
            Err(error) if error.is_eof() || is_clean_disconnect(&error) => break,
            Err(error) => return Err(error).context("failed to read Karva worker event"),
        };
        let event = match message {
            WireMessage::Event(event) => event,
            WireMessage::Hello { .. } => bail!("Karva worker sent more than one handshake"),
            WireMessage::TestSelection(_) => {
                bail!("Karva worker sent a test selection to its controller")
            }
        };
        if sender
            .send(Incoming::Event(ControllerEvent { worker_id, event }))
            .is_err()
        {
            return Ok(());
        }
    }
    sender.send(Incoming::Disconnected { worker_id }).ok();
    Ok(())
}

fn is_clean_disconnect(error: &serde_json::Error) -> bool {
    matches!(
        error.io_error_kind(),
        Some(ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted)
    )
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, Read as _};

    use rstest::rstest;

    use super::*;

    fn selection(test_paths: Vec<String>) -> WorkerSelection {
        WorkerSelection {
            test_paths,
            resume_skip: Vec::new(),
        }
    }

    fn accept_connections(server: &mut ControllerServer, count: usize) {
        while server.readers.len() < count {
            server.accept_pending().expect("accept worker");
            thread::yield_now();
        }
    }

    #[test]
    fn streams_attributed_worker_events() {
        let mut server = ControllerServer::bind("run-id").expect("bind controller");
        server
            .register_worker_selection(7, selection(vec!["mod::test".to_string()]))
            .expect("register worker selection");
        let address = server.address().expect("address");
        let worker = thread::spawn(move || {
            let (client, selection) =
                WorkerClient::connect(address, "run-id", 7).expect("connect worker");
            assert_eq!(selection.test_paths, ["mod::test"]);
            client
                .send_event(WorkerEvent::TestStarted {
                    name: "mod::test".to_string(),
                    cache_key: TestCacheKey::function_name("mod::test"),
                })
                .expect("send event");
            client.complete().expect("complete worker");
        });

        accept_connections(&mut server, 1);
        worker.join().expect("join worker");
        server.finish().expect("finish readers");
        let event = server
            .try_recv()
            .expect("receive event")
            .expect("queued event");

        assert_eq!(event.worker_id, 7);
        assert!(matches!(
            *event.event,
            WorkerEvent::TestStarted { name, cache_key }
                if name == "mod::test"
                    && cache_key == TestCacheKey::function_name("mod::test")
        ));
    }

    #[test]
    fn rejects_wrong_run_id() {
        let mut server = ControllerServer::bind("expected").expect("bind controller");
        let address = server.address().expect("address");
        let worker = thread::spawn(move || {
            let error = WorkerClient::connect(address, "wrong", 0)
                .err()
                .expect("wrong run id should close connection");
            assert!(error.to_string().contains("before sending test paths"));
        });

        accept_connections(&mut server, 1);
        worker.join().expect("join worker");
        server.finish().expect("finish readers");
        let Err(error) = server.try_recv() else {
            panic!("wrong run id should be rejected");
        };

        assert!(error.to_string().contains("expected `expected`"));
    }

    #[test]
    fn reset_connections_are_clean_disconnects() {
        for kind in [ErrorKind::ConnectionReset, ErrorKind::ConnectionAborted] {
            let error = serde_json::Error::io(std::io::Error::from(kind));
            assert!(is_clean_disconnect(&error));
        }

        let error = serde_json::Error::io(std::io::Error::from(ErrorKind::InvalidData));
        assert!(!is_clean_disconnect(&error));
    }

    #[test]
    fn completion_closes_connection_after_terminal_event() {
        let mut server = ControllerServer::bind("run-id").expect("bind controller");
        server
            .register_worker_selection(7, selection(Vec::new()))
            .expect("register worker selection");
        let address = server.address().expect("address");
        let worker = thread::spawn(move || {
            let (client, selection) =
                WorkerClient::connect(address, "run-id", 7).expect("connect worker");
            assert!(selection.test_paths.is_empty());
            client.complete().expect("complete worker");
        });

        accept_connections(&mut server, 1);
        worker.join().expect("join worker");
        server.finish().expect("finish readers");
        let event = server
            .try_recv()
            .expect("receive event")
            .expect("queued event");

        assert_eq!(event.worker_id, 7);
        assert!(matches!(*event.event, WorkerEvent::WorkerFinished));
    }

    #[test]
    fn transfers_resume_skip_cases() {
        let mut server = ControllerServer::bind("run-id").expect("bind controller");
        server
            .register_worker_selection(
                7,
                WorkerSelection {
                    test_paths: vec!["mod::test".to_string()],
                    resume_skip: vec![TestCacheKey::function_name("mod::test[1]")],
                },
            )
            .expect("register worker selection");
        let address = server.address().expect("address");
        let worker = thread::spawn(move || {
            let (client, selection) =
                WorkerClient::connect(address, "run-id", 7).expect("connect worker");
            assert_eq!(
                selection.resume_skip,
                [TestCacheKey::function_name("mod::test[1]")]
            );
            client.complete().expect("complete worker");
        });

        accept_connections(&mut server, 1);
        worker.join().expect("join worker");
        server.finish().expect("finish readers");
    }

    #[test]
    fn truncated_terminal_event_is_a_worker_disconnect() {
        let mut server = ControllerServer::bind("run-id").expect("bind controller");
        server
            .register_worker_selection(7, selection(vec!["mod::test".to_string()]))
            .expect("register worker selection");
        let address = server.address().expect("address");
        let worker = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).expect("connect worker");
            serde_json::to_writer(
                &mut stream,
                &WireMessage::Hello {
                    run_id: "run-id".to_string(),
                    worker_id: 7,
                },
            )
            .expect("write handshake");
            stream.write_all(b"\n").expect("frame handshake");
            stream.flush().expect("flush handshake");

            let mut selection = String::new();
            BufReader::new(stream.try_clone().expect("clone stream"))
                .read_line(&mut selection)
                .expect("read selection");
            assert!(!selection.is_empty());

            stream
                .write_all(br#"{"Event":{"TestStarted""#)
                .expect("write truncated event");
            stream.flush().expect("flush truncated event");
            stream
                .shutdown(Shutdown::Write)
                .expect("close worker write stream");
            let mut drain = Vec::new();
            stream
                .read_to_end(&mut drain)
                .expect("drain controller stream");
        });

        accept_connections(&mut server, 1);
        worker.join().expect("join worker");
        server.finish().expect("finish readers");
        assert!(server.try_recv().expect("drain events").is_none());
        assert!(server.worker_started(7).expect("read worker state"));
        assert!(server.worker_disconnected(7));
    }

    #[rstest]
    fn transfers_large_worker_selection(#[values(50_000, 1_000_000)] path_count: usize) {
        // Receipt: 50,000 is the reported workload; 1,000,000 is the requested stress case.
        let mut test_paths = Vec::with_capacity(path_count);
        for index in 0..path_count {
            test_paths.push(format!("tests/test_{index}.py::test_case"));
        }

        let mut server = ControllerServer::bind("run-id").expect("bind controller");
        server
            .register_worker_selection(0, selection(test_paths))
            .expect("register worker selection");
        let address = server.address().expect("address");
        let worker = thread::spawn(move || {
            let (client, selection) =
                WorkerClient::connect(address, "run-id", 0).expect("connect worker");
            let test_paths = selection.test_paths;
            assert_eq!(test_paths.len(), path_count);
            assert_eq!(
                test_paths.first().map(String::as_str),
                Some("tests/test_0.py::test_case")
            );
            let last_path = format!("tests/test_{}.py::test_case", path_count - 1);
            assert_eq!(
                test_paths.last().map(String::as_str),
                Some(last_path.as_str())
            );
            client.complete().expect("complete worker");
        });

        accept_connections(&mut server, 1);
        worker.join().expect("join worker");
        server.finish().expect("finish readers");
    }
}
