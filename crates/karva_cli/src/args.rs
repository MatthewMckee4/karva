use camino::Utf8PathBuf;
use clap::{
    Parser,
    builder::{
        Styles,
        styling::{AnsiColor, Effects},
    },
};
use karva_project::{Options, SrcOptions, TerminalOptions, TestOptions};
use ruff_db::diagnostic::DiagnosticFormat;

use crate::logging::Verbosity;

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default());

#[derive(Debug, Parser)]
#[command(author, name = "karva", about = "A Python test runner.")]
#[command(version)]
#[command(styles = STYLES)]
pub struct Args {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Run tests.
    Test(TestCommand),

    /// Display Karva's version
    Version,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Parser)]
pub struct TestCommand {
    /// List of files or directories to test.
    #[clap(
        help = "List of files, directories, or test functions to test [default: the project root]",
        value_name = "PATH"
    )]
    pub(crate) paths: Vec<String>,

    #[clap(flatten)]
    pub(crate) verbosity: Verbosity,

    /// The path to a `karva.toml` file to use for configuration.
    ///
    /// While ty configuration can be included in a `pyproject.toml` file, it is not allowed in this context.
    #[arg(long, env = "KARVA_CONFIG_FILE", value_name = "PATH")]
    pub(crate) config_file: Option<Utf8PathBuf>,

    /// The prefix of the test functions.
    #[clap(long)]
    pub(crate) test_prefix: Option<String>,

    /// The format to use for printing diagnostic messages.
    #[arg(long)]
    pub(crate) output_format: Option<OutputFormat>,

    /// Show Python stdout during test execution.
    #[clap(short = 's', default_missing_value = "true", num_args=0..1)]
    pub(crate) show_output: Option<bool>,

    /// When set, .gitignore files will not be respected.
    #[clap(long, default_missing_value = "true", num_args=0..1)]
    pub(crate) no_ignore: Option<bool>,

    /// When set, the test will fail immediately if any test fails.
    #[clap(long, default_missing_value = "true", num_args=0..1)]
    pub(crate) fail_fast: Option<bool>,

    /// When set, we will not show individual test case results during execution.
    #[clap(long, default_missing_value = "true", num_args=0..1)]
    pub(crate) no_progress: Option<bool>,

    /// Control when colored output is used.
    #[arg(long)]
    pub(crate) color: Option<TerminalColor>,
}

/// The diagnostic output format.
#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq, PartialOrd, Ord, Default, clap::ValueEnum)]
pub enum OutputFormat {
    /// Print diagnostics verbosely, with context and helpful hints (default).
    #[default]
    #[value(name = "full")]
    Full,

    /// Print diagnostics concisely, one per line.
    #[value(name = "concise")]
    Concise,
}

impl From<OutputFormat> for DiagnosticFormat {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Full => Self::Full,
            OutputFormat::Concise => Self::Concise,
        }
    }
}

impl From<OutputFormat> for karva_project::OutputFormat {
    fn from(format: OutputFormat) -> Self {
        match format {
            OutputFormat::Full => Self::Full,
            OutputFormat::Concise => Self::Concise,
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

impl TestCommand {
    pub(crate) fn into_options(self) -> Options {
        Options {
            src: Some(SrcOptions {
                respect_ignore_files: self.no_ignore.map(|no_ignore| !no_ignore),
                include: Some(self.paths),
            }),
            terminal: Some(TerminalOptions {
                output_format: self.output_format.map(Into::into),
                show_python_output: self.show_output,
            }),
            test: Some(TestOptions {
                test_function_prefix: self.test_prefix,
                fail_fast: self.fail_fast,
            }),
        }
    }
}
