use std::num::NonZeroU32;

use camino::Utf8PathBuf;
use clap::Parser;
use karva_logging::{FinalStatusLevel, ProgressMode, StatusLevel, TerminalColor};
use karva_metadata::{
    CovFailUnder, CoverageOptions, MaxFail, Options, SlowTimeoutSecs, SrcOptions, TerminalOptions,
    TestOptions, TestTimeoutSecs,
};

use crate::enums::{CovReport, NoTests, OutputFormat, RunIgnored};
use crate::partition::PartitionSelection;
use crate::verbosity::Verbosity;

/// Shared test execution options that can be used by both main CLI and worker processes
#[derive(Debug, Parser, Clone, Default)]
pub struct SubTestCommand {
    /// List of files or directories to test.
    #[clap(
        help = "List of files, directories, or test functions to test [default: the project root]",
        value_name = "PATH"
    )]
    pub paths: Vec<String>,

    /// Control when colored output is used.
    #[arg(long)]
    pub color: Option<TerminalColor>,

    #[clap(flatten)]
    pub verbosity: Verbosity,

    /// The prefix of the test functions.
    #[clap(long, help_heading = "Filter options")]
    pub test_prefix: Option<String>,

    /// When set, .gitignore files will not be respected.
    #[clap(long, default_missing_value = "true", num_args=0..1, help_heading = "Filter options")]
    pub no_ignore: Option<bool>,

    /// When set, we will try to import functions in each test file as well as parsing the ast to find them.
    ///
    /// This is often slower, so it is not recommended for most projects.
    #[clap(long, default_missing_value = "true", num_args=0..1, help_heading = "Filter options")]
    pub try_import_fixtures: Option<bool>,

    /// Filter tests using a filterset expression.
    ///
    /// Predicates: `test(<matcher>)` matches the fully qualified test name;
    /// `tag(<matcher>)` matches any custom tag on the test.
    ///
    /// Matchers: `=exact`, `~substring`, `/regex/`, `#glob`. The default is
    /// substring for `test()` and exact for `tag()`. String bodies may be
    /// quoted (`"..."`) to allow spaces or reserved characters.
    ///
    /// Operators: `&` / `and`, `|` / `or`, `not` / `!`, and `-` as
    /// shorthand for "and not". Use parentheses for grouping. `and` binds
    /// tighter than `or`.
    ///
    /// When specified multiple times, a test runs if it matches any of the
    /// expressions (OR semantics across flags).
    ///
    /// Examples: `-E 'tag(slow)'`, `-E 'test(/^mod::test_login$/)'`,
    /// `-E 'tag(slow) & test(~login)'`,
    /// `-E '(tag(fast) | tag(unit)) - tag(flaky)'`.
    #[clap(short = 'E', long = "filter", help_heading = "Filter options")]
    pub filter_expressions: Vec<String>,

    /// Behavior when no tests are found to run [default: auto]
    #[arg(
        long,
        value_name = "ACTION",
        env = "KARVA_NO_TESTS",
        help_heading = "Filter options"
    )]
    pub no_tests: Option<NoTests>,

    /// Run ignored tests.
    #[arg(long, help_heading = "Filter options")]
    pub run_ignored: Option<RunIgnored>,

    /// Stop scheduling new tests after this many failures.
    ///
    /// Accepts a positive integer such as `--max-fail=3`. `--max-fail=1` is
    /// equivalent to the legacy `--fail-fast`, and `--no-fail-fast` clears
    /// the limit. When `--max-fail` is provided alongside `--fail-fast` or
    /// `--no-fail-fast`, `--max-fail` takes precedence.
    #[clap(long, value_name = "N", help_heading = "Runner options")]
    pub max_fail: Option<NonZeroU32>,

    /// Stop scheduling new tests after the first failure.
    ///
    /// Equivalent to `--max-fail=1`. Use `--no-fail-fast` to keep running
    /// after failures.
    #[clap(long, default_missing_value = "true", num_args=0..1, overrides_with = "no_fail_fast", help_heading = "Runner options")]
    pub fail_fast: Option<bool>,

    /// Run every test regardless of how many fail.
    ///
    /// Clears any `fail-fast` or `max-fail` value set in configuration. When
    /// `--max-fail` is provided alongside `--no-fail-fast`, `--max-fail`
    /// takes precedence.
    #[clap(long, action = clap::ArgAction::SetTrue, overrides_with = "fail_fast", help_heading = "Runner options")]
    pub no_fail_fast: bool,

    /// When set, the test will retry failed tests up to this number of times.
    #[clap(long, help_heading = "Runner options")]
    pub retry: Option<u32>,

    /// Threshold in seconds after which a test is flagged as slow.
    ///
    /// When a test takes longer than this duration, it is reported with a
    /// `SLOW` status line (gated on `--status-level=slow` or higher) and
    /// counted in the run summary. Pass a positive number such as
    /// `--slow-timeout=60` or `--slow-timeout=0.5`.
    #[clap(long, value_name = "SECONDS", help_heading = "Runner options")]
    pub slow_timeout: Option<f64>,

    /// Hard per-test timeout, in seconds.
    ///
    /// Tests that run longer than this duration are killed and reported
    /// as failures. A test-level [`@karva.tags.timeout`] decorator
    /// overrides the default for that specific test.
    ///
    /// Accepts fractional seconds such as `--timeout=120` or `--timeout=0.5`.
    #[clap(long, value_name = "SECONDS", help_heading = "Runner options")]
    pub timeout: Option<f64>,

    /// Update snapshots directly instead of creating pending `.snap.new` files.
    ///
    /// When set, `karva.assert_snapshot()` will write directly to `.snap` files,
    /// accepting any changes automatically.
    #[clap(long, default_missing_value = "true", num_args=0..1, help_heading = "Runner options")]
    pub snapshot_update: Option<bool>,

    /// The format to use for printing diagnostic messages.
    #[arg(long, help_heading = "Reporter options")]
    pub output_format: Option<OutputFormat>,

    /// Show Python stdout during test execution.
    #[clap(short = 's', long, default_missing_value = "true", num_args=0..1, help_heading = "Reporter options")]
    pub show_output: Option<bool>,

    /// Test result statuses to display during the run [default: pass]
    #[arg(
        long,
        value_name = "LEVEL",
        env = "KARVA_STATUS_LEVEL",
        help_heading = "Reporter options"
    )]
    pub status_level: Option<StatusLevel>,

    /// Test summary information to display at the end of the run [default: pass]
    #[arg(
        long,
        value_name = "LEVEL",
        env = "KARVA_FINAL_STATUS_LEVEL",
        help_heading = "Reporter options"
    )]
    pub final_status_level: Option<FinalStatusLevel>,

    /// Live progress display while tests run [default: none]
    ///
    /// `none` leaves output untouched. `counter` prints a refreshing
    /// `N/M tests` line on stderr. `bar` renders a visual progress bar.
    /// Stderr is used so the display does not interfere with per-test
    /// result lines on stdout.
    #[arg(
        long,
        value_name = "MODE",
        env = "KARVA_SHOW_PROGRESS",
        help_heading = "Reporter options"
    )]
    pub show_progress: Option<ProgressMode>,

    /// Measure code coverage for the given source path.
    ///
    /// May be passed multiple times to measure several sources. Pass without
    /// a value (`--cov`) to measure the current working directory.
    #[clap(
        long = "cov",
        value_name = "SOURCE",
        num_args = 0..=1,
        default_missing_value = "",
        action = clap::ArgAction::Append,
        overrides_with = "no_cov",
        help_heading = "Coverage options"
    )]
    pub cov: Vec<String>,

    /// Disable coverage measurement for this run.
    ///
    /// Overrides any `--cov` flag and any `[coverage] sources` configured in
    /// `karva.toml` / `pyproject.toml`. Useful when iterating locally without
    /// editing config.
    #[clap(
        long = "no-cov",
        action = clap::ArgAction::SetTrue,
        overrides_with = "cov",
        help_heading = "Coverage options"
    )]
    pub no_cov: bool,

    /// Coverage terminal report type.
    ///
    /// `term` (default) prints a compact terminal table.
    /// `term-missing` extends it with a `Missing` column listing the
    /// uncovered line numbers per file.
    #[clap(
        long = "cov-report",
        value_name = "TYPE",
        value_enum,
        help_heading = "Coverage options"
    )]
    pub cov_report: Option<CovReport>,

    /// Fail the run if total coverage is below the given percentage.
    ///
    /// Accepts any value in `0..=100` (fractional values such as `90.5`
    /// are allowed). When the reported `TOTAL` percentage is below the
    /// threshold, the test command exits with a non-zero status even if
    /// every test passed. Has no effect when tests have already failed.
    #[clap(
        long = "cov-fail-under",
        value_name = "PERCENT",
        value_parser = parse_cov_fail_under,
        help_heading = "Coverage options"
    )]
    pub cov_fail_under: Option<f64>,

    /// Internal: per-worker coverage data file path.
    ///
    /// Set automatically by the runner when `--cov` is enabled. Not intended
    /// for direct use.
    #[clap(long, hide = true, value_name = "PATH")]
    pub cov_data_file: Option<Utf8PathBuf>,
}

