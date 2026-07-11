use karva_logging::VerbosityLevel;

#[derive(clap::Args, Debug, Clone, Default)]
#[command(about = None, long_about = None)]
pub struct Verbosity {
    #[arg(
        long,
        short = 'v',
        help = "Use verbose output (or `-vv` and `-vvv` for more verbose output)",
        action = clap::ArgAction::Count,
        global = true,
    )]
    verbose: u8,
}

impl Verbosity {
    /// Returns the verbosity level based on the number of `-v` flags.
    pub fn level(&self) -> VerbosityLevel {
        match self.verbose {
            0 => VerbosityLevel::Default,
            1 => VerbosityLevel::Verbose,
            2 => VerbosityLevel::ExtraVerbose,
            _ => VerbosityLevel::Trace,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbosity_count_maps_to_level() {
        assert_eq!(Verbosity { verbose: 0 }.level(), VerbosityLevel::Default);
        assert_eq!(Verbosity { verbose: 1 }.level(), VerbosityLevel::Verbose);
        assert_eq!(
            Verbosity { verbose: 2 }.level(),
            VerbosityLevel::ExtraVerbose
        );
        assert_eq!(Verbosity { verbose: 3 }.level(), VerbosityLevel::Trace);
        assert_eq!(
            Verbosity { verbose: u8::MAX }.level(),
            VerbosityLevel::Trace
        );
    }
}
