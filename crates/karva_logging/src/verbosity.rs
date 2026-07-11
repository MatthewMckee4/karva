use tracing_subscriber::filter::LevelFilter;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Default)]
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
    pub fn level_filter(self) -> LevelFilter {
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

    pub fn is_trace(self) -> bool {
        matches!(self, Self::Trace)
    }

    pub fn is_extra_verbose(self) -> bool {
        matches!(self, Self::ExtraVerbose)
    }

    pub fn cli_arg(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Verbose => Some("-v"),
            Self::ExtraVerbose => Some("-vv"),
            Self::Trace => Some("-vvv"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbosity_levels_map_to_tracing_filters() {
        assert_eq!(VerbosityLevel::Default.level_filter(), LevelFilter::WARN);
        assert_eq!(VerbosityLevel::Verbose.level_filter(), LevelFilter::INFO);
        assert_eq!(
            VerbosityLevel::ExtraVerbose.level_filter(),
            LevelFilter::DEBUG
        );
        assert_eq!(VerbosityLevel::Trace.level_filter(), LevelFilter::TRACE);
    }

    #[test]
    fn verbosity_levels_map_to_worker_cli_args() {
        assert_eq!(VerbosityLevel::Default.cli_arg(), None);
        assert_eq!(VerbosityLevel::Verbose.cli_arg(), Some("-v"));
        assert_eq!(VerbosityLevel::ExtraVerbose.cli_arg(), Some("-vv"));
        assert_eq!(VerbosityLevel::Trace.cli_arg(), Some("-vvv"));
    }

    #[test]
    fn predicate_methods_match_exact_levels() {
        assert!(VerbosityLevel::Default.is_default());
        assert!(!VerbosityLevel::Verbose.is_default());

        assert!(VerbosityLevel::ExtraVerbose.is_extra_verbose());
        assert!(!VerbosityLevel::Trace.is_extra_verbose());

        assert!(VerbosityLevel::Trace.is_trace());
        assert!(!VerbosityLevel::ExtraVerbose.is_trace());
    }
}