#[derive(Debug, Parser)]
pub struct TestCommand {
    #[clap(flatten)]
    pub sub_command: SubTestCommand,

    /// Re-run only the tests that failed in the previous run.
    #[clap(long, alias = "lf", help_heading = "Filter options")]
    pub last_failed: bool,

    /// Run only a slice of the collected tests, distributed round-robin.
    ///
    /// Accepts `slice:M/N` where this run executes slice `M` of `N` total
    /// slices (1-indexed). Tests are sorted by qualified name and then
    /// distributed by cycling through slices: test 1 to slice 1, test 2 to
    /// slice 2, ..., test N+1 to slice 1, and so on. Running every
    /// `slice:1/N` through `slice:N/N` together covers every collected test
    /// exactly once.
    ///
    /// Useful for splitting a test run across CI jobs. Slice membership
    /// shifts when tests are added or removed, so it gives less stable
    /// per-test placement than a hash-based scheme.
    #[clap(long, value_name = "STRATEGY:M/N", help_heading = "Filter options")]
    pub partition: Option<PartitionSelection>,

    /// Number of parallel workers (default: number of CPU cores)
    #[clap(short = 'n', long, help_heading = "Runner options")]
    pub num_workers: Option<usize>,

    /// Disable parallel execution (equivalent to `--num-workers 1`)
    #[clap(long, default_missing_value = "true", num_args=0..1, help_heading = "Runner options")]
    pub no_parallel: Option<bool>,

