use std::{fmt, fs::File, io::BufWriter};

use colored::Colorize;
use karva_project::VerbosityLevel;
use tracing::{Event, Subscriber};
use tracing_subscriber::{
    EnvFilter,
    filter::LevelFilter,
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
    registry::LookupSpan,
};

#[derive(clap::Args, Debug, Clone, Default)]
#[command(about = None, long_about = None)]
pub struct Verbosity {
    #[arg(
        long,
        short = 'v',
        help = "Use verbose output (or `-vv` and `-vvv` for more verbose output)",
        action = clap::ArgAction::Count,
        global = true,
        overrides_with = "quiet",
    )]
    verbose: u8,

    #[arg(
        long,
        short,
        help = "Use quiet output (or `-qq` for silent output)",
        action = clap::ArgAction::Count,
        global = true,
        overrides_with = "verbose",
    )]
    quiet: u8,
}

impl Verbosity {
    /// Returns the verbosity level based on the number of `-v` and `-q` flags.
    ///
    /// Returns `None` if the user did not specify any verbosity flags.
    pub(crate) const fn level(&self) -> VerbosityLevel {
        // `--quiet` and `--verbose` are mutually exclusive in Clap, so we can just check one first.
        match self.quiet {
            0 => {}
            _ => return VerbosityLevel::Quiet,
        }

        match self.verbose {
            0 => VerbosityLevel::Default,
            1 => VerbosityLevel::Verbose,
            2 => VerbosityLevel::ExtraVerbose,
            _ => VerbosityLevel::Trace,
        }
    }
}

impl PartialEq<u8> for Verbosity {
    fn eq(&self, other: &u8) -> bool {
        self.verbose == *other
    }
}

impl PartialOrd<u8> for Verbosity {
    fn partial_cmp(&self, other: &u8) -> Option<std::cmp::Ordering> {
        Some(self.verbose.cmp(other))
    }
}

pub fn setup_tracing(level: VerbosityLevel) -> TracingGuard {
    use tracing_subscriber::prelude::*;

    let filter = match level {
        VerbosityLevel::Default => EnvFilter::default().add_directive(LevelFilter::WARN.into()),
        level => {
            let level_filter = level.level_filter();

            let filter = EnvFilter::default().add_directive(
                format!("karva={level_filter}")
                    .parse()
                    .expect("Hardcoded directive to be valid"),
            );

            filter.add_directive(
                format!("karva={level_filter}")
                    .parse()
                    .expect("Hardcoded directive to be valid"),
            )
        }
    };

    let (profiling_layer, guard) = setup_profile();

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

    TracingGuard {
        _flame_guard: guard,
    }
}

#[allow(clippy::type_complexity)]
fn setup_profile<S>() -> (
    Option<tracing_flame::FlameLayer<S, BufWriter<File>>>,
    Option<tracing_flame::FlushGuard<BufWriter<File>>>,
)
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    if let Ok("1" | "true") = std::env::var("KARVA_LOG_PROFILE").as_deref() {
        let (layer, guard) = tracing_flame::FlameLayer::with_file("tracing.folded")
            .expect("Flame layer to be created");
        (Some(layer), Some(guard))
    } else {
        (None, None)
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
