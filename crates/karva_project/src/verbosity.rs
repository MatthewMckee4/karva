use tracing_subscriber::filter::LevelFilter;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Default)]
pub enum VerbosityLevel {
    /// Silent output
    Silent,
    /// Quiet output.  Only shows karva events up to the [`ERROR`](tracing::Level::ERROR).
    /// Silences output except for summary information.
    Quiet,

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
    pub const fn level_filter(self) -> LevelFilter {
        match self {
            Self::Silent => LevelFilter::OFF,
            Self::Quiet => LevelFilter::ERROR,
            Self::Default => LevelFilter::WARN,
            Self::Verbose => LevelFilter::INFO,
            Self::ExtraVerbose => LevelFilter::DEBUG,
            Self::Trace => LevelFilter::TRACE,
        }
    }

    pub const fn is_quiet(self) -> bool {
        matches!(self, Self::Quiet | Self::Silent)
    }

    pub const fn is_default(self) -> bool {
        matches!(self, Self::Default)
    }

    pub const fn is_trace(self) -> bool {
        matches!(self, Self::Trace)
    }

    pub const fn is_extra_verbose(self) -> bool {
        matches!(self, Self::ExtraVerbose)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verbosity_level_filter() {
        assert_eq!(VerbosityLevel::Default.level_filter(), LevelFilter::WARN);
        assert_eq!(VerbosityLevel::Verbose.level_filter(), LevelFilter::INFO);
        assert_eq!(
            VerbosityLevel::ExtraVerbose.level_filter(),
            LevelFilter::DEBUG
        );
        assert_eq!(VerbosityLevel::Trace.level_filter(), LevelFilter::TRACE);
    }

    #[test]
    fn test_verbosity_level_is_trace() {
        assert!(VerbosityLevel::Trace.is_trace());
    }

    #[test]
    fn test_verbosity_level_is_extra_verbose() {
        assert!(VerbosityLevel::ExtraVerbose.is_extra_verbose());
    }
}
