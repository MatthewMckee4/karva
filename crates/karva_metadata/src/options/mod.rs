//! Layered configuration models before resolution into runtime settings.

mod config;
mod overrides;

use std::collections::BTreeMap;

use karva_combine::Combine;
use karva_logging::{FinalStatusLevel, StatusLevel};
use karva_macros::{Combine, OptionsMetadata};
use serde::{Deserialize, Serialize};

pub use config::{Config, IncompatibleVersionError, KarvaTomlError, UnknownProfile};
pub use overrides::ProjectOptionsOverrides;

/// The implicit name of the default profile.
pub const DEFAULT_PROFILE: &str = "default";

use crate::filter::{FiltersetSet, ValidatedFilter};
use crate::max_fail::MaxFail;
use crate::settings::{
    CovFailUnder, CoverageExcludePattern, CoveragePartialPattern, CoveragePrecision,
    CoverageSettings, DEFAULT_COVERAGE_DATA_FILE, FailSlowSecs, FlakyResult, JunitFlakyFailStatus,
    JunitSettings, NoTestsMode, OverrideSettings, ProjectSettings, RunIgnoredMode, RunTimeoutSecs,
    SlowTimeoutSecs, SrcSettings, TerminalSettings, TerminationGracePeriodSecs, TestSettings,
    TestTimeoutSecs,
};
use crate::{EnvironmentVariable, EnvironmentVariableName};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, OptionsMetadata)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
/// Configuration groups combined across defaults, profiles, environment, and CLI.
pub struct Options {
    /// Environment variables applied to test workers before Python imports any
    /// test modules or fixtures. Strings always set values. Use
    /// `{ value = "...", preserve = true }` to keep an existing value, or
    /// `{ unset = true }` to remove one. Karva's own variables are reserved.
    #[option(
        default = r#"{}"#,
        value_type = "table",
        example = r#"
            [tool.karva.profile.default.env]
            APP_ENV = "test"
            CACHE_DIR = { value = ".cache/tests", preserve = true }
            LIVE_API_TOKEN = { unset = true }
        "#
    )]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<EnvironmentVariableName, EnvironmentVariable>,

    /// Source discovery overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option_group]
    pub src: Option<SrcOptions>,

    /// Terminal presentation overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option_group]
    pub terminal: Option<TerminalOptions>,

    /// Test execution overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option_group]
    pub test: Option<TestOptions>,

    /// Coverage collection and report overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option_group]
    pub coverage: Option<CoverageOptions>,

    /// `JUnit` report overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option_group]
    pub junit: Option<JunitOptions>,

    /// Per-test configuration overrides.
    ///
    /// Each entry pairs a [filter expression](#filter) with one or more
    /// option overrides. The first override whose filter matches the
    /// running test wins for any given option. Fields left unset on a
    /// matching override fall through to the next match (or the
    /// profile-level default).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<OverrideOptions>,
}

impl Combine for Options {
    fn combine_with(&mut self, other: Self) {
        Combine::combine_with(&mut self.env, other.env);
        Combine::combine_with(&mut self.src, other.src);
        Combine::combine_with(&mut self.terminal, other.terminal);
        Combine::combine_with(&mut self.test, other.test);
        Combine::combine_with(&mut self.coverage, other.coverage);
        Combine::combine_with(&mut self.junit, other.junit);
        // Overrides obey "first match wins"; higher-precedence entries
        // (i.e. those from `self`) must come first, so prepend rather
        // than using the default `Vec::combine_with` which appends.
        self.overrides.extend(other.overrides);
    }
}

impl Options {
    /// Resolves sparse values into complete runtime settings and compiled overrides.
    pub fn to_settings(&self) -> ProjectSettings {
        ProjectSettings {
            tags: BTreeMap::new(),
            env: self.env.clone(),
            terminal: self.terminal.clone().unwrap_or_default().to_settings(),
            src: self.src.clone().unwrap_or_default().to_settings(),
            test: self.test.clone().unwrap_or_default().to_settings(),
            coverage: self.coverage.clone().unwrap_or_default().to_settings(),
            junit: self.junit.clone().unwrap_or_default().to_settings(),
            overrides: self
                .overrides
                .iter()
                .map(OverrideOptions::to_settings)
                .collect(),
        }
    }
}

/// A single per-test override entry.
///
/// Mirrors `[[profile.<name>.overrides]]` in `karva.toml`. Each override
/// pairs a filter expression with one or more option fields to apply when
/// the filter matches a given test.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, OptionsMetadata)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct OverrideOptions {
    /// A filter expression evaluated against each test. Tests whose name
    /// or tags match the expression pick up this override's settings.
    #[option(
        default = "required",
        value_type = "string",
        example = r#"
            filter = "tag(slow)"
        "#
    )]
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    filter: ValidatedFilter,

    /// Number of times to retry a matching test before giving up. Mirrors
    /// the profile-level [`retry`](#retry) field.
    #[option(
        default = "null",
        value_type = "u32",
        example = r#"
            retries = 2
        "#
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    retries: Option<u32>,

    /// Whether matching flaky tests pass or fail the run.
    #[option(
        default = "null",
        value_type = "pass | fail",
        example = r#"
            flaky-result = "pass"
        "#
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    flaky_result: Option<FlakyResult>,

    /// `JUnit` behavior for matching flaky tests configured to fail.
    #[option_group]
    #[serde(skip_serializing_if = "Option::is_none")]
    junit: Option<OverrideJunitOptions>,

    /// Hard per-test timeout, in seconds, applied to matching tests.
    /// Mirrors the profile-level [`timeout`](#timeout) field. A value of
    /// `0` (or any non-positive value) disables the hard timeout for the
    /// matching test.
    #[option(
        default = "null",
        value_type = "float (seconds)",
        example = r#"
            timeout = 30.0
        "#
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<TestTimeoutSecs>,

    /// Threshold (in seconds) above which a matching test is flagged as
    /// slow. Mirrors the profile-level
    /// [`slow-timeout`](#slow-timeout) field. A non-positive value
    /// disables slow tracking for the matching test.
    #[option(
        default = "null",
        value_type = "float (seconds)",
        example = r#"
            slow-timeout = 1.0
        "#
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    slow_timeout: Option<SlowTimeoutSecs>,

    /// Duration budget (in seconds) for a matching test's full lifecycle.
    /// Mirrors the profile-level [`fail-slow`](#fail-slow) field. A
    /// non-positive value disables the budget for the matching test.
    #[option(
        default = "null",
        value_type = "float (seconds)",
        example = r#"
            fail-slow = 10.0
        "#
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    fail_slow: Option<FailSlowSecs>,
}

impl OverrideOptions {
    fn to_settings(&self) -> OverrideSettings {
        OverrideSettings {
            filter: self.filter.clone(),
            retries: self.retries,
            flaky_result: self.flaky_result,
            junit_flaky_fail_status: self
                .junit
                .as_ref()
                .and_then(|junit| junit.flaky_fail_status),
            timeout: self.timeout,
            slow_timeout: self.slow_timeout,
            fail_slow: self.fail_slow,
        }
    }
}

