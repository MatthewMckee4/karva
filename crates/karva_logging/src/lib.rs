//! Logging initialization, terminal output, and display-level policy.

use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

use colored::Colorize;
use karva_static::{EnvVars, parse_boolish_env_var};
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

/// Installs process-global tracing and returns resources that must outlive the run.
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

/// Optional tracing profile layer plus resources and recoverable setup warning.
struct ProfileSetup<S> {
    /// Layer installed when profile file creation succeeds.
    layer: Option<tracing_flame::FlameLayer<S, BufWriter<File>>>,

    /// Guard that flushes buffered profile data.
    guard: Option<tracing_flame::FlushGuard<BufWriter<File>>>,

    /// Setup failure reported after normal tracing starts.
    warning: Option<String>,
}

fn setup_profile<S>() -> ProfileSetup<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    match parse_boolish_env_var(EnvVars::KARVA_LOG_PROFILE) {
        Ok(Some(true)) => setup_profile_file("tracing.folded"),
        Ok(Some(false) | None) => ProfileSetup {
            layer: None,
            guard: None,
            warning: None,
        },
        Err(err) => ProfileSetup {
            layer: None,
            guard: None,
            warning: Some(format!("{err}; profiling disabled")),
        },
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

/// Keeps optional tracing-profile output alive and flushes it on drop.
pub struct TracingGuard {
    _flame_guard: Option<tracing_flame::FlushGuard<BufWriter<File>>>,
}

/// Compact event formatter used below trace verbosity.
struct KarvaFormat {
    /// Whether each event starts with local wall-clock time.
    display_timestamp: bool,

    /// Whether each event includes its tracing level.
    display_level: bool,

    /// Whether events include their enclosing span path.
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

/// Applies user color policy to the global `colored` output controller.
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
    /// Returns the canonical configuration spelling.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}
