use std::sync::OnceLock;

use crossbeam_channel::Receiver;

static SHUTDOWN_CHANNEL: OnceLock<(crossbeam_channel::Sender<()>, Receiver<()>)> = OnceLock::new();

/// Returns a reference to the global shutdown receiver.
///
/// The first call initializes the channel and sets up the Ctrl+C handler.
/// Subsequent calls return the same receiver. This is safe to call multiple
/// times (idempotent).
pub fn shutdown_receiver() -> &'static Receiver<()> {
    let (_, rx) = SHUTDOWN_CHANNEL.get_or_init(|| {
        let (tx, rx) = crossbeam_channel::unbounded();

        let handler_tx = tx.clone();
        if let Err(err) = ctrlc::set_handler(move || {
            let _ = handler_tx.send(());
        }) {
            tracing::warn!("Failed to set Ctrl+C handler: {err}");
        }

        (tx, rx)
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shutdown_receiver_is_idempotent() {
        let rx1 = shutdown_receiver();
        let rx2 = shutdown_receiver();

        // Both calls should return the same receiver
        assert!(std::ptr::eq(rx1, rx2));
    }
}
