//! Cross-platform local transport for controller-worker IPC.
//!
//! Endpoints cross the process boundary, listeners own controller-side socket
//! lifetime, and streams hide platform-specific I/O from the wire protocol.

mod endpoint;
mod listener;
mod stream;

#[cfg(test)]
mod tests;

pub use endpoint::ControllerEndpoint;
pub use listener::ControllerListener;
pub use stream::ControllerStream;
