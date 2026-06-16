//! Serialized worker output drain.
//!
//! Workers write preformatted reporter lines to per-worker output files while
//! their stdout is piped to the orchestrator. The drain below is the single
//! owner that prints those lines to stdout, which prevents parallel workers
//! from interleaving bytes on the shared terminal.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write as _};
use std::process::ChildStdout;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use camino::Utf8PathBuf;
use karva_cache::RunCache;

const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Captured stdout pipe for a single worker child process.
pub struct WorkerPipes {
    pub stdout: Option<ChildStdout>,
}

/// Drains per-worker output files and stdout pipes.
pub struct OutputDrain {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    pipe_handles: Vec<JoinHandle<()>>,
}

impl OutputDrain {
    /// Start the output drain.
    pub fn start(num_workers: usize, cache: &RunCache, pipes: Vec<WorkerPipes>) -> Self {
        let output_paths: Vec<Utf8PathBuf> =
            (0..num_workers).map(|id| cache.output_file(id)).collect();

        let (stdout_tx, stdout_rx) = mpsc::channel::<String>();

        let mut pipe_handles: Vec<JoinHandle<()>> = Vec::new();
        for pipe in pipes {
            if let Some(out) = pipe.stdout {
                let tx = stdout_tx.clone();
                pipe_handles.push(thread::spawn(move || forward_pipe(out, &tx)));
            }
        }
        drop(stdout_tx);

        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || drain_loop(&output_paths, &stop, &stdout_rx))
        };

        Self {
            stop,
            handle: Some(handle),
            pipe_handles,
        }
    }

    /// Stop polling and drain remaining whole lines.
    pub fn finish(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        for handle in self.pipe_handles.drain(..) {
            if handle.join().is_err() {
                tracing::warn!("worker stdout drain thread panicked");
            }
        }
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            tracing::warn!("worker output drain thread panicked");
        }
    }
}

impl Drop for OutputDrain {
    fn drop(&mut self) {
        if !self.stop.load(Ordering::SeqCst) {
            self.shutdown();
        }
    }
}

fn forward_pipe<R: Read>(reader: R, tx: &mpsc::Sender<String>) {
    let mut reader = BufReader::new(reader);
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => return,
            Ok(_) => {
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                }
                let line = String::from_utf8_lossy(&buf).into_owned();
                if tx.send(line).is_err() {
                    return;
                }
            }
            Err(err) => {
                tracing::warn!("failed to read worker stdout pipe: {err}");
                return;
            }
        }
    }
}

struct WorkerStream {
    path: Utf8PathBuf,
    file: Option<File>,
    offset: u64,
    partial: Vec<u8>,
}

impl WorkerStream {
    fn new(path: Utf8PathBuf) -> Self {
        Self {
            path,
            file: None,
            offset: 0,
            partial: Vec::new(),
        }
    }

    fn poll(&mut self, out: &mut Vec<String>) -> bool {
        if self.file.is_none() {
            if !self.path.exists() {
                return false;
            }
            match File::open(&self.path) {
                Ok(file) => self.file = Some(file),
                Err(err) => {
                    tracing::warn!(path = %self.path, "failed to open worker output file: {err}");
                    return false;
                }
            }
        }

        let Some(file) = self.file.as_mut() else {
            return false;
        };

        if let Err(err) = file.seek(SeekFrom::Start(self.offset)) {
            tracing::warn!(path = %self.path, "failed to seek worker output file: {err}");
            return false;
        }

        let mut buf = Vec::new();
        let n = match file.read_to_end(&mut buf) {
            Ok(n) => n,
            Err(err) => {
                tracing::warn!(path = %self.path, "failed to read worker output file: {err}");
                return false;
            }
        };
        if n == 0 {
            return false;
        }
        let Ok(read_len) = u64::try_from(n) else {
            tracing::warn!(path = %self.path, "worker output file read length does not fit u64");
            return false;
        };
        self.offset = self.offset.saturating_add(read_len);

        let mut start = 0usize;
        for (index, byte) in buf.iter().enumerate() {
            if *byte == b'\n' {
                let line_bytes = if self.partial.is_empty() {
                    &buf[start..index]
                } else {
                    self.partial.extend_from_slice(&buf[start..index]);
                    self.partial.as_slice()
                };
                out.push(String::from_utf8_lossy(line_bytes).into_owned());
                if !self.partial.is_empty() {
                    self.partial.clear();
                }
                start = index + 1;
            }
        }
        if start < buf.len() {
            self.partial.extend_from_slice(&buf[start..]);
        }

        true
    }
}

fn drain_loop(output_paths: &[Utf8PathBuf], stop: &AtomicBool, stdout_rx: &mpsc::Receiver<String>) {
    let mut streams: Vec<WorkerStream> = output_paths
        .iter()
        .cloned()
        .map(WorkerStream::new)
        .collect();

    loop {
        let mut lines: Vec<String> = Vec::new();
        let mut progressed = false;

        while let Ok(line) = stdout_rx.try_recv() {
            lines.push(line);
            progressed = true;
        }
        for stream in &mut streams {
            if stream.poll(&mut lines) {
                progressed = true;
            }
        }
        emit_lines(&lines);

        if stop.load(Ordering::SeqCst) {
            let mut final_lines: Vec<String> = Vec::new();
            while let Ok(line) = stdout_rx.try_recv() {
                final_lines.push(line);
            }
            for stream in &mut streams {
                stream.poll(&mut final_lines);
            }
            emit_lines(&final_lines);
            break;
        }

        if !progressed {
            thread::sleep(POLL_INTERVAL);
        }
    }
}

fn emit_lines(lines: &[String]) {
    if lines.is_empty() {
        return;
    }

    let mut stdout = std::io::stdout().lock();
    for line in lines {
        if let Err(err) = writeln!(stdout, "{line}") {
            tracing::warn!("failed to write worker output line: {err}");
            return;
        }
    }
}