#[derive(Debug, Default, Clone, Eq, PartialEq, Serialize, Deserialize, OptionsMetadata)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
/// `JUnit`-specific values available inside one per-test override.
pub struct OverrideJunitOptions {
    /// Whether matching flaky-fail tests appear as failures or successes in
    /// `JUnit`, while preserving their flaky attempt details.
    #[option(
        default = "null",
        value_type = "failure | success",
        example = r#"
            flaky-fail-status = "success"
        "#
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    flaky_fail_status: Option<JunitFlakyFailStatus>,
}

#[derive(
    Debug, Default, Clone, Eq, PartialEq, Serialize, Deserialize, OptionsMetadata, Combine,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
/// Controls test-path discovery and whether filesystem ignore rules are honored.
pub struct SrcOptions {
    /// Whether to automatically exclude files that are ignored by `.ignore`,
    /// `.gitignore`, `.git/info/exclude`, and global `gitignore` files.
    /// Enabled by default.
    #[option(
        default = r#"true"#,
        value_type = r#"bool"#,
        example = r#"
            respect-ignore-files = false
        "#
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub respect_ignore_files: Option<bool>,

    /// A list of files, directories, and shell-style glob patterns to check.
    /// Including a file or directory will make it so that it (and its contents)
    /// are tested. Glob patterns are relative to the project root and support
    /// `*`, `?`, `[]`, and recursive `**` matching.
    /// When unset, Karva checks the `tests` directory if it exists, otherwise
    /// it checks the project root.
    ///
    /// - `tests` matches a directory named `tests`
    /// - `tests/test.py` matches a file named `test.py` in the `tests` directory
    /// - `tests/**/test_*.py` matches test files below the `tests` directory
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = r#"list[str]"#,
        example = r#"
            include = ["tests"]
        "#
    )]
    pub include: Option<Vec<String>>,
}

impl SrcOptions {
    fn to_settings(&self) -> SrcSettings {
        SrcSettings {
            respect_ignore_files: self.respect_ignore_files.unwrap_or(true),
            include_paths: self.include.clone().unwrap_or_default(),
        }
    }
}

#[derive(
    Debug, Default, Clone, Eq, PartialEq, Combine, Serialize, Deserialize, OptionsMetadata,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
/// Controls diagnostic formatting, captured output, and displayed test statuses.
pub struct TerminalOptions {
    /// The format to use for printing diagnostic messages.
    ///
    /// Defaults to `full`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"full"#,
        value_type = "full | concise",
        example = r#"
            output-format = "concise"
        "#
    )]
    pub output_format: Option<OutputFormat>,

    /// Whether to show the python output.
    ///
    /// This is the output the `print` goes to etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"true"#,
        value_type = "true | false",
        example = r#"
            show-python-output = false
        "#
    )]
    pub show_python_output: Option<bool>,

    /// Test result statuses to display during the run.
    ///
    /// Modeled after `cargo-nextest`'s `--status-level`. Levels are
    /// cumulative: `pass` shows passing and failed tests, `skip` adds
    /// skipped tests on top, and so on. `retry` and `slow` are accepted
    /// for forward-compatibility but currently behave like `fail`.
    ///
    /// Defaults to `pass`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"pass"#,
        value_type = "none | fail | retry | slow | pass | skip | all",
        example = r#"
            status-level = "fail"
        "#
    )]
    pub status_level: Option<StatusLevel>,

    /// Test summary information to display at the end of the run.
    ///
    /// Modeled after `cargo-nextest`'s `--final-status-level`. Levels are
    /// cumulative in the same way as [`status_level`](#status-level).
    ///
    /// Defaults to `pass`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"pass"#,
        value_type = "none | fail | retry | slow | pass | skip | all",
        example = r#"
            final-status-level = "fail"
        "#
    )]
    pub final_status_level: Option<FinalStatusLevel>,
}

impl TerminalOptions {
    /// Applies defaults and produces terminal settings ready for runtime use.
    fn to_settings(&self) -> TerminalSettings {
        TerminalSettings {
            output_format: self.output_format.unwrap_or_default(),
            show_python_output: self.show_python_output.unwrap_or_default(),
            status_level: self.status_level.unwrap_or_default(),
            final_status_level: self.final_status_level.unwrap_or_default(),
        }
    }
}

#[derive(
    Debug, Default, Clone, Eq, PartialEq, Combine, Serialize, Deserialize, OptionsMetadata,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
