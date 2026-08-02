use karva_logging::VerbosityLevel;

#[derive(clap::Args, Debug, Clone, Default)]
#[command(about = None, long_about = None)]
/// Repeatable `-v` flags mapped onto Karva's bounded verbosity levels.
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
