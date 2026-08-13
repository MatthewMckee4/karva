//! Controller-side listener, authentication, and worker event intake.
//!
//! Each accepted connection gets a reader thread. The reader authenticates the
//! worker, sends its selection, and forwards complete events through a channel;
//! the controller remains responsible for ordering and interpreting them.

use std::collections::{HashMap, HashSet};
use std::io::{BufReader, BufWriter, ErrorKind, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};

use crate::protocol::{WireMessage, WorkerEvent, WorkerSelection};

/// Reader-thread notification consumed by the controller event loop.
enum Incoming {
    /// Worker completed authentication.
    Connected { worker_id: usize },

    /// Worker event stream reached a clean end.
    Disconnected { worker_id: usize },

    /// Authenticated runtime event ready for serial dispatch.
    Event(ControllerEvent),

    /// Reader failure that must terminate the controller run.
    Error(String),
}

/// Event attributed to the worker authenticated by the stream handshake.
pub struct ControllerEvent {
    /// Worker number established by the connection handshake.
    pub worker_id: usize,

    /// Runtime state change received from that worker.
    pub event: Box<WorkerEvent>,
}

/// Controller-side loopback listener and active worker stream readers.
pub struct ControllerServer {
    /// Non-blocking listener used to accept worker connections.
    listener: TcpListener,

    /// Run identifier required in every worker handshake.
    run_id: String,

    /// Reader-to-controller notifications.
    sender: Sender<Incoming>,

    /// Controller-side queue of reader notifications.
    receiver: Receiver<Incoming>,

    /// Reader threads and control streams retained for shutdown and progress checks.
    readers: Vec<ControllerReader>,

    /// Workers that completed authentication.
    workers: HashSet<usize>,

    /// Workers whose event streams reached EOF.
    disconnected_workers: HashSet<usize>,

    /// Selections waiting for the corresponding authenticated worker connection.
    worker_selections: Arc<Mutex<HashMap<usize, WorkerSelection>>>,
}

/// Reader thread and shared progress state for one worker connection.
struct ControllerReader {
    /// Thread consuming and authenticating the worker stream.
    handle: JoinHandle<()>,

    /// Controller clone used to interrupt an escaped descendant's socket.
    stream: TcpStream,

    /// Worker id populated after the handshake succeeds.
    worker_id: Arc<AtomicUsize>,

    /// Number of complete event frames consumed by the reader.
    event_count: Arc<AtomicUsize>,
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
                    let control_stream = stream
                        .try_clone()
                        .context("failed to control Karva worker connection")?;
                    let shutdown_stream = stream
                        .try_clone()
                        .context("failed to close Karva worker connection")?;
                    let worker_id = Arc::new(AtomicUsize::new(usize::MAX));
                    let reader_worker_id = Arc::clone(&worker_id);
                    let event_count = Arc::new(AtomicUsize::new(0));
                    let reader_event_count = Arc::clone(&event_count);
                    let handle = thread::spawn(move || {
                        if let Err(error) = read_worker(
                            stream,
                            &run_id,
                            &worker_selections,
                            &sender,
                            &reader_worker_id,
                            &reader_event_count,
                        ) {
                            sender.send(Incoming::Error(format!("{error:#}"))).ok();
                        }
                        shutdown_stream.shutdown(Shutdown::Write).ok();
                        shutdown_stream.shutdown(Shutdown::Read).ok();
                    });
                    self.readers.push(ControllerReader {
                        handle,
                        stream: control_stream,
                        worker_id,
                        event_count,
                    });
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

    /// Number of complete event frames read from one authenticated worker.
    pub fn worker_event_count(&self, worker_id: usize) -> usize {
        self.readers
            .iter()
            .find(|reader| reader.worker_id.load(Ordering::Acquire) == worker_id)
            .map_or(0, |reader| reader.event_count.load(Ordering::Acquire))
    }

    /// Closes one authenticated worker stream retained by an escaped descendant.
    pub fn disconnect_worker(&self, worker_id: usize) -> Result<()> {
        let Some(reader) = self
            .readers
            .iter()
            .find(|reader| reader.worker_id.load(Ordering::Acquire) == worker_id)
        else {
            return Ok(());
        };
        reader
            .stream
            .shutdown(Shutdown::Write)
            .or_else(|error| {
                if error.kind() == ErrorKind::NotConnected {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .context("failed to close Karva worker connection")?;
        reader
            .stream
            .shutdown(Shutdown::Read)
            .or_else(|error| {
                if error.kind() == ErrorKind::NotConnected {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .context("failed to close Karva worker connection")
    }

    /// Joins every accepted reader after worker processes have exited.
    pub fn finish(&mut self) -> Result<()> {
        self.accept_pending()?;
        for reader in self.readers.drain(..) {
            if reader.handle.join().is_err() {
                bail!("Karva worker connection reader panicked");
            }
        }
        Ok(())
    }
}

/// Authenticates one stream, transfers its selection, then decodes its events.
fn read_worker(
    stream: TcpStream,
    expected_run_id: &str,
    worker_selections: &Mutex<HashMap<usize, WorkerSelection>>,
    sender: &Sender<Incoming>,
    reader_worker_id: &AtomicUsize,
    reader_event_count: &AtomicUsize,
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
    reader_worker_id.store(worker_id, Ordering::Release);
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
        reader_event_count.fetch_add(1, Ordering::Release);
    }
    sender.send(Incoming::Disconnected { worker_id }).ok();
    Ok(())
}

/// Distinguishes normal socket teardown from a malformed event frame.
fn is_clean_disconnect(error: &serde_json::Error) -> bool {
    matches!(
        error.io_error_kind(),
        Some(ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted)
    )
}

#[cfg(test)]
mod tests;
