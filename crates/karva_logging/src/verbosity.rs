use tracing_subscriber::filter::LevelFilter;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Default)]
/// Bounded logging verbosity selected by repeated `-v` flags.
pub enum VerbosityLevel {
    /// Default output level. Only shows karva events up to the [`WARN`](tracing::Level::WARN).
    #[default]
    Default,

    /// Enables verbose output. Emits karva events up to the [`INFO`](tracing::Level::INFO).
    /// Corresponds to `-v`.
    Verbose,

    /// Enables a more verbose tracing format and emits karva events up to [`DEBUG`](tracing::Level::DEBUG).
    /// Corresponds to `-vv`
    ExtraVerbose,

    /// Enables all tracing events and uses a tree-like output format. Corresponds to `-vvv`.
    Trace,
}

impl VerbosityLevel {
    /// Returns the maximum tracing level enabled for Karva targets.
    pub(super) fn level_filter(self) -> LevelFilter {
        match self {
            Self::Default => LevelFilter::WARN,
            Self::Verbose => LevelFilter::INFO,
            Self::ExtraVerbose => LevelFilter::DEBUG,
            Self::Trace => LevelFilter::TRACE,
        }
    }

    pub fn is_default(self) -> bool {
        matches!(self, Self::Default)
    }

    pub(super) fn is_trace(self) -> bool {
        matches!(self, Self::Trace)
    }

    pub(super) fn is_extra_verbose(self) -> bool {
        matches!(self, Self::ExtraVerbose)
    }

    /// Returns the CLI flags needed to reproduce this level in a worker.
    pub fn cli_arg(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Verbose => Some("-v"),
            Self::ExtraVerbose => Some("-vv"),
            Self::Trace => Some("-vvv"),
        }
    }
}
