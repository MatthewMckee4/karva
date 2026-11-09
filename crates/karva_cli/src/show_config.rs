use camino::Utf8PathBuf;
use clap::Parser;
use karva_static::EnvVars;

/// Configuration location and profile shared by source checks and configuration display.
#[derive(Debug, Parser)]
pub struct ConfigArgs {
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

/// Arguments for printing resolved configuration.
pub type ShowConfigCommand = ConfigArgs;
