//! Private, typed communication between Karva's controller and workers.
//!
//! Workers connect over loopback TCP so transient coordination never touches
//! the project filesystem. JSON values form a self-delimiting stream; keeping
//! the wire format serde-based avoids platform-specific pipe implementations.

use std::collections::HashMap;
use std::io::{BufReader, BufWriter, ErrorKind, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use karva_diagnostic::AggregatedResults;
use serde::{Deserialize, Serialize};

/// One runtime state change sent from a worker to the controller.
#[derive(Serialize, Deserialize)]
pub enum WorkerEvent {
    /// On-demand snapshot used to report a worker during cancellation.
    CurrentTest {
        name: Option<String>,
        elapsed: Duration,
    },

    /// Worker exhausted its local failure budget.
    FailFast,

    /// Worker completed and produced its full result set.
    Completed(AggregatedResults),
}

#[derive(Serialize, Deserialize)]
enum WireMessage {
    Hello { run_id: String, worker_id: usize },
    Event(WorkerEvent),
}

#[derive(Serialize, Deserialize)]
enum ControllerCommand {
    ReadCurrentTest,
}

enum Incoming {
    Connected {
        worker_id: usize,
        writer: BufWriter<TcpStream>,
    },
    Event(ControllerEvent),
    Error(String),
}

#[derive(Default, Clone)]
/// In-memory test state queried by the controller only during cancellation.
pub struct WorkerState {
    current: Arc<Mutex<Option<CurrentTest>>>,
}

struct CurrentTest {
    name: String,
    started: Instant,
}

impl WorkerState {
    /// Records a test immediately before execution starts.
    pub fn start(&self, name: String) {
        if let Ok(mut current) = self.current.lock() {
            *current = Some(CurrentTest {
                name,
                started: Instant::now(),
            });
        } else {
            tracing::warn!("Karva worker state lock poisoned");
        }
    }

    /// Clears state immediately after the current test finishes.
    pub fn finish(&self) {
        if let Ok(mut current) = self.current.lock() {
            *current = None;
        } else {
            tracing::warn!("Karva worker state lock poisoned");
        }
    }

    fn event(&self) -> WorkerEvent {
        let Ok(current) = self.current.lock() else {
            tracing::warn!("Karva worker state lock poisoned");
            return WorkerEvent::CurrentTest {
                name: None,
                elapsed: Duration::ZERO,
            };
        };
        current.as_ref().map_or(
            WorkerEvent::CurrentTest {
                name: None,
                elapsed: Duration::ZERO,
            },
            |current| WorkerEvent::CurrentTest {
                name: Some(current.name.clone()),
                elapsed: current.started.elapsed(),
            },
        )
    }
}

/// Event attributed to the worker authenticated by the stream handshake.
pub struct ControllerEvent {
    /// Worker number established by the connection handshake.
    pub worker_id: usize,

    /// Runtime state change received from that worker.
    pub event: WorkerEvent,
}

/// Cloneable worker-side writer shared with execution reporters.
#[derive(Clone)]
pub struct WorkerClient {
    writer: Arc<Mutex<BufWriter<TcpStream>>>,
}

impl WorkerClient {
    /// Connects to the controller and identifies this worker before events.
    pub fn connect(address: SocketAddr, run_id: &str, worker_id: usize) -> Result<Self> {
        let stream = TcpStream::connect(address)
            .with_context(|| format!("failed to connect to Karva controller at {address}"))?;
        stream
            .set_nodelay(true)
            .context("failed to configure Karva controller connection")?;
        let client = Self {
            writer: Arc::new(Mutex::new(BufWriter::new(stream))),
        };
        client.send(&WireMessage::Hello {
            run_id: run_id.to_string(),
            worker_id,
        })?;
        Ok(client)
    }

    /// Sends one state change immediately so cancellation sees current state.
    pub fn send_event(&self, event: WorkerEvent) -> Result<()> {
        self.send(&WireMessage::Event(event))
    }

    /// Sends the terminal result payload and gracefully closes the connection.
    pub fn complete(self, results: AggregatedResults) -> Result<()> {
        self.send(&WireMessage::Event(WorkerEvent::Completed(results)))?;
        self.writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?
            .get_ref()
            .shutdown(Shutdown::Both)
            .context("failed to close Karva controller connection")
    }

    /// Starts a dormant reader that answers controller state queries.
    pub fn listen_for_commands(&self, state: WorkerState) -> Result<()> {
        let stream = self
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?
            .get_ref()
            .try_clone()
            .context("failed to clone Karva controller connection")?;
        let client = self.clone();
        thread::spawn(move || {
            let commands = serde_json::Deserializer::from_reader(BufReader::new(stream))
                .into_iter::<ControllerCommand>();
            for command in commands {
                match command {
                    Ok(ControllerCommand::ReadCurrentTest) => {
                        if let Err(error) = client.send_event(state.event()) {
                            tracing::warn!("failed to send current test to controller: {error:#}");
                            return;
                        }
                    }
                    Err(error) => {
                        tracing::warn!("failed to read Karva controller command: {error}");
                        return;
                    }
                }
            }
        });
        Ok(())
    }

    fn send(&self, message: &WireMessage) -> Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?;
        serde_json::to_writer(&mut *writer, message)
            .context("failed to serialize Karva worker event")?;
        writer
            .write_all(b"\n")
            .context("failed to frame Karva worker event")?;
        writer
            .flush()
            .context("failed to send Karva worker event")?;
        Ok(())
    }
}