/// Controls test selection, retries, timeouts, and failure policies.
pub struct TestOptions {
    /// The prefix to use for test functions.
    ///
    /// Defaults to `test`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"test"#,
        value_type = "string",
        example = r#"
            test-function-prefix = "test"
        "#
    )]
    pub test_function_prefix: Option<String>,

    /// Reject custom tags that are absent from the project-wide `[tags]` registry.
    /// Built-in Karva tags and pytest marks remain available without registration.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"false"#,
        value_type = "true | false",
        example = r#"
            strict-tags = true
        "#
    )]
    pub strict_tags: Option<bool>,

    /// Whether to stop at the first test failure.
    ///
    /// This is a legacy alias for [`max_fail`](#max-fail): `true`
    /// corresponds to `max-fail = 1` and `false` leaves the limit unset.
    /// When both are set, `max-fail` takes precedence.
    ///
    /// Defaults to `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"false"#,
        value_type = "true | false",
        example = r#"
            fail-fast = true
        "#
    )]
    pub fail_fast: Option<bool>,

    /// Stop scheduling new tests once this many tests have failed.
    ///
    /// Accepts a positive integer. Omitting the field (the default) lets
    /// every test run regardless of how many fail. Setting `max-fail = 1`
    /// is equivalent to the legacy `fail-fast = true`.
    ///
    /// When both [`fail_fast`](#fail-fast) and `max-fail` are set,
    /// `max-fail` takes precedence.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = "unlimited",
        value_type = "positive integer",
        example = r#"
            max-fail = 3
        "#
    )]
    pub max_fail: Option<MaxFail>,

    /// When set, we will try to import functions in each test file as well as parsing the ast to find them.
    ///
    /// This is often slower, so it is not recommended for most projects.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"false"#,
        value_type = "true | false",
        example = r#"
            try-import-fixtures = true
        "#
    )]
    pub try_import_fixtures: Option<bool>,

    /// Collect examples from module, class, function, and method docstrings.
    ///
    /// Defaults to `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"false"#,
        value_type = "true | false",
        example = r#"
            doctest-modules = true
        "#
    )]
    pub doctest_modules: Option<bool>,

    /// When set, we will retry failed tests up to this number of times.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"0"#,
        value_type = "u32",
        example = r#"
            retry = 3
        "#
    )]
    pub retry: Option<u32>,

    /// Use seeded randomized ordering instead of duration-aware scheduling.
    ///
    /// Defaults to `false`. When enabled, Karva prints the seed used for the
    /// run so the same order can be reproduced.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"false"#,
        value_type = "true | false",
        example = r#"
            shuffle = true
        "#
    )]
    pub shuffle: Option<bool>,

    /// Seed used to randomize test order when [`shuffle`](#shuffle) is enabled.
    ///
    /// When omitted, Karva generates and prints a seed for the run. Setting a
    /// seed does not enable shuffling by itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = "u64",
        example = r#"
            random-seed = 170938
        "#
    )]
    pub random_seed: Option<u64>,

    /// Whether tests that pass only after a retry should fail the run.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"pass"#,
        value_type = "pass | fail",
        example = r#"
            flaky-result = "fail"
        "#
    )]
    pub flaky_result: Option<FlakyResult>,

    /// Configures behavior when no tests are found to run.
    ///
    /// `auto` (the default) fails when no filter expressions were given, and
    /// passes silently when filters were given. Use `fail` to always fail,
    /// `warn` to always warn, or `pass` to always succeed silently.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"auto"#,
        value_type = "auto | pass | warn | fail",
        example = r#"
            no-tests = "warn"
        "#
    )]
    pub no_tests: Option<NoTestsMode>,

    /// Threshold (in seconds) after which a test is flagged as slow.
    ///
    /// When set, tests that take longer than this duration are reported with
    /// a `SLOW` status line and counted in the run summary. The `SLOW` line
    /// is gated on `--status-level=slow` (or higher); the summary always
    /// shows the slow count when `--final-status-level=slow` is set.
    ///
    /// Defaults to unset, which disables slow-test detection.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = "float (seconds)",
        example = r#"
            slow-timeout = 60.0
        "#
    )]
    pub slow_timeout: Option<SlowTimeoutSecs>,

    /// Hard per-test timeout (in seconds).
    ///
    /// When set, every test that runs longer than this duration is killed
    /// and reported as a failure. Tests can override the limit individually
    /// with [`@karva.tags.timeout`](https://docs.karva.dev/usage/tags/timeout/),
    /// which takes precedence over the configured default.
    ///
    /// Defaults to unset, which disables hard timeouts unless a tag is
    /// applied to the test.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = "float (seconds)",
        example = r#"
            timeout = 120.0
        "#
    )]
    pub timeout: Option<TestTimeoutSecs>,

    /// Duration budget (in seconds) for a test's full lifecycle (fixture
    /// setup, the test call, and fixture teardown).
    ///
    /// When set, a test is allowed to run to completion — including
    /// teardown, so cleanup is never skipped — and is then reported as a
    /// failure if the full lifecycle took longer than this budget. Tests
    /// can override the limit individually with
    /// [`@karva.tags.fail_slow`](https://docs.karva.dev/usage/failure-handling/fail-slow/),
    /// which takes precedence over the configured default.
    ///
    /// This is distinct from [`timeout`](#timeout), which kills a test
    /// mid-execution, and [`slow_timeout`](#slow-timeout), which is purely
    /// informational and never fails a test.
    ///
    /// Defaults to unset, which disables budget checking unless a tag is
    /// applied to the test.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = "float (seconds)",
        example = r#"
            fail-slow = 0.25
        "#
    )]
    pub fail_slow: Option<FailSlowSecs>,

    /// Wall-clock limit (in seconds) for the entire run.
    ///
    /// When the run takes longer than this duration, karva stops the
    /// remaining workers and exits with a failure status. This is a safety
    /// net for CI to bound runaway suites; it does not affect individual
    /// test results that already completed.
    ///
    /// Defaults to unset, which lets the run take as long as it needs.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = "float (seconds)",
        example = r#"
            run-timeout = 1800.0
        "#
    )]
    pub run_timeout: Option<RunTimeoutSecs>,

    /// Grace period (in seconds) between graceful worker termination and
    /// force-kill.
    ///
    /// Karva uses this when stopping workers because of Ctrl+C, fail-fast, or
    /// `run-timeout`. Set to `0` to send the force-kill signal immediately
    /// after the graceful termination signal.
    ///
    /// Defaults to 10 seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"10.0"#,
        value_type = "float (seconds)",
        example = r#"
            termination-grace-period = 10.0
        "#
    )]
    pub termination_grace_period: Option<TerminationGracePeriodSecs>,
}

impl TestOptions {
    /// Applies defaults and converts second-based limits into platform durations.
    fn to_settings(&self) -> TestSettings {
        let max_fail = self
            .max_fail
            .or_else(|| self.fail_fast.map(MaxFail::from_fail_fast))
            .unwrap_or_default();

        TestSettings {
            test_function_prefix: self
                .test_function_prefix
                .clone()
                .unwrap_or_else(|| "test".to_string()),
            strict_tags: self.strict_tags.unwrap_or_default(),
            max_fail,
            try_import_fixtures: self.try_import_fixtures.unwrap_or_default(),
            doctest_modules: self.doctest_modules.unwrap_or_default(),
            retry: self.retry.unwrap_or_default(),
            shuffle: self.shuffle.unwrap_or_default(),
            random_seed: self.random_seed,
            flaky_result: self.flaky_result.unwrap_or_default(),
            filter: FiltersetSet::default(),
            run_ignored: RunIgnoredMode::default(),
            no_tests: self.no_tests.unwrap_or_default(),
            slow_timeout: self.slow_timeout.and_then(SlowTimeoutSecs::as_duration),
            fail_slow: self.fail_slow.and_then(FailSlowSecs::as_duration),
            timeout: self.timeout.and_then(TestTimeoutSecs::as_duration),
            run_timeout: self.run_timeout.and_then(RunTimeoutSecs::as_duration),
            termination_grace_period: self
                .termination_grace_period
                .and_then(TerminationGracePeriodSecs::as_duration),
        }
    }
}

