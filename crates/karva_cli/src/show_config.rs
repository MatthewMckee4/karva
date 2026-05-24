use camino::Utf8PathBuf;
use clap::Parser;

/// Print the resolved configuration karva would run with.
///
/// Resolves the same settings the test runner builds — defaults layered with
/// `karva.toml` / `pyproject.toml` and any selected profile — and prints them
/// as TOML.
#[derive(Debug, Parser)]
pub struct ShowConfigCommand {
    /// The path to a `karva.toml` file to use for configuration.
    #[arg(
        long,
        env = "KARVA_CONFIG_FILE",
        value_name = "PATH",
        help_heading = "Config options"
    )]
    pub config_file: Option<Utf8PathBuf>,

    /// Configuration profile to resolve.
    ///
    /// Defaults to `default`.
    #[arg(
        short = 'P',
        long,
        env = "KARVA_PROFILE",
        value_name = "NAME",
        help_heading = "Config options"
    )]
    pub profile: Option<String>,
}
