//! Shared rendering for worker crash and interruption diagnostics.

use std::io::Write;
use std::time::Duration;

use colored::Colorize;
use karva_logging::Printer;

/// Width shared with result labels in `karva_diagnostic::reporter`.
pub(super) const LABEL_COLUMN_WIDTH: usize = 12;

/// Formats a worker-reported qualified test name with Karva's result colors.
pub(super) fn format_in_flight_test(name: &str) -> String {
    if let Some((module, rest)) = name.split_once("::") {
        format!("{}::{}", module.cyan(), rest.blue().bold())
    } else {
        name.blue().bold().to_string()
    }
}

/// Prints one crash line unless status output is disabled.
pub(super) fn print_crashed_test(printer: Printer, name: &str, duration: Duration) {
    if printer.status_level() == karva_logging::StatusLevel::None {
        return;
    }
    let label = "CRASH".red().bold();
    let padding = " ".repeat(LABEL_COLUMN_WIDTH.saturating_sub("CRASH".len()));
    let duration = karva_logging::time::format_duration_bracketed(duration);
    let mut stdout = printer.stream_for_test_result().lock();
    if let Err(error) = writeln!(
        stdout,
        "{padding}{label} {duration} {}",
        format_in_flight_test(name)
    ) {
        tracing::warn!(target: "karva_runner::orchestration", "failed to write crashed test line: {error}");
    }
}

/// Describes an exit status in user-facing diagnostic text.
pub(super) fn termination_description(status: std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            let name = match signal {
                libc::SIGABRT => "SIGABRT",
                libc::SIGBUS => "SIGBUS",
                libc::SIGILL => "SIGILL",
                libc::SIGSEGV => "SIGSEGV",
                libc::SIGTRAP => "SIGTRAP",
                _ => "signal",
            };
            return format!("{name} ({signal})");
        }
    }
    status.code().map_or_else(
        || "an unknown status".to_string(),
        |code| format!("exit code {code}"),
    )
}