#[derive(Debug, Default, Clone, Eq, PartialEq, Serialize, Deserialize, OptionsMetadata)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
/// Controls measured Python sources and coverage report generation.
pub struct CoverageOptions {
    /// Native coverage artifact read and written by coverage commands.
    ///
    /// Relative paths are resolved from the project root.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#".karva/coverage/data.json"#,
        value_type = r#"path"#,
        example = r#"
            data-file = ".karva/coverage/data.json"
        "#
    )]
    pub data_file: Option<String>,

    /// Ordered `FROM=TO` path mappings applied when native artifacts are read.
    ///
    /// Use aliases to relocate absolute sources collected outside the project
    /// or artifacts produced under a different CI checkout layout.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = r#"list[str]"#,
        example = r#"
            path-aliases = ["/workspace=.", "C:/repo=."]
        "#
    )]
    pub path_aliases: Option<Vec<String>>,

    /// Source paths or importable Python names to measure coverage for.
    ///
    /// Equivalent to passing `--cov=<source>` on the command line; may be
    /// listed multiple times. An empty entry (`""`) measures the current
    /// working directory, matching pytest-cov's bare `--cov`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = r#"list[str]"#,
        example = r#"
            sources = ["src"]
        "#
    )]
    pub sources: Option<Vec<String>>,

    /// Include only coverage report files matching these globs.
    ///
    /// Globs are matched against the project-relative file path shown in the
    /// coverage report, such as `src/package/module.py`. When unset, all files
    /// under the configured coverage sources are included unless omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = r#"list[str]"#,
        example = r#"
            include = ["src/**"]
        "#
    )]
    pub include: Option<Vec<String>>,

    /// Exclude coverage report files matching these globs.
    ///
    /// Globs are matched against the project-relative file path shown in the
    /// coverage report, such as `src/package/module.py`. Omit filters are
    /// applied after include filters.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = r#"list[str]"#,
        example = r#"
            omit = ["**/migrations/*"]
        "#
    )]
    pub omit: Option<Vec<String>>,

    /// Regular expressions excluding matching source lines or whole clauses.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = r#"list[str]"#,
        example = r#"
            exclude-lines = ["if TYPE_CHECKING:"]
        "#
    )]
    pub exclude_lines: Option<Vec<CoverageExcludePattern>>,

    /// Regular expressions marking intentionally partial branch lines.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = r#"list[str]"#,
        example = r#"
            partial-branches = ["if platform.system"]
        "#
    )]
    pub partial_branches: Option<Vec<CoveragePartialPattern>>,

    /// Static context component attached to every observation in the run.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = r#"str"#,
        example = r#"
            context = "python=3.14"
        "#
    )]
    pub context: Option<String>,

    /// Include execution attributed to contexts matching these regular expressions.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = r#"list[str]"#,
        example = r#"
            contexts = ["python=3\\.14", "test_checkout"]
        "#
    )]
    pub contexts: Option<Vec<String>>,

    /// Decimal places shown in coverage percentages.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"0"#,
        value_type = r#"non-negative integer"#,
        example = r#"
            precision = 2
        "#
    )]
    pub precision: Option<CoveragePrecision>,

    /// Add a test run to compatible native data instead of replacing it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"false"#,
        value_type = r#"true | false"#,
        example = r#"
            append = true
        "#
    )]
    pub append: Option<bool>,

    /// Coverage report type.
    ///
    /// `term` (default) prints a compact terminal table.
    /// `term-missing` extends it with a `Missing` column listing the
    /// uncovered line numbers per file. `none` persists native data only.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"term"#,
        value_type = r#"none | term | term-missing | xml | json | html | lcov"#,
        example = r#"
            report = "term-missing"
        "#
    )]
    pub report: Option<CovReport>,

    /// Optional output path for machine-readable coverage reports.
    ///
    /// For XML, JSON, and LCOV reports, this controls the output file. For HTML,
    /// it controls the output directory. If omitted, karva writes to
    /// `coverage.xml`, `coverage.json`, `coverage.lcov`, or `htmlcov/` in the
    /// project root.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = r#"path"#,
        example = r#"
            report-path = "build/coverage.xml"
        "#
    )]
    pub report_path: Option<String>,

    /// Whether to measure branch coverage in addition to line coverage.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"false"#,
        value_type = r#"true | false"#,
        example = r#"
            branch = true
        "#
    )]
    pub branch: Option<bool>,

    /// Minimum total coverage percentage required for the run to succeed.
    ///
    /// When set, the test command exits with a non-zero status if the
    /// reported `TOTAL` coverage is below this value, even when every test
    /// passed. Has no effect when tests already failed (the exit code is
    /// already non-zero).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = r#"float (0..=100)"#,
        example = r#"
            fail-under = 90
        "#
    )]
    pub fail_under: Option<CovFailUnder>,

    /// Set by `--no-cov` to disable coverage for a single run, overriding
    /// any sources configured in `karva.toml`.
    ///
    /// Not user-facing: skipped during (de)serialization so it cannot be
    /// set from a configuration file.
    #[serde(skip)]
    pub disabled: Option<bool>,
}

impl Combine for CoverageOptions {
    fn combine_with(&mut self, other: Self) {
        let report_overridden = self.report.is_some();

        self.data_file = self.data_file.take().combine(other.data_file);
        if self.path_aliases.is_none() {
            self.path_aliases = other.path_aliases;
        }
        self.sources = self.sources.take().combine(other.sources);
        self.include = self.include.take().combine(other.include);
        self.omit = self.omit.take().combine(other.omit);
        self.exclude_lines = self.exclude_lines.take().combine(other.exclude_lines);
        self.partial_branches = self.partial_branches.take().combine(other.partial_branches);
        self.context = self.context.take().combine(other.context);
        self.contexts = self.contexts.take().combine(other.contexts);
        self.precision = self.precision.combine(other.precision);
        self.append = self.append.combine(other.append);
        self.report = self.report.combine(other.report);
        self.report_path = if report_overridden && self.report_path.is_none() {
            None
        } else {
            self.report_path.take().combine(other.report_path)
        };
        self.branch = self.branch.combine(other.branch);
        self.fail_under = self.fail_under.combine(other.fail_under);
        self.disabled = self.disabled.combine(other.disabled);
    }
}

impl CoverageOptions {
    /// Applies defaults and honors runtime-only coverage disablement.
    fn to_settings(&self) -> CoverageSettings {
        let sources = if self.disabled.unwrap_or(false) {
            Vec::new()
        } else {
            self.sources.clone().unwrap_or_default()
        };
        CoverageSettings {
            data_file: self
                .data_file
                .clone()
                .unwrap_or_else(|| DEFAULT_COVERAGE_DATA_FILE.to_owned()),
            path_aliases: self.path_aliases.clone().unwrap_or_default(),
            sources,
            include: self.include.clone().unwrap_or_default(),
            omit: self.omit.clone().unwrap_or_default(),
            exclude_lines: self.exclude_lines.clone().unwrap_or_default(),
            partial_branches: self.partial_branches.clone().unwrap_or_default(),
            context: self.context.clone(),
            contexts: self.contexts.clone().unwrap_or_default(),
            precision: self.precision.unwrap_or_default(),
            append: self.append.unwrap_or_default(),
            report: self.report.unwrap_or_default(),
            report_path: self.report_path.clone(),
            branch: self.branch.unwrap_or_default(),
            fail_under: self.fail_under.map(|t| t.0),
        }
    }
}

#[derive(
    Debug, Default, Clone, Eq, PartialEq, Combine, Serialize, Deserialize, OptionsMetadata,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
/// Controls `JUnit` XML output, captured streams, and flaky-test representation.
pub struct JunitOptions {
    /// Output path for the `JUnit` XML report.
    ///
    /// When unset, no `JUnit` report is written.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"null"#,
        value_type = r#"path"#,
        example = r#"
            path = "reports/test-results.xml"
        "#
    )]
    pub path: Option<String>,

    /// Name of the top-level `JUnit` test suite collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#""karva-tests""#,
        value_type = r#"string"#,
        example = r#"
            report-name = "karva-tests"
        "#
    )]
    pub report_name: Option<String>,

    /// Whether to include captured stdout and stderr for passing tests.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"false"#,
        value_type = r#"true | false"#,
        example = r#"
            store-success-output = true
        "#
    )]
    pub store_success_output: Option<bool>,

    /// Whether to include captured stdout and stderr for failing tests.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"true"#,
        value_type = r#"true | false"#,
        example = r#"
            store-failure-output = true
        "#
    )]
    pub store_failure_output: Option<bool>,

    /// How flaky tests configured to fail are represented in `JUnit`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"failure"#,
        value_type = r#"failure | success"#,
        example = r#"
            flaky-fail-status = "success"
        "#
    )]
    pub flaky_fail_status: Option<JunitFlakyFailStatus>,
}

