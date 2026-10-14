//! Newline-delimited IPC decoded from a reusable buffer rather than bytewise I/O.

use std::io::{self, BufRead, BufReader, Read};

use crate::protocol::WireMessage;

/// Owns incomplete frames across reads, including the final frame before EOF.
/// All protocol writers terminate compact JSON messages with a newline.
pub struct Messages<R> {
    reader: BufReader<R>,

    frame: Vec<u8>,

    /// Delivery failures follow any complete trailing message already received.
    pending_error: Option<io::Error>,
}

impl<R: Read> Messages<R> {
    pub(super) fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
            frame: Vec::new(),
            pending_error: None,
        }
    }

    /// Keeps routine buffers reusable without retaining unusually large output
    /// or selection frames for the rest of the connection's lifetime.
    fn decode_frame(&mut self) -> serde_json::Result<WireMessage> {
        let message = serde_json::from_slice(&self.frame);
        if self.frame.capacity() > self.reader.capacity() {
            self.frame = Vec::new();
        }
        message
    }
}

impl<R: Read> Iterator for Messages<R> {
    type Item = serde_json::Result<WireMessage>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(error) = self.pending_error.take() {
            return Some(Err(serde_json::Error::io(error)));
        }
        self.frame.clear();
        loop {
            match self.reader.read_until(b'\n', &mut self.frame) {
                Ok(0) if self.frame.is_empty() => return None,
                Ok(_) => {
                    if self
                        .frame
                        .iter()
                        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
                    {
                        self.frame.clear();
                        continue;
                    }
                    return Some(self.decode_frame());
                }
                Err(error) => {
                    // A reset can arrive after a complete JSON object but before
                    // its newline. Preserve that message just as the stream decoder did.
                    match self.decode_frame() {
                        Ok(message) => {
                            self.pending_error = Some(error);
                            return Some(Ok(message));
                        }
                        Err(parse_error) if !parse_error.is_eof() => return Some(Err(parse_error)),
                        Err(_) => return Some(Err(serde_json::Error::io(error))),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read};

    use rstest::rstest;

    use super::Messages;
    use crate::protocol::{WireMessage, WorkerEvent};

    struct ChunkedRead<'a> {
        bytes: &'a [u8],
        chunk: usize,
    }

    impl Read for ChunkedRead<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let length = buffer.len().min(self.chunk);
            self.bytes.read(&mut buffer[..length])
        }
    }

    #[rstest]
    fn preserves_frames_and_escaped_newlines(
        #[values(false, true)] final_newline: bool,
        #[values(1, 7)] chunk: usize,
    ) {
        let trailing = if final_newline { "\n" } else { "" };
        let data = format!(
            "\n{{\"Hello\":{{\"run_id\":\"run\\nλ\",\"worker_id\":7}}}}\n{{\"Event\":\"WorkerFinished\"}}{trailing}"
        );
        let mut messages = Messages::new(ChunkedRead {
            bytes: data.as_bytes(),
            chunk,
        });
        assert!(
            matches!(messages.next().expect("handshake").expect("decode handshake"),
            WireMessage::Hello { run_id, worker_id: 7 } if run_id == "run\nλ")
        );
        assert!(
            matches!(messages.next().expect("completion").expect("decode completion"),
            WireMessage::Event(event) if matches!(*event, WorkerEvent::WorkerFinished))
        );
        assert!(messages.next().is_none());
    }

    struct ResetAfterFrame(&'static [u8]);

    impl Read for ResetAfterFrame {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.0.is_empty() {
                return Err(io::ErrorKind::ConnectionReset.into());
            }
            self.0.read(buffer)
        }
    }

    #[test]
    fn preserves_complete_checkpoint_before_connection_reset() {
        let mut messages = Messages::new(ResetAfterFrame(br#"{"C":{"f":"mod::test"}}"#));
        assert!(
            matches!(messages.next().expect("checkpoint").expect("decode checkpoint"),
            WireMessage::TestCheckpoint { function_name: Some(name), .. } if name == "mod::test")
        );
        let error = messages.next().expect("reset").err().expect("read error");
        assert_eq!(error.io_error_kind(), Some(io::ErrorKind::ConnectionReset));
    }

    #[test]
    fn truncated_final_frame_remains_an_eof_error() {
        let mut messages = Messages::new(br#"{"C"#.as_slice());
        assert!(
            messages
                .next()
                .expect("partial frame")
                .err()
                .expect("incomplete JSON")
                .is_eof()
        );
        assert!(messages.next().is_none());
    }
}