/// Controller-side loopback listener and active worker stream readers.
pub struct ControllerServer {
    listener: TcpListener,
    run_id: String,
    sender: Sender<Incoming>,
    receiver: Receiver<Incoming>,
    readers: Vec<JoinHandle<()>>,
    workers: HashMap<usize, BufWriter<TcpStream>>,
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
            workers: HashMap::new(),
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
                Ok(Incoming::Connected { worker_id, writer }) => {
                    if self.workers.insert(worker_id, writer).is_some() {
                        bail!("Karva worker {worker_id} connected more than once");
                    }
                }
                Ok(Incoming::Event(event)) => return Ok(Some(event)),
                Ok(Incoming::Error(error)) => bail!(error),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(None),
            }
        }
    }

    /// Requests an in-memory state snapshot from every connected live worker.
    pub fn request_current_tests(&mut self, worker_ids: impl IntoIterator<Item = usize>) {
        for worker_id in worker_ids {
            let Some(writer) = self.workers.get_mut(&worker_id) else {
                continue;
            };
            if let Err(error) = write_command(writer, &ControllerCommand::ReadCurrentTest) {
                tracing::warn!(worker_id, "failed to query current test: {error:#}");
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
    let writer = stream
        .try_clone()
        .context("failed to clone Karva worker connection")?;
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
    writer
        .set_nodelay(true)
        .context("failed to configure Karva worker connection")?;
    if sender
        .send(Incoming::Connected {
            worker_id,
            writer: BufWriter::new(writer),
        })
        .is_err()
    {
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

fn write_command(writer: &mut BufWriter<TcpStream>, command: &ControllerCommand) -> Result<()> {
    serde_json::to_writer(&mut *writer, command)
        .context("failed to serialize Karva controller command")?;
    writer
        .write_all(b"\n")
        .context("failed to frame Karva controller command")?;
    writer
        .flush()
        .context("failed to send Karva controller command")?;
    Ok(())
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
            .send_event(WorkerEvent::FailFast)
            .expect("send event");
        drop(client);

        server.accept_pending().expect("accept worker");
        server.finish().expect("finish readers");
        let event = server
            .try_recv()
            .expect("receive event")
            .expect("queued event");

        assert_eq!(event.worker_id, 7);
        assert!(matches!(event.event, WorkerEvent::FailFast));
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
    fn completion_closes_connection_after_sending_results() {
        let mut server = ControllerServer::bind("run-id").expect("bind controller");
        let client = WorkerClient::connect(server.address().expect("address"), "run-id", 7)
            .expect("connect worker");
        client
            .complete(AggregatedResults::default())
            .expect("complete worker");

        server.accept_pending().expect("accept worker");
        server.finish().expect("finish readers");
        let event = server
            .try_recv()
            .expect("receive event")
            .expect("queued event");

        assert_eq!(event.worker_id, 7);
        assert!(matches!(event.event, WorkerEvent::Completed(_)));
    }
}
