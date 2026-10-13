//! Controller-owned capture buffers survive abrupt worker exit.
//!
//! Private stream markers delimit attempts; a synchronous IPC read waits for
//! both end markers, never for inherited pipe EOF. Only one capture is pending
//! per worker because the worker collects it before starting another attempt.

use std::collections::VecDeque;
use std::io;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use karva_diagnostic::CapturedTestOutput;

/// Beginning and end of a stream, allocated only as output arrives.
#[derive(Clone, Debug)]
struct OutputBuffer {
    limit: NonZeroUsize,

    /// Bytes observed in this window, including bytes no longer retained.
    total: u64,

    head: Vec<u8>,
    tail: VecDeque<u8>,
}

impl OutputBuffer {
    fn new(limit: NonZeroUsize) -> Self {
        Self {
            limit,
            total: 0,
            head: Vec::new(),
            tail: VecDeque::new(),
        }
    }

    fn push(&mut self, mut bytes: &[u8]) {
        self.total = self.total.saturating_add(bytes.len() as u64);
        let head_limit = self.limit.get().div_ceil(2);
        let first = bytes.len().min(head_limit - self.head.len());
        self.head.extend_from_slice(&bytes[..first]);
        bytes = &bytes[first..];
        let tail_limit = self.limit.get() - head_limit;
        if bytes.len() >= tail_limit {
            self.tail.clear();
            self.tail.extend(&bytes[bytes.len() - tail_limit..]);
        } else {
            let remove = (self.tail.len() + bytes.len()).saturating_sub(tail_limit);
            self.tail.drain(..remove);
            self.tail.extend(bytes);
        }
    }

    fn text(&self) -> String {
        let tail = self.tail.iter().copied().collect::<Vec<_>>();
        let omitted = self
            .total
            .saturating_sub((self.head.len() + tail.len()) as u64);
        if omitted == 0 {
            let mut bytes = self.head.clone();
            bytes.extend_from_slice(&tail);
            return String::from_utf8_lossy(&bytes).into_owned();
        }
        format!(
            "{}\n... captured output truncated: {omitted} of {} bytes omitted (max-output-bytes={}) ...\n{}",
            String::from_utf8_lossy(&self.head),
            self.total,
            self.limit,
            String::from_utf8_lossy(&tail),
        )
    }
}

/// Identifies one of the worker's inherited output streams.
#[derive(Clone, Copy)]
pub enum OutputStream {
    /// Standard output.
    Stdout,

    /// Standard error.
    Stderr,
}

/// Tracks one pipe independently; undecided bytes are at most one frame prefix.
#[derive(Debug)]
struct StreamCapture {
    inside: bool,
    closed: bool,
    pending: Vec<u8>,
    output: OutputBuffer,

    /// End marker received; the next begin marker requires collection first.
    complete: bool,
}

impl StreamCapture {
    fn new(limit: NonZeroUsize) -> Self {
        Self {
            inside: false,
            closed: false,
            pending: Vec::new(),
            output: OutputBuffer::new(limit),
            complete: false,
        }
    }

    fn consume(&mut self, bytes: &[u8], outside: &mut Vec<u8>) {
        if self.inside {
            self.output.push(bytes);
        } else {
            outside.extend_from_slice(bytes);
        }
    }

    fn push(&mut self, marker: &[u8; 16], bytes: &[u8]) -> io::Result<Vec<u8>> {
        let mut nonce = vec![0];
        nonce.extend_from_slice(marker);
        let marker = nonce.as_slice();
        self.pending.extend_from_slice(bytes);
        let mut outside = Vec::new();
        while let Some(start) = self
            .pending
            .windows(marker.len())
            .position(|part| part == marker)
        {
            if start + marker.len() == self.pending.len() {
                break;
            }
            let bytes = self.pending.drain(..start).collect::<Vec<_>>();
            self.consume(&bytes, &mut outside);
            let tag = self.pending[marker.len()];
            self.pending.drain(..=marker.len());
            match tag {
                b'B' if !self.inside && !self.complete => {
                    self.output = OutputBuffer::new(self.output.limit);
                    self.inside = true;
                }
                b'E' if self.inside => {
                    self.complete = true;
                    self.inside = false;
                }
                b'F' if !self.inside => {
                    self.closed = true;
                    self.pending.clear();
                    return Ok(outside);
                }
                _ => return Err(io::Error::other("invalid Karva output capture boundary")),
            }
        }
        let retained = (1..=marker.len().min(self.pending.len()))
            .rev()
            .find(|&length| self.pending.ends_with(&marker[..length]))
            .unwrap_or(0);
        let ready = self.pending.len() - retained;
        let bytes = self.pending.drain(..ready).collect::<Vec<_>>();
        self.consume(&bytes, &mut outside);
        Ok(outside)
    }

