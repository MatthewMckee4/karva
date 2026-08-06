//! Private, typed communication between Karva's controller and workers.
//!
//! Workers connect over loopback TCP so transient coordination never touches
//! the project filesystem. JSON values form a self-delimiting stream; keeping
//! the wire format serde-based avoids platform-specific pipe implementations.

use std::collections::HashSet;
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
    TestStarted { name: String },

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
    Event(Box<WorkerEvent>),
}

enum Incoming {
    Connected { worker_id: usize },
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
    latest: Option<String>,
    sent: Option<String>,
}

impl WorkerClient {
    /// Connects to the controller and identifies this worker before events.
    pub fn connect(address: SocketAddr, run_id: &str, worker_id: usize) -> Result<Self> {
        let stream = TcpStream::connect(address)
            .with_context(|| format!("failed to connect to Karva controller at {address}"))?;
        stream
            .set_nodelay(true)
            .context("failed to configure Karva controller connection")?;
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
        Ok(client)
    }

    /// Queues one state change, flushing failures immediately for global fail-fast.
    pub fn send_event(&self, event: WorkerEvent) -> Result<()> {
        let mut current_test = self
            .connection
            .current_test
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva worker current-test lock poisoned"))?;
        if let WorkerEvent::TestStarted { name } = &event {
            current_test.latest = Some(name.clone());
            return Ok(());
        }
        if matches!(event, WorkerEvent::TestFinished { .. }) {
            current_test.latest = None;
            current_test.sent = None;
        }
        let flush = matches!(
            &event,
            WorkerEvent::TestFinished { result, .. } if result.outcome().is_non_success()
        );
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
            if let Some(name) = current_test.latest.as_ref() {
                self.write(
                    &WireMessage::Event(Box::new(WorkerEvent::TestStarted { name: name.clone() })),
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

// Receipt: synchronous per-event flushing made the 16,807-case parametrized
// benchmark 40.5% slower. The controller observes workers every 10 ms, so a
// shorter flush interval cannot improve its response time.
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
        if let Some(name) = current_test.latest.as_ref() {
            serde_json::to_writer(
                &mut *writer,
                &WireMessage::Event(Box::new(WorkerEvent::TestStarted { name: name.clone() })),
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
        })
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
                    self.readers.push(thread::spawn(move || {
                        if let Err(error) = read_worker(stream, &run_id, &sender) {
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
                Ok(Incoming::Event(event)) => return Ok(Some(event)),
                Ok(Incoming::Error(error)) => bail!(error),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(None),
            }
        }
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

fn read_worker(stream: TcpStream, expected_run_id: &str, sender: &Sender<Incoming>) -> Result<()> {
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
    if sender.send(Incoming::Connected { worker_id }).is_err() {
        return Ok(());
    }

    for message in messages {
        let message = match message {
            Ok(message) => message,
            Err(error) if is_clean_disconnect(&error) => return Ok(()),
            Err(error) => return Err(error).context("failed to read Karva worker event"),
        };
        let event = match message {
            WireMessage::Event(event) => event,
            WireMessage::Hello { .. } => bail!("Karva worker sent more than one handshake"),
        };
        if sender
            .send(Incoming::Event(ControllerEvent { worker_id, event }))
            .is_err()
        {
            return Ok(());
        }
    }
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
    use super::*;

    #[test]
    fn streams_attributed_worker_events() {
        let mut server = ControllerServer::bind("run-id").expect("bind controller");
        let client = WorkerClient::connect(server.address().expect("address"), "run-id", 7)
            .expect("connect worker");
        client
            .send_event(WorkerEvent::TestStarted {
                name: "mod::test".to_string(),
            })
            .expect("send event");
        client.complete().expect("complete worker");

        server.accept_pending().expect("accept worker");
        server.finish().expect("finish readers");
        let event = server
            .try_recv()
            .expect("receive event")
            .expect("queued event");

        assert_eq!(event.worker_id, 7);
        assert!(matches!(
            *event.event,
            WorkerEvent::TestStarted { name } if name == "mod::test"
        ));
    }

    #[test]
    fn rejects_wrong_run_id() {
        let mut server = ControllerServer::bind("expected").expect("bind controller");
        let client = WorkerClient::connect(server.address().expect("address"), "wrong", 0)
            .expect("connect worker");
        drop(client);

        server.accept_pending().expect("accept worker");
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
        let client = WorkerClient::connect(server.address().expect("address"), "run-id", 7)
            .expect("connect worker");
        client.complete().expect("complete worker");

        server.accept_pending().expect("accept worker");
        server.finish().expect("finish readers");
        let event = server
            .try_recv()
            .expect("receive event")
            .expect("queued event");

        assert_eq!(event.worker_id, 7);
        assert!(matches!(*event.event, WorkerEvent::WorkerFinished));
    }
}
