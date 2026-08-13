/// Mutable state owned by the language-server event loop.
#[derive(Debug, Default)]
pub struct Session {
    shutdown_requested: bool,
}

impl Session {
    pub(super) fn is_shutdown_requested(&self) -> bool {
        self.shutdown_requested
    }

    pub(super) fn request_shutdown(&mut self) {
        self.shutdown_requested = true;
    }
}