    fn snapshot(&self) -> String {
        if !self.inside {
            return String::new();
        }
        let mut output = self.output.clone();
        output.push(&self.pending);
        output.text()
    }
}

/// Both streams share a collection barrier while pipe readers advance independently.
#[derive(Debug)]
struct State {
    stdout: StreamCapture,
    stderr: StreamCapture,
}

/// Bounded controller storage for one worker's current output window.
#[derive(Debug)]
pub struct OutputCaptureSink {
    marker: [u8; 16],
    state: Mutex<State>,
    ready: Condvar,
}

impl OutputCaptureSink {
    /// Creates empty, lazily allocated per-stream buffers.
    pub fn new(marker: [u8; 16], limit: NonZeroUsize) -> Self {
        Self {
            marker,
            state: Mutex::new(State {
                stdout: StreamCapture::new(limit),
                stderr: StreamCapture::new(limit),
            }),
            ready: Condvar::new(),
        }
    }

    /// Consumes a pipe chunk and returns only bytes outside test capture.
    pub fn push(&self, stream: OutputStream, bytes: &[u8]) -> io::Result<Vec<u8>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("output capture lock poisoned"))?;
        let capture = match stream {
            OutputStream::Stdout => &mut state.stdout,
            OutputStream::Stderr => &mut state.stderr,
        };
        let result = capture.push(&self.marker, bytes);
        self.ready.notify_all();
        result
    }

    /// Whether the worker sealed a framework stream independently of descendant EOF.
    pub fn is_closed(&self, stream: OutputStream) -> bool {
        self.state.lock().is_ok_and(|state| match stream {
            OutputStream::Stdout => state.stdout.closed,
            OutputStream::Stderr => state.stderr.closed,
        })
    }

    /// Marks EOF or forced disconnect and returns undecided bytes outside capture.
    pub fn close(&self, stream: OutputStream) -> io::Result<Vec<u8>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("output capture lock poisoned"))?;
        let capture = match stream {
            OutputStream::Stdout => &mut state.stdout,
            OutputStream::Stderr => &mut state.stderr,
        };
        capture.closed = true;
        let pending = std::mem::take(&mut capture.pending);
        let mut outside = Vec::new();
        capture.consume(&pending, &mut outside);
        self.ready.notify_all();
        Ok(outside)
    }

    /// Collects one completed attempt after both ordered end markers arrive.
    pub fn take(
        &self,
        interrupted: &AtomicBool,
        poll_interval: Duration,
    ) -> io::Result<CapturedTestOutput> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("output capture lock poisoned"))?;
        loop {
            if state.stdout.complete && state.stderr.complete {
                for (name, stream) in [("stdout", &state.stdout), ("stderr", &state.stderr)] {
                    if stream.output.total > stream.output.limit.get() as u64 {
                        tracing::warn!(
                            "captured {name} exceeded max-output-bytes: requested={} bytes, allowed={} bytes; retained beginning and end",
                            stream.output.total,
                            stream.output.limit
                        );
                    }
                }
                let stdout = state.stdout.output.text();
                let stderr = state.stderr.output.text();
                let State {
                    stdout: out,
                    stderr: err,
                } = &mut *state;
                for stream in [out, err] {
                    stream.complete = false;
                    stream.output.head.clear();
                    stream.output.tail.clear();
                }
                return Ok(CapturedTestOutput::new(stdout, stderr));
            }
            if state.stdout.closed || state.stderr.closed || interrupted.load(Ordering::Acquire) {
                return Err(io::Error::other(
                    "worker stream closed before output capture completed",
                ));
            }
            state = self
                .ready
                .wait_timeout(state, poll_interval)
                .map_err(|_| io::Error::other("output capture lock poisoned"))?
                .0;
        }
    }

    /// Returns retained output from an attempt interrupted before its end markers.
    pub fn interrupted(&self) -> Option<CapturedTestOutput> {
        let state = self.state.lock().ok()?;
        let output = CapturedTestOutput::new(state.stdout.snapshot(), state.stderr.snapshot());
        (!output.is_empty()).then_some(output)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use rstest::rstest;

    use super::{OutputBuffer, OutputCaptureSink, OutputStream};

    fn frame(marker: &[u8; 16], tag: u8) -> Vec<u8> {
        let mut bytes = vec![0];
        bytes.extend_from_slice(marker);
        bytes.push(tag);
        bytes
    }

    #[rstest]
    fn splits_capture_markers_at_every_relevant_boundary(
        #[values(1, 2, 16, 17, 18, 64)] chunk_size: usize,
    ) {
        let marker = *b"0123456789ABCDEF";
        let sink = OutputCaptureSink::new(marker, NonZeroUsize::new(4).expect("positive budget"));
        let mut bytes = b"before\n".to_vec();
        bytes.extend(frame(&marker, b'B'));
        bytes.extend(b"abcdef");
        bytes.extend(frame(&marker, b'E'));
        bytes.extend(b"after\n");
        let mut outside = Vec::new();
        for chunk in bytes.chunks(chunk_size) {
            outside.extend(
                sink.push(OutputStream::Stdout, chunk)
                    .expect("consume stdout"),
            );
        }
        let mut stderr = frame(&marker, b'B');
        stderr.extend(frame(&marker, b'E'));
        sink.push(OutputStream::Stderr, &stderr)
            .expect("consume stderr");
        assert_eq!(outside, b"before\nafter\n");
        let output = sink
            .take(&AtomicBool::new(false), Duration::ZERO)
            .expect("completed capture");
        assert_eq!(
            output.stdout(),
            "ab\n... captured output truncated: 2 of 6 bytes omitted (max-output-bytes=4) ...\nef"
        );
        assert_eq!(output.stderr(), "");
    }

    #[test]
    fn interrupted_capture_keeps_bytes_without_a_newline() {
        let marker = *b"0123456789ABCDEF";
        let sink = OutputCaptureSink::new(marker, NonZeroUsize::new(64).expect("positive budget"));
        let mut bytes = frame(&marker, b'B');
        bytes.extend(b"unterminated output");
        sink.push(OutputStream::Stderr, &bytes)
            .expect("consume bytes");
        sink.close(OutputStream::Stderr).expect("close capture");
        assert_eq!(
            sink.interrupted().expect("interrupted output").stderr(),
            "unterminated output"
        );
    }

    #[test]
    fn framework_seal_does_not_wait_for_inherited_writer_eof() {
        let marker = *b"0123456789ABCDEF";
        let sink = OutputCaptureSink::new(marker, NonZeroUsize::MIN);
        let mut bytes = b"status\n".to_vec();
        bytes.extend(frame(&marker, b'F'));
        bytes.extend(b"late descendant output");
        assert_eq!(
            sink.push(OutputStream::Stdout, &bytes)
                .expect("seal stream"),
            b"status\n"
        );
        assert!(sink.is_closed(OutputStream::Stdout));
    }

    #[test]
    fn capture_wait_stops_when_connection_is_interrupted() {
        let sink = OutputCaptureSink::new([0; 16], NonZeroUsize::MIN);
        assert!(sink.take(&AtomicBool::new(true), Duration::ZERO).is_err());
    }

    #[test]
    fn decodes_utf8_across_the_head_tail_boundary() {
        let mut buffer = OutputBuffer::new(NonZeroUsize::new(4).expect("positive budget"));
        buffer.push("a€".as_bytes());
        assert_eq!(buffer.text(), "a€");
    }

    #[test]
    fn one_byte_budget_retains_only_the_first_byte() {
        let mut buffer = OutputBuffer::new(NonZeroUsize::MIN);
        buffer.push(b"abc");
        assert_eq!(buffer.head, b"a");
        assert!(buffer.tail.is_empty());
    }
}
