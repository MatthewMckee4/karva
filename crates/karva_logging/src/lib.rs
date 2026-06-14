use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

use colored::Colorize;
use tracing::{Event, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

mod printer;
mod status_level;
pub mod time;
mod verbosity;

pub use printer::{Printer, Stdout};
pub use status_level::{FinalStatusLevel, StatusLevel};
pub use verbosity::VerbosityLevel;

pub fn error_chain_contains_broken_pipe<'a>(
    causes: impl IntoIterator<Item = &'a (dyn std::error::Error + 'static)>,
) -> bool {
    causes.into_iter().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|err| err.kind() == io::ErrorKind::BrokenPipe)
    })
}

pub fn write_error_chain<'a>(
    writer: &mut impl io::Write,
    causes: impl IntoIterator<Item = &'a (dyn std::error::Error + 'static)>,
) -> io::Result<()> {
    writeln!(writer, "{}", "Karva failed".red().bold())?;
    for cause in causes {
        writeln!(writer, "  {} {cause}", "Cause:".bold())?;
    }
    Ok(())
}

pub fn setup_tracing(level: VerbosityLevel) -> TracingGuard {
    use tracing_subscriber::prelude::*;

    let filter = if level.is_default() {
        EnvFilter::default().add_directive(LevelFilter::WARN.into())
    } else {
        let level_filter = level.level_filter();
        EnvFilter::default().add_directive(
            format!("karva={level_filter}")
                .parse()
                .expect("Hardcoded directive to be valid"),
        )
    };

    let ProfileSetup {
        layer: profiling_layer,
        guard,
        warning: profile_warning,
    } = setup_profile();

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(profiling_layer);

    if level.is_trace() {
        let subscriber = registry.with(
            tracing_tree::HierarchicalLayer::default()
                .with_indent_lines(true)
                .with_indent_amount(2)
                .with_bracketed_fields(true)
                .with_thread_ids(true)
                .with_targets(true)
                .with_writer(std::io::stderr)
                .with_timer(tracing_tree::time::Uptime::default()),
        );

        subscriber.init();
    } else {
        let subscriber = registry.with(
            tracing_subscriber::fmt::layer()
                .event_format(KarvaFormat {
                    display_level: true,
                    display_timestamp: level.is_extra_verbose(),
                    show_spans: false,
                })
                .with_writer(std::io::stderr),
        );

        subscriber.init();
    }

    if let Some(warning) = profile_warning {
        tracing::warn!("{warning}");
    }

    TracingGuard {
        _flame_guard: guard,
    }
}

struct ProfileSetup<S> {
    layer: Option<tracing_flame::FlameLayer<S, BufWriter<File>>>,
    guard: Option<tracing_flame::FlushGuard<BufWriter<File>>>,
    warning: Option<String>,
}

fn setup_profile<S>() -> ProfileSetup<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    if let Ok("1" | "true") = std::env::var("KARVA_LOG_PROFILE").as_deref() {
        setup_profile_file("tracing.folded")
    } else {
        ProfileSetup {
            layer: None,
            guard: None,
            warning: None,
        }
    }
}

fn setup_profile_file<S>(path: impl AsRef<Path>) -> ProfileSetup<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    let path = path.as_ref();
    match tracing_flame::FlameLayer::with_file(path) {
        Ok((layer, guard)) => ProfileSetup {
            layer: Some(layer),
            guard: Some(guard),
            warning: None,
        },
        Err(err) => ProfileSetup {
            layer: None,
            guard: None,
            warning: Some(format!(
                "failed to create tracing profile file `{}`; profiling disabled: {err}",
                path.display()
            )),
        },
    }
}

pub struct TracingGuard {
    _flame_guard: Option<tracing_flame::FlushGuard<BufWriter<File>>>,
}

struct KarvaFormat {
    display_timestamp: bool,
    display_level: bool,
    show_spans: bool,
}

impl<S, N> FormatEvent<S, N> for KarvaFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        let ansi = writer.has_ansi_escapes();

        if self.display_timestamp {
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            if ansi {
                write!(writer, "{} ", timestamp.dimmed())?;
            } else {
                write!(writer, "{timestamp} ")?;
            }
        }

        if self.display_level {
            let level = meta.level();
            if ansi {
                let formatted_level = level.to_string().bold();
                let coloured_level = match *level {
                    tracing::Level::TRACE => formatted_level.purple(),
                    tracing::Level::DEBUG => formatted_level.blue(),
                    tracing::Level::INFO => formatted_level.green(),
                    tracing::Level::WARN => formatted_level.yellow(),
                    tracing::Level::ERROR => formatted_level.red(),
                };
                write!(writer, "{coloured_level} ")?;
            } else {
                write!(writer, "{level} ")?;
            }
        }

        if self.show_spans {
            let span = event.parent();
            let mut seen = false;

            let span = span
                .and_then(|id| ctx.span(id))
                .or_else(|| ctx.lookup_current());

            let scope = span.into_iter().flat_map(|span| span.scope().from_root());

            for span in scope {
                seen = true;
                if ansi {
                    write!(writer, "{}:", span.metadata().name().bold())?;
                } else {
                    write!(writer, "{}:", span.metadata().name())?;
                }
            }

            if seen {
                writer.write_char(' ')?;
            }
        }

        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}

pub fn set_colored_override(color: Option<TerminalColor>) {
    let Some(color) = color else {
        return;
    };

    match color {
        TerminalColor::Auto => {
            colored::control::unset_override();
        }
        TerminalColor::Always => {
            colored::control::set_override(true);
        }
        TerminalColor::Never => {
            colored::control::set_override(false);
        }
    }
}

/// Control when colored output is used.
#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq, PartialOrd, Ord, Default, clap::ValueEnum)]
pub enum TerminalColor {
    /// Display colors if the output goes to an interactive terminal.
    #[default]
    Auto,

    /// Always display colors.
    Always,

    /// Never display colors.
    Never,
}

impl TerminalColor {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{error_chain_contains_broken_pipe, setup_profile_file, write_error_chain};

    struct FailingWriter {
        kind: io::ErrorKind,
    }

    impl io::Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(self.kind))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn profile_setup_disables_profiling_when_file_cannot_be_created() {
        let setup = setup_profile_file::<tracing_subscriber::Registry>(".");

        assert!(setup.layer.is_none());
        assert!(setup.guard.is_none());
        assert!(matches!(
            setup.warning.as_deref(),
            Some(warning) if warning.contains("profiling disabled")
        ));
    }

    #[test]
    fn write_error_chain_writes_header_and_causes() {
        let first = io::Error::other("first");
        let second = io::Error::other("second");
        let causes: [&dyn std::error::Error; 2] = [&first, &second];

        let mut output = Vec::new();
        write_error_chain(&mut output, causes).expect("write should succeed");

        let output = String::from_utf8(output).expect("valid UTF-8");
        assert!(output.contains("Karva failed"));
        assert!(output.contains("Cause:"));
        assert!(output.contains("first"));
        assert!(output.contains("second"));
    }

    #[test]
    fn write_error_chain_propagates_write_failures() {
        let cause = io::Error::other("cause");
        let causes: [&dyn std::error::Error; 1] = [&cause];
        let mut writer = FailingWriter {
            kind: io::ErrorKind::PermissionDenied,
        };

        let err = write_error_chain(&mut writer, causes).expect_err("write should fail");

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn error_chain_contains_broken_pipe_detects_io_cause() {
        let cause = io::Error::from(io::ErrorKind::BrokenPipe);
        let causes: [&dyn std::error::Error; 1] = [&cause];

        assert!(error_chain_contains_broken_pipe(causes));
    }
}