    /// Disable output capture and run tests serially.
    ///
    /// Lets stdout/stderr from tests flow directly to the terminal,
    /// useful when debugging with print statements or interactive
    /// debuggers. Implies `--show-output` and forces a single worker
    /// so output from concurrent tests cannot interleave.
    #[clap(long, action = clap::ArgAction::SetTrue, help_heading = "Runner options")]
    pub no_capture: bool,

    /// Disable reading the karva cache for test duration history
    #[clap(long, default_missing_value = "true", num_args=0..1, help_heading = "Runner options")]
    pub no_cache: Option<bool>,

    /// Re-run tests when Python source files change.
    #[clap(long, help_heading = "Runner options")]
    pub watch: bool,

    /// Show the N slowest tests after the run completes.
    #[clap(long, value_name = "N", help_heading = "Reporter options")]
    pub durations: Option<usize>,

    /// The path to a `karva.toml` file to use for configuration.
    ///
    /// While karva configuration can be included in a `pyproject.toml` file, it is not allowed in this context.
    #[arg(
        long,
        env = "KARVA_CONFIG_FILE",
        value_name = "PATH",
        help_heading = "Config options"
    )]
    pub config_file: Option<Utf8PathBuf>,

    /// Configuration profile to use.
    ///
    /// Profiles are defined as `[profile.<name>]` sections in `karva.toml`
    /// (or `[tool.karva.profile.<name>]` in `pyproject.toml`) and may
    /// override any of the `[src]`, `[terminal]`, and `[test]` settings.
    /// The selected profile is layered on top of any `[profile.default]`
    /// overrides, which themselves layer on top of the top-level options.
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

impl TestCommand {
    pub fn verbosity(&self) -> &Verbosity {
        &self.sub_command.verbosity
    }
}

impl SubTestCommand {
    pub fn into_options(self) -> Options {
        // `--no-fail-fast` forces `fail_fast = false` and clears any
        // `max-fail` limit from config. `overrides_with` guarantees
        // `--fail-fast` and `--no-fail-fast` cannot both be active.
        // An explicit `--max-fail=N` still wins over `--no-fail-fast`.
        let fail_fast = if self.no_fail_fast {
            Some(false)
        } else {
            self.fail_fast
        };

        let max_fail = match (self.max_fail, self.no_fail_fast) {
            (Some(n), _) => Some(MaxFail::from(n)),
            (None, true) => Some(MaxFail::unlimited()),
            (None, false) => None,
        };

        Options {
            src: Some(SrcOptions {
                respect_ignore_files: self.no_ignore.map(|no_ignore| !no_ignore),
                include: Some(self.paths),
            }),
            terminal: Some(TerminalOptions {
                output_format: self.output_format.map(Into::into),
                show_python_output: self.show_output,
                status_level: self.status_level,
                final_status_level: self.final_status_level,
                show_progress: self.show_progress,
            }),
            test: Some(TestOptions {
                test_function_prefix: self.test_prefix,
                fail_fast,
                max_fail,
                try_import_fixtures: self.try_import_fixtures,
                retry: self.retry,
                no_tests: self.no_tests.map(Into::into),
                slow_timeout: self.slow_timeout.map(SlowTimeoutSecs),
                timeout: self.timeout.map(TestTimeoutSecs),
            }),
            coverage: Some(CoverageOptions {
                sources: (!self.cov.is_empty()).then(|| self.cov.clone()),
                report: self.cov_report.map(Into::into),
                fail_under: self.cov_fail_under.map(CovFailUnder),
                disabled: self.no_cov.then_some(true),
            }),
        }
    }
}

impl TestCommand {
    pub fn into_options(self) -> Options {
        let mut sub_command = self.sub_command;
        if self.no_capture {
            sub_command.show_output = Some(true);
        }
        sub_command.into_options()
    }
}

/// Parse and validate a `--cov-fail-under=N` argument.
///
/// Accepts any finite percentage in `0..=100`.
fn parse_cov_fail_under(raw: &str) -> Result<f64, String> {
    let value: f64 = raw
        .parse()
        .map_err(|err| format!("`{raw}` is not a valid number: {err}"))?;
    if !value.is_finite() || !(0.0..=100.0).contains(&value) {
        return Err(format!("must be between 0 and 100, got `{raw}`"));
    }
    Ok(value)
}
