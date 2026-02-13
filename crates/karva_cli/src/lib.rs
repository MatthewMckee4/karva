use camino::Utf8PathBuf;
use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::{AnsiColor, Effects};
use karva_logging::{TerminalColor, VerbosityLevel};
use karva_metadata::{Options, SrcOptions, TerminalOptions, TestOptions};
use ruff_db::diagnostic::DiagnosticFormat;

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default());

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
    pub const fn level(&self) -> VerbosityLevel {
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

#[derive(Debug, Parser)]
#[command(author, name = "karva", about = "A Python test runner.")]
#[command(version)]
#[command(styles = STYLES)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Run tests.
    Test(TestCommand),

    /// Manage snapshots created by `karva.assert_snapshot()`.
    Snapshot(SnapshotCommand),

    /// Display Karva's version
    Version,
}

#[derive(Debug, Parser)]
pub struct SnapshotCommand {
    #[command(subcommand)]
    pub action: SnapshotAction,
}

#[derive(Debug, clap::Subcommand)]
pub enum SnapshotAction {
    /// Accept all (or filtered) pending snapshots.
    Accept(SnapshotFilterArgs),

    /// Reject all (or filtered) pending snapshots.
    Reject(SnapshotFilterArgs),

    /// List pending snapshots.
    Pending(SnapshotFilterArgs),

    /// Interactively review pending snapshots.
    Review(SnapshotFilterArgs),

    /// Remove snapshot files whose source test no longer exists.
    Prune(SnapshotPruneArgs),

    /// Delete all (or filtered) snapshot files (.snap and .snap.new).
    Delete(SnapshotDeleteArgs),
}

#[derive(Debug, Parser, Default)]
pub struct SnapshotFilterArgs {
    /// Optional paths to filter snapshots by directory or file.
    #[clap(value_name = "PATH")]
    pub paths: Vec<String>,
}

#[derive(Debug, Parser, Default)]
pub struct SnapshotPruneArgs {
    /// Optional paths to filter snapshots by directory or file.
    #[clap(value_name = "PATH")]
    pub paths: Vec<String>,

    /// Show which snapshots would be removed without deleting them.
    #[clap(long)]
    pub dry_run: bool,
}

#[derive(Debug, Parser, Default)]
pub struct SnapshotDeleteArgs {
    /// Optional paths to filter which snapshot files are deleted.
    #[clap(value_name = "PATH")]
    pub paths: Vec<String>,

    /// Show which snapshot files would be deleted without removing them.
    #[clap(long)]
    pub dry_run: bool,
}

/// Shared test execution options that can be used by both main CLI and worker processes
#[derive(Debug, Parser, Clone, Default)]
pub struct SubTestCommand {
    /// List of files or directories to test.
    #[clap(
        help = "List of files, directories, or test functions to test [default: the project root]",
        value_name = "PATH"
    )]
    pub paths: Vec<String>,

    /// The prefix of the test functions.
    #[clap(long)]
    pub test_prefix: Option<String>,

    /// The format to use for printing diagnostic messages.
    #[arg(long)]
    pub output_format: Option<OutputFormat>,

    /// Show Python stdout during test execution.
    #[clap(short = 's', default_missing_value = "true", num_args=0..1)]
    pub show_output: Option<bool>,

    /// When set, .gitignore files will not be respected.
    #[clap(long, default_missing_value = "true", num_args=0..1)]
    pub no_ignore: Option<bool>,

    /// When set, the test will fail immediately if any test fails.
    ///
    /// This only works when running tests in parallel.
    #[clap(long, default_missing_value = "true", num_args=0..1)]
    pub fail_fast: Option<bool>,

    /// When set, the test will retry failed tests up to this number of times.
    #[clap(long)]
    pub retry: Option<u32>,

    /// When set, we will not show individual test case results during execution.
    #[clap(long, default_missing_value = "true", num_args=0..1)]
    pub no_progress: Option<bool>,

    /// When set, we will try to import functions in each test file as well as parsing the ast to find them.
    ///
    /// This is often slower, so it is not recommended for most projects.
    #[clap(long, default_missing_value = "true", num_args=0..1)]
    pub try_import_fixtures: Option<bool>,

    /// Control when colored output is used.
    #[arg(long)]
    pub color: Option<TerminalColor>,

    /// Filter tests by tag expression. Only tests with matching custom tags will run.
    ///
    /// Expressions support `and`, `or`, `not`, and parentheses for grouping.
    /// When specified multiple times, a test runs if it matches any of the expressions.
    ///
    /// Examples: `-t slow`, `-t 'not slow'`, `-t 'slow and integration'`,
    /// `-t 'slow or integration'`, `-t '(slow or fast) and not flaky'`.
    #[clap(short = 't', long = "tag")]
    pub tag_expressions: Vec<String>,

    /// Filter tests by name using a regular expression.
    ///
    /// Only tests whose fully qualified name matches the pattern will run.
    /// Uses partial matching (the pattern can match anywhere in the name).
    /// When specified multiple times, a test runs if it matches any of the patterns.
    ///
    /// Examples: `-m auth`, `-m '^test::test_login'`, `-m 'slow|fast'`.
    #[clap(short = 'm', long = "match")]
    pub name_patterns: Vec<String>,

    /// Update snapshots directly instead of creating pending `.snap.new` files.
    ///
    /// When set, `karva.assert_snapshot()` will write directly to `.snap` files,
    /// accepting any changes automatically.
    #[clap(long, default_missing_value = "true", num_args=0..1)]
    pub snapshot_update: Option<bool>,

    #[clap(flatten)]
    pub verbosity: Verbosity,
}

#[derive(Debug, Parser)]
pub struct TestCommand {
    #[clap(flatten)]
    pub sub_command: SubTestCommand,

    /// The path to a `karva.toml` file to use for configuration.
    ///
    /// While karva configuration can be included in a `pyproject.toml` file, it is not allowed in this context.
    #[arg(long, env = "KARVA_CONFIG_FILE", value_name = "PATH")]
    pub config_file: Option<Utf8PathBuf>,

    /// Number of parallel workers (default: number of CPU cores)
    #[clap(short = 'n', long)]
    pub num_workers: Option<usize>,

    /// Disable parallel execution (equivalent to `--num-workers 1`)
    #[clap(long, default_missing_value = "true", num_args=0..1)]
    pub no_parallel: Option<bool>,

    /// Disable reading the karva cache for test duration history
    #[clap(long, default_missing_value = "true", num_args=0..1)]
    pub no_cache: Option<bool>,

    /// Print discovered tests without executing them.
    #[clap(long)]
    pub dry_run: bool,
}

impl TestCommand {
    pub const fn verbosity(&self) -> &Verbosity {
        &self.sub_command.verbosity
    }
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

impl From<OutputFormat> for karva_metadata::OutputFormat {
    fn from(format: OutputFormat) -> Self {
        match format {
            OutputFormat::Full => Self::Full,
            OutputFormat::Concise => Self::Concise,
        }
    }
}

impl SubTestCommand {
    pub fn into_options(self) -> Options {
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
                try_import_fixtures: self.try_import_fixtures,
                retry: self.retry,
            }),
        }
    }
}

impl TestCommand {
    pub fn into_options(self) -> Options {
        self.sub_command.into_options()
    }
}