impl JunitOptions {
    /// Applies `JUnit` defaults and produces report settings.
    fn to_settings(&self) -> JunitSettings {
        JunitSettings {
            path: self.path.clone(),
            report_name: self
                .report_name
                .clone()
                .unwrap_or_else(|| "karva-tests".to_string()),
            store_success_output: self.store_success_output.unwrap_or_default(),
            store_failure_output: self.store_failure_output.unwrap_or(true),
            flaky_fail_status: self.flaky_fail_status.unwrap_or_default(),
        }
    }
}

/// Coverage report type.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum CovReport {
    /// Persist native data without rendering a report.
    None,

    /// Compact terminal table (default).
    #[default]
    Term,

    /// Terminal table with a `Missing` column listing uncovered line numbers.
    TermMissing,

    /// Cobertura XML written to disk for CI integrations.
    Xml,

    /// JSON written to disk for machine-readable coverage consumption.
    Json,

    /// HTML written to disk for interactive browsing.
    Html,

    /// LCOV tracefile written to disk for external tooling.
    Lcov,
}

impl Combine for CovReport {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

impl CovReport {
    /// Returns canonical configuration spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Term => "term",
            Self::TermMissing => "term-missing",
            Self::Xml => "xml",
            Self::Json => "json",
            Self::Html => "html",
            Self::Lcov => "lcov",
        }
    }
}

/// The diagnostic output format.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum OutputFormat {
    /// Multi-line diagnostics with source context and hints.
    #[default]
    Full,

    /// One diagnostic per line.
    Concise,
}

impl OutputFormat {
    /// Returns canonical configuration spelling.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Concise => "concise",
        }
    }
}

impl Combine for OutputFormat {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use insta::{assert_debug_snapshot, assert_snapshot};
    use karva_combine::Combine;
    use rstest::rstest;

    use super::*;

