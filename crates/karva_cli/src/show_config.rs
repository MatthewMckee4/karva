use camino::Utf8PathBuf;
use clap::Parser;
use karva_static::EnvVars;

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
        env = EnvVars::KARVA_CONFIG_FILE,
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
        env = EnvVars::KARVA_PROFILE,
        value_name = "NAME",
        help_heading = "Config options"
    )]
    pub profile: Option<String>,
}
