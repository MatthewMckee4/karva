//! Stderr-only tracing and the process-wide panic boundary for the stdio server.

use std::any::Any;
use std::backtrace::Backtrace;
use std::panic::{AssertUnwindSafe, PanicHookInfo, catch_unwind};
use std::sync::Mutex;

use anyhow::Context;
use crossbeam_channel::Sender;
use lsp_server::{Message, Notification};
use lsp_types::{MessageType, Notification as _, ShowMessageNotification, ShowMessageParams};
use tracing_subscriber::{EnvFilter, filter::LevelFilter};

// Hook replacement is process-global. Serialize scoped runs and restore the
// caller's hook only after the main thread and joined workers stop unwinding.
static PANIC_HOOK_LOCK: Mutex<()> = Mutex::new(());
type PanicHook = Box<dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static>;

/// Installs worker-visible logging without mixing logs into the LSP stdout stream.
/// `RUST_LOG` selects the filter; warnings and errors are enabled by default.
pub fn initialize_logging() -> anyhow::Result<()> {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .from_env()
        .context("invalid language-server RUST_LOG filter")?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize language-server logging: {error}"))
}

struct PanicHookGuard {
    previous: Option<PanicHook>,
}

impl PanicHookGuard {
    fn install(sender: Sender<Message>) -> Self {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            tracing::error!(panic = %info, backtrace = %Backtrace::force_capture(), "Karva language server panicked");
            let params = ShowMessageParams::new(
                MessageType::Error,
                format!(
                    "Karva language server panicked: {info}. See the language-server stderr log for a backtrace."
                ),
            );
            if let Ok(params) = serde_json::to_value(params) {
                let _ = sender.send(Message::Notification(Notification::new(
                    ShowMessageNotification::METHOD.as_str().to_owned(),
                    params,
                )));
            }
        }));
        Self {
            previous: Some(previous),
        }
    }
}

impl Drop for PanicHookGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::panic::set_hook(previous);
        }
    }
}

/// Reports panics to the editor, returns a contextual error, and restores the hook.
/// Recoverable request panics still pass through their internal-error response boundary.
pub fn with_panic_reporting(
    sender: Sender<Message>,
    run: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let _lock = PANIC_HOOK_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("language-server panic hook lock poisoned"))?;
    let guard = PanicHookGuard::install(sender);
    let result = catch_unwind(AssertUnwindSafe(run));
    drop(guard);
    match result {
        Ok(result) => {
            if let Err(error) = &result {
                tracing::error!(error = %format!("{error:#}"), "Karva language server failed");
            }
            result
        }
        Err(payload) => Err(panic_error(payload.as_ref())),
    }
}

pub fn panic_error(payload: &(dyn Any + Send)) -> anyhow::Error {
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("non-string panic payload");
    anyhow::anyhow!("unexpected Karva language-server panic: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[rstest::rstest]
    fn restores_hook_after_shutdown_initialization_failure_and_panic(
        #[values(0, 1, 2)] outcome: u8,
    ) {
        // Process isolation also makes hook restoration safe under parallel cargo test.
        if std::env::var_os("KARVA_PANIC_BOUNDARY_TEST_CHILD").is_none() {
            let thread = std::thread::current();
            let output =
                std::process::Command::new(std::env::current_exe().expect("test executable"))
                    .args(["--exact", thread.name().expect("test name"), "--nocapture"])
                    .env("KARVA_PANIC_BOUNDARY_TEST_CHILD", "1")
                    .output()
                    .expect("isolated panic boundary test");
            assert!(
                output.status.success(),
                "status: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            if outcome == 2 {
                assert!(
                    String::from_utf8_lossy(&output.stderr)
                        .contains("Karva language server panicked")
                );
            }
            return;
        }
        initialize_logging().expect("stderr logging");
        let previous = std::panic::take_hook();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        std::panic::set_hook(Box::new(move |_| {
            observed.fetch_add(1, Ordering::Relaxed);
        }));
        let (sender, receiver) = crossbeam_channel::unbounded();
        let result = with_panic_reporting(sender, || match outcome {
            0 => Ok(()),
            1 => Err(anyhow::anyhow!(
                "failed to initialize language server: invalid options"
            )),
            _ => std::panic::panic_any("main-loop failure"),
        });
        let restored = catch_unwind(|| std::panic::panic_any("restored hook"));
        std::panic::set_hook(previous);
        assert!(restored.is_err());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        if outcome == 2 {
            assert!(
                result
                    .expect_err("panic returned error")
                    .to_string()
                    .contains("main-loop failure")
            );
            let message = receiver.try_recv().expect("editor panic report");
            assert!(matches!(message, Message::Notification(_)));
            let Message::Notification(notification) = message else {
                return;
            };
            assert_eq!(
                notification.method,
                ShowMessageNotification::METHOD.as_str()
            );
            let params: ShowMessageParams =
                serde_json::from_value(notification.params).expect("show message");
            assert_eq!(params.kind, MessageType::Error);
            assert!(params.message.contains("main-loop failure"));
        } else {
            assert_eq!(result.is_ok(), outcome == 0);
            assert!(receiver.try_recv().is_err());
        }
    }
}