    #[test]
    fn to_settings_fail_fast_true_becomes_max_fail_one() {
        let options = TestOptions {
            fail_fast: Some(true),
            ..TestOptions::default()
        };
        assert_debug_snapshot!(options.to_settings().max_fail, @"
        MaxFail(
            Some(
                1,
            ),
        )
        ");
    }

    #[test]
    fn to_settings_fail_fast_false_is_unlimited() {
        let options = TestOptions {
            fail_fast: Some(false),
            ..TestOptions::default()
        };
        assert_debug_snapshot!(options.to_settings().max_fail, @"
        MaxFail(
            None,
        )
        ");
    }

    #[test]
    fn to_settings_max_fail_takes_precedence_over_fail_fast() {
        let options = TestOptions {
            fail_fast: Some(true),
            max_fail: Some(MaxFail::from(NonZeroU32::new(5).expect("non-zero"))),
            ..TestOptions::default()
        };
        assert_debug_snapshot!(options.to_settings().max_fail, @"
        MaxFail(
            Some(
                5,
            ),
        )
        ");
    }

    #[test]
    fn to_settings_doctest_modules_defaults_to_disabled() {
        assert!(!TestOptions::default().to_settings().doctest_modules);
    }

    #[test]
    fn parse_profile_doctest_modules() {
        let config = Config::from_toml_str("[profile.default.test]\ndoctest-modules = true\n")
            .expect("parse");

        assert_eq!(
            config
                .resolve_profile(None)
                .expect("resolve")
                .test
                .expect("test options")
                .doctest_modules,
            Some(true)
        );
    }

    #[test]
    fn from_toml_str_rejects_unknown_key() {
        let toml = r"
[profile.default.test]
fail-fast = true
nonsense = 42
";
        assert_snapshot!(
            Config::from_toml_str(toml).expect_err("unknown field"),
            @"
        TOML parse error at line 4, column 1
          |
        4 | nonsense = 42
          | ^^^^^^^^
        unknown field `nonsense`
        "
        );
    }

    #[test]
    fn from_toml_str_rejects_unknown_top_level_section() {
        let toml = r"
[bogus]
foo = 1
";
        assert_snapshot!(
            Config::from_toml_str(toml).expect_err("unknown section"),
            @"
        TOML parse error at line 2, column 2
          |
        2 | [bogus]
          |  ^^^^^
        unknown field `bogus`
        "
        );
    }

    #[test]
    fn from_toml_str_rejects_top_level_option_groups() {
        let toml = r#"
[test]
test-function-prefix = "test"
"#;
        assert_snapshot!(
            Config::from_toml_str(toml).expect_err("top-level rejected"),
            @"
        TOML parse error at line 2, column 2
          |
        2 | [test]
          |  ^^^^
        unknown field `test`
        "
        );
    }

    #[test]
    fn from_toml_str_empty_is_default() {
        assert_eq!(Config::from_toml_str("").expect("parse"), Config::default());
    }

    /// `MaxFail` wraps `NonZeroU32`, so raw `0` must be rejected by the
    /// deserializer rather than silently producing `unlimited`.
    #[test]
    fn from_toml_str_rejects_max_fail_zero() {
        let toml = r"
[profile.default.test]
max-fail = 0
";
        assert_snapshot!(
            Config::from_toml_str(toml).expect_err("zero rejected"),
            @"
        TOML parse error at line 3, column 12
          |
        3 | max-fail = 0
          |            ^
        invalid value: integer `0`, expected a nonzero u32
        "
        );
    }

    #[test]
    fn combine_prefers_self_for_scalars() {
        let cli = TestOptions {
            test_function_prefix: Some("cli_prefix".to_string()),
            retry: Some(5),
            ..TestOptions::default()
        };
        let file = TestOptions {
            test_function_prefix: Some("file_prefix".to_string()),
            retry: Some(1),
            try_import_fixtures: Some(true),
            ..TestOptions::default()
        };
        assert_eq!(
            cli.combine(file),
            TestOptions {
                test_function_prefix: Some("cli_prefix".to_string()),
                try_import_fixtures: Some(true),
                retry: Some(5),
                ..TestOptions::default()
            }
        );
    }

    #[test]
    fn combine_fills_missing_fields_from_other() {
        let cli = TestOptions::default();
        let file = TestOptions {
            test_function_prefix: Some("from_file".to_string()),
            fail_fast: Some(true),
            retry: Some(3),
            ..TestOptions::default()
        };
        assert_eq!(
            cli.combine(file),
            TestOptions {
                test_function_prefix: Some("from_file".to_string()),
                fail_fast: Some(true),
                retry: Some(3),
                ..TestOptions::default()
            }
        );
    }

    /// `Vec::combine` appends `self` after `other`, so CLI entries take
    /// precedence at the tail.
    #[test]
    fn combine_merges_include_paths_with_cli_taking_precedence() {
        let cli = SrcOptions {
            include: Some(vec!["cli_only".to_string()]),
            ..SrcOptions::default()
        };
        let file = SrcOptions {
            include: Some(vec!["file_only".to_string()]),
            respect_ignore_files: Some(false),
        };
        assert_eq!(
            cli.combine(file),
            SrcOptions {
                respect_ignore_files: Some(false),
                include: Some(vec!["file_only".to_string(), "cli_only".to_string()]),
            }
        );
    }

    #[test]
    fn project_overrides_apply_cli_over_file() {
        let cli_options = Options {
            test: Some(TestOptions {
                test_function_prefix: Some("cli".to_string()),
                ..TestOptions::default()
            }),
            ..Options::default()
        };
        let toml = r#"
[profile.default.test]
test-function-prefix = "file"
retry = 2
"#;
        let config = Config::from_toml_str(toml).expect("parse");
        let overrides = ProjectOptionsOverrides::new(None, cli_options);
        assert_eq!(
            overrides.apply_to(config).expect("resolves").test,
            Some(TestOptions {
                test_function_prefix: Some("cli".to_string()),
                retry: Some(2),
                ..TestOptions::default()
            })
        );
    }

    #[test]
    fn parse_profile_section() {
        let toml = r#"
[profile.default.test]
test-function-prefix = "test"

[profile.ci.test]
retry = 5
no-tests = "fail"

[profile.ci.terminal]
output-format = "concise"
"#;
        let config = Config::from_toml_str(toml).expect("parse");
        assert_debug_snapshot!(config.has_profile("ci"), @"true");
        assert_debug_snapshot!(config.has_profile("default"), @"true");
        assert_debug_snapshot!(config.has_profile("missing"), @"false");
    }

    #[test]
    fn resolve_profile_layers_named_over_default() {
        let toml = r#"
[profile.default.test]
test-function-prefix = "base"
retry = 2
fail-fast = true

[profile.ci.test]
retry = 5
"#;
        let resolved = Config::from_toml_str(toml)
            .expect("parse")
            .resolve_profile(Some("ci"))
            .expect("resolves");
        assert_eq!(
            resolved.test,
            Some(TestOptions {
                test_function_prefix: Some("base".to_string()),
                fail_fast: Some(true),
                retry: Some(5),
                ..TestOptions::default()
            })
        );
    }

    #[test]
    fn resolve_default_profile_applies_default_overrides() {
        let toml = r"
[profile.default.test]
retry = 9
";
        let resolved = Config::from_toml_str(toml)
            .expect("parse")
            .resolve_profile(None)
            .expect("resolves");
        assert_debug_snapshot!(resolved.test.unwrap().retry, @r"
        Some(
            9,
        )
        ");
    }

    #[test]
    fn resolve_profile_missing_profile_errors() {
        let toml = r"
[profile.ci.test]
retry = 5
";
        let err = Config::from_toml_str(toml)
            .expect("parse")
            .resolve_profile(Some("nope"))
            .expect_err("unknown");
        assert_snapshot!(
            err,
            @"profile `nope` is not defined in configuration (available: ci, default)"
        );
    }

    #[test]
    fn resolve_default_profile_when_empty_config_is_ok() {
        let config = Config::default();
        assert!(config.resolve_profile(None).is_ok());
    }

    #[test]
    fn resolve_non_default_profile_when_empty_config_errors() {
        let config = Config::default();
        let err = config.resolve_profile(Some("ci")).expect_err("unknown");
        assert_snapshot!(
            err,
            @"profile `ci` is not defined in configuration (available: default)"
        );
    }

    #[test]
    fn from_toml_str_rejects_reserved_default_prefix() {
        let toml = r"
[profile.default-ci.test]
retry = 1
";
        assert_snapshot!(
            Config::from_toml_str(toml).expect_err("reserved"),
            @"invalid profile name `default-ci`: the `default-` prefix is reserved for built-in profiles"
        );
    }

    #[test]
    fn from_toml_str_rejects_invalid_profile_name_chars() {
        let toml = r#"
[profile."ci/fast".test]
retry = 1
"#;
        assert_snapshot!(
            Config::from_toml_str(toml).expect_err("invalid"),
            @"invalid profile name `ci/fast`: profile names may only contain ASCII letters, digits, `-`, and `_`"
        );
    }

    #[test]
    fn cli_overrides_win_over_resolved_profile() {
        let cli_options = Options {
            test: Some(TestOptions {
                retry: Some(99),
                ..TestOptions::default()
            }),
            ..Options::default()
        };
        let toml = r"
[profile.ci.test]
retry = 5
";
        let config = Config::from_toml_str(toml).expect("parse");
        let overrides =
            ProjectOptionsOverrides::new(None, cli_options).with_profile(Some("ci".to_string()));
        let resolved = overrides.apply_to(config).expect("resolves");
        assert_debug_snapshot!(resolved.test.unwrap().retry, @r"
        Some(
            99,
        )
        ");
    }

    #[test]
    fn parse_coverage_section() {
        let toml = r#"
[profile.default.coverage]
data-file = "build/coverage-data.json"
path-aliases = ["/workspace=."]
sources = ["src", "tests"]
include = ["src/**"]
omit = ["**/generated.py"]
context = "python=3.14"
contexts = ["python=3\\.14"]
precision = 2
report = "term-missing"
report-path = "build/coverage.xml"
"#;
        let resolved = Config::from_toml_str(toml)
            .expect("parse")
            .resolve_profile(None)
            .expect("resolves");
        assert_eq!(
            resolved.coverage,
            Some(CoverageOptions {
                data_file: Some("build/coverage-data.json".to_string()),
                path_aliases: Some(vec!["/workspace=.".to_string()]),
                sources: Some(vec!["src".to_string(), "tests".to_string()]),
                include: Some(vec!["src/**".to_string()]),
                omit: Some(vec!["**/generated.py".to_string()]),
                context: Some("python=3.14".to_string()),
                contexts: Some(vec![r"python=3\.14".to_string()]),
                precision: Some(CoveragePrecision(2)),
                report: Some(CovReport::TermMissing),
                report_path: Some("build/coverage.xml".to_string()),
                ..CoverageOptions::default()
            })
        );
    }

    #[test]
    fn coverage_precision_rejects_unrepresentable_digits() {
        let requested = CoveragePrecision::MAX + 1;
        let toml = format!("[profile.default.coverage]\nprecision = {requested}\n");

        let error = Config::from_toml_str(&toml).expect_err("reject excessive precision");
        let message = error.to_string();

        assert!(message.contains(&requested.to_string()));
        assert!(message.contains(&CoveragePrecision::MAX.to_string()));
    }

    #[test]
    fn parse_junit_section() {
        let toml = r#"
[profile.ci.junit]
path = "reports/test-results.xml"
report-name = "karva-ci"
store-success-output = true
store-failure-output = false
"#;
        let resolved = Config::from_toml_str(toml)
            .expect("parse")
            .resolve_profile(Some("ci"))
            .expect("resolves");
        assert_eq!(
            resolved.junit,
            Some(JunitOptions {
                path: Some("reports/test-results.xml".to_string()),
                report_name: Some("karva-ci".to_string()),
                store_success_output: Some(true),
                store_failure_output: Some(false),
                ..JunitOptions::default()
            })
        );
    }

    /// CLI `--cov` sources accumulate with file sources at the tail (matching
    /// the existing `include` behavior).
    #[test]
    fn combine_appends_cli_coverage_sources_after_file() {
        let cli = CoverageOptions {
            sources: Some(vec!["tests".to_string()]),
            ..CoverageOptions::default()
        };
        let file = CoverageOptions {
            sources: Some(vec!["src".to_string()]),
            report: Some(CovReport::TermMissing),
            ..CoverageOptions::default()
        };
        assert_eq!(
            cli.combine(file),
            CoverageOptions {
                sources: Some(vec!["src".to_string(), "tests".to_string()]),
                report: Some(CovReport::TermMissing),
                ..CoverageOptions::default()
            }
        );
    }

    /// CLI filters accumulate with file filters, matching coverage sources.
    #[test]
    fn combine_appends_cli_coverage_filters_after_file() {
        let cli = CoverageOptions {
            include: Some(vec!["tests/**".to_string()]),
            omit: Some(vec!["**/generated.py".to_string()]),
            ..CoverageOptions::default()
        };
        let file = CoverageOptions {
            include: Some(vec!["src/**".to_string()]),
            omit: Some(vec!["**/migrations/*".to_string()]),
            ..CoverageOptions::default()
        };
        assert_eq!(
            cli.combine(file),
            CoverageOptions {
                include: Some(vec!["src/**".to_string(), "tests/**".to_string()]),
                omit: Some(vec![
                    "**/migrations/*".to_string(),
                    "**/generated.py".to_string(),
                ]),
                ..CoverageOptions::default()
            }
        );
    }

    /// CLI `--cov-report` overrides the configured value (scalar `Combine`).
    #[test]
    fn combine_cli_coverage_report_wins_over_file() {
        let cli = CoverageOptions {
            report: Some(CovReport::Term),
            ..CoverageOptions::default()
        };
        let file = CoverageOptions {
            report: Some(CovReport::TermMissing),
            ..CoverageOptions::default()
        };
        assert_debug_snapshot!(cli.combine(file).report, @r"
        Some(
            Term,
        )
        ");
    }

    #[test]
    fn combine_cli_coverage_report_path_wins_over_file() {
        let cli = CoverageOptions {
            report_path: Some("cli.xml".to_string()),
            ..CoverageOptions::default()
        };
        let file = CoverageOptions {
            report_path: Some("config.xml".to_string()),
            ..CoverageOptions::default()
        };
        assert_debug_snapshot!(cli.combine(file).report_path, @r#"
        Some(
            "cli.xml",
        )
        "#);
    }

    #[test]
    fn combine_cli_coverage_report_without_path_clears_file_path() {
        let cli = CoverageOptions {
            report: Some(CovReport::Json),
            ..CoverageOptions::default()
        };
        let file = CoverageOptions {
            report: Some(CovReport::Xml),
            report_path: Some("build/coverage.xml".to_string()),
            ..CoverageOptions::default()
        };
        assert_eq!(
            cli.combine(file),
            CoverageOptions {
                report: Some(CovReport::Json),
                ..CoverageOptions::default()
            }
        );
    }

    /// `--no-cov` (CLI sets `disabled = Some(true)`) overrides any sources
    /// configured in `karva.toml`.
    #[test]
    fn to_settings_disabled_clears_configured_sources() {
        let cli = CoverageOptions {
            disabled: Some(true),
            ..CoverageOptions::default()
        };
        let file = CoverageOptions {
            sources: Some(vec!["src".to_string()]),
            ..CoverageOptions::default()
        };
        let combined = cli.combine(file);
        assert_debug_snapshot!(combined.to_settings().sources, @"[]");
    }

    /// `disabled` is CLI-only; `deny_unknown_fields` should reject it from TOML.
    #[test]
    fn from_toml_str_rejects_disabled_key() {
        let toml = r"
[profile.default.coverage]
disabled = true
";
        assert_snapshot!(
            Config::from_toml_str(toml).expect_err("unknown field"),
            @"
        TOML parse error at line 3, column 1
          |
        3 | disabled = true
          | ^^^^^^^^
        unknown field `disabled`
        "
        );
    }

    #[test]
    fn from_toml_str_rejects_unknown_coverage_key() {
        let toml = r#"
[profile.default.coverage]
sources = ["src"]
nonsense = 1
"#;
        assert_snapshot!(
            Config::from_toml_str(toml).expect_err("unknown field"),
            @"
        TOML parse error at line 4, column 1
          |
        4 | nonsense = 1
          | ^^^^^^^^
        unknown field `nonsense`
        "
        );
    }

    #[test]
    fn parse_overrides_section() {
        let toml = r#"
[[profile.default.overrides]]
filter = "tag(network)"
retries = 5

[[profile.default.overrides]]
filter = "tag(unit)"
retries = 0
"#;
        let resolved = Config::from_toml_str(toml)
            .expect("parse")
            .resolve_profile(None)
            .expect("resolves");
        let overrides = resolved.overrides;
        assert_eq!(overrides.len(), 2);
        assert_eq!(overrides[0].filter.as_str(), "tag(network)");
        assert_eq!(overrides[0].retries, Some(5));
        assert_eq!(overrides[1].filter.as_str(), "tag(unit)");
        assert_eq!(overrides[1].retries, Some(0));
    }

    /// Named profile entries layer on top of the default profile's
    /// overrides — both lists end up in the resolved options.
    #[test]
    fn resolve_profile_appends_named_overrides_on_top_of_default() {
        let toml = r#"
[[profile.default.overrides]]
filter = "tag(network)"
retries = 3

[[profile.ci.overrides]]
filter = "tag(slow)"
retries = 1
"#;
        let resolved = Config::from_toml_str(toml)
            .expect("parse")
            .resolve_profile(Some("ci"))
            .expect("resolves");
        let raw: Vec<&str> = resolved
            .overrides
            .iter()
            .map(|o| o.filter.as_str())
            .collect();
        assert_eq!(raw, vec!["tag(slow)", "tag(network)"]);
    }

    #[test]
    fn from_toml_str_rejects_invalid_override_filter() {
        let toml = r#"
[[profile.default.overrides]]
filter = "tag("
retries = 1
"#;
        let err = Config::from_toml_str(toml).expect_err("invalid filter");
        assert!(
            err.to_string().contains("expected a matcher body"),
            "expected filter parse error in: {err}"
        );
    }

    #[test]
    fn to_settings_compiles_overrides() {
        let toml = r#"
[[profile.default.overrides]]
filter = "tag(network)"
retries = 5
"#;
        let resolved = Config::from_toml_str(toml)
            .expect("parse")
            .resolve_profile(None)
            .expect("resolves");
        let settings = resolved.to_settings();
        let overrides = settings.overrides();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].retries, Some(5));
        let ctx = crate::filter::EvalContext {
            test_name: "test::foo",
            tags: &["network"],
        };
        assert!(overrides[0].matches(&ctx));
        let other = crate::filter::EvalContext {
            test_name: "test::bar",
            tags: &["unit"],
        };
        assert!(!overrides[0].matches(&other));
    }

    #[test]
    fn retry_for_picks_first_matching_override() {
        let toml = r#"
[profile.default.test]
retry = 1

[[profile.default.overrides]]
filter = "tag(network)"
retries = 5

[[profile.default.overrides]]
filter = "tag(unit)"
retries = 0
"#;
        let resolved = Config::from_toml_str(toml)
            .expect("parse")
            .resolve_profile(None)
            .expect("resolves");
        let settings = resolved.to_settings();
        let net = crate::filter::EvalContext {
            test_name: "test::a",
            tags: &["network"],
        };
        let unit = crate::filter::EvalContext {
            test_name: "test::b",
            tags: &["unit"],
        };
        let other = crate::filter::EvalContext {
            test_name: "test::c",
            tags: &[],
        };
        assert_eq!(settings.retry_for(&net), 5);
        assert_eq!(settings.retry_for(&unit), 0);
        assert_eq!(settings.retry_for(&other), 1);
    }

    #[test]
    fn may_retry_covers_profile_and_positive_overrides() {
        let no_retry = Config::default()
            .resolve_profile(None)
            .expect("default profile resolves")
            .to_settings();
        assert!(!no_retry.may_retry());

        let profile_retry = Config::from_toml_str("[profile.default.test]\nretry = 1\n")
            .expect("profile retry parses")
            .resolve_profile(None)
            .expect("default profile resolves")
            .to_settings();
        assert!(profile_retry.may_retry());

        let override_retry = Config::from_toml_str(
            "[[profile.default.overrides]]\nfilter = \"tag(retry)\"\nretries = 1\n",
        )
        .expect("override retry parses")
        .resolve_profile(None)
        .expect("default profile resolves")
        .to_settings();
        assert!(override_retry.may_retry());

        let disabled_override = Config::from_toml_str(
            "[[profile.default.overrides]]\nfilter = \"tag(retry)\"\nretries = 0\n",
        )
        .expect("disabled override parses")
        .resolve_profile(None)
        .expect("default profile resolves")
        .to_settings();
        assert!(!disabled_override.may_retry());
    }

    #[test]
    fn timeout_for_picks_first_matching_override() {
        let toml = r#"
[profile.default.test]
timeout = 30.0

[[profile.default.overrides]]
filter = "tag(slow)"
timeout = 300.0

[[profile.default.overrides]]
filter = "tag(unit)"
timeout = 0
"#;
        let resolved = Config::from_toml_str(toml)
            .expect("parse")
            .resolve_profile(None)
            .expect("resolves");
        let settings = resolved.to_settings();
        let slow = crate::filter::EvalContext {
            test_name: "test::a",
            tags: &["slow"],
        };
        let unit = crate::filter::EvalContext {
            test_name: "test::b",
            tags: &["unit"],
        };
        let other = crate::filter::EvalContext {
            test_name: "test::c",
            tags: &[],
        };
        assert_eq!(
            settings.timeout_for(&slow),
            Some(std::time::Duration::from_mins(5))
        );
        // `timeout = 0` on a matching override disables the hard limit.
        assert_eq!(settings.timeout_for(&unit), None);
        assert_eq!(
            settings.timeout_for(&other),
            Some(std::time::Duration::from_secs(30))
        );
    }

    #[test]
    fn slow_timeout_for_picks_first_matching_override() {
        let toml = r#"
[profile.default.test]
slow-timeout = 1.0

[[profile.default.overrides]]
filter = "tag(integration)"
slow-timeout = 30.0
"#;
        let resolved = Config::from_toml_str(toml)
            .expect("parse")
            .resolve_profile(None)
            .expect("resolves");
        let settings = resolved.to_settings();
        let integration = crate::filter::EvalContext {
            test_name: "test::a",
            tags: &["integration"],
        };
        let other = crate::filter::EvalContext {
            test_name: "test::b",
            tags: &[],
        };
        assert_eq!(
            settings.slow_timeout_for(&integration),
            Some(std::time::Duration::from_secs(30))
        );
        assert_eq!(
            settings.slow_timeout_for(&other),
            Some(std::time::Duration::from_secs(1))
        );
    }

    #[test]
    fn fail_slow_for_picks_first_matching_override() {
        let toml = r#"
[profile.default.test]
fail-slow = 1.0

[[profile.default.overrides]]
filter = "tag(slow)"
fail-slow = 30.0

[[profile.default.overrides]]
filter = "tag(unit)"
fail-slow = 0
"#;
        let resolved = Config::from_toml_str(toml)
            .expect("parse")
            .resolve_profile(None)
            .expect("resolves");
        let settings = resolved.to_settings();
        let slow = crate::filter::EvalContext {
            test_name: "test::a",
            tags: &["slow"],
        };
        let unit = crate::filter::EvalContext {
            test_name: "test::b",
            tags: &["unit"],
        };
        let other = crate::filter::EvalContext {
            test_name: "test::c",
            tags: &[],
        };
        assert_eq!(
            settings.fail_slow_for(&slow),
            Some(std::time::Duration::from_secs(30))
        );
        // `fail-slow = 0` on a matching override disables the budget.
        assert_eq!(settings.fail_slow_for(&unit), None);
        assert_eq!(
            settings.fail_slow_for(&other),
            Some(std::time::Duration::from_secs(1))
        );
    }

    #[test]
    fn fail_slow_rejects_unrepresentable_config_duration() {
        for value in ["1e-300", "1e300"] {
            let profile = format!(
                r"
[profile.default.test]
fail-slow = {value}
"
            );
            assert!(Config::from_toml_str(&profile).is_err());

            let override_value = format!(
                r#"
[[profile.default.overrides]]
filter = "tag(slow)"
fail-slow = {value}
"#
            );
            assert!(Config::from_toml_str(&override_value).is_err());
        }
    }

    #[rstest]
    fn timeout_rejects_invalid_config_duration(
        #[values("slow-timeout", "timeout", "run-timeout", "termination-grace-period")]
        option: &str,
        #[values("nan", "inf", "1e300")] value: &str,
    ) {
        let toml = format!(
            r"
[profile.default.test]
{option} = {value}
"
        );
        let error = Config::from_toml_str(&toml).expect_err("invalid duration");
        assert!(
            error.to_string().contains(&format!(
                "{option} must be a finite duration supported by this platform"
            )),
            "unexpected error: {error}"
        );
    }
}
