mod config;
mod overrides;

use karva_combine::Combine;
use karva_logging::{FinalStatusLevel, ProgressMode, StatusLevel};
use karva_macros::{Combine, OptionsMetadata};
use ruff_db::diagnostic::DiagnosticFormat;
use serde::{Deserialize, Serialize};

pub use config::{
    Config, DEFAULT_PROFILE, IncompatibleVersionError, KarvaTomlError, UnknownProfile,
};
pub use overrides::ProjectOptionsOverrides;

use crate::filter::FiltersetSet;
use crate::max_fail::MaxFail;
use crate::settings::{
    CovFailUnder, CoverageSettings, NoTestsMode, ProjectSettings, RunIgnoredMode, SlowTimeoutSecs,
    SrcSettings, TerminalSettings, TestSettings, TestTimeoutSecs,
};

#[derive(
    Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, OptionsMetadata, Combine,
)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Options {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option_group]
    pub src: Option<SrcOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option_group]
    pub terminal: Option<TerminalOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option_group]
    pub test: Option<TestOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option_group]
    pub coverage: Option<CoverageOptions>,
}

impl Options {
    pub fn to_settings(&self) -> ProjectSettings {
        ProjectSettings {
            terminal: self.terminal.clone().unwrap_or_default().to_settings(),
            src: self.src.clone().unwrap_or_default().to_settings(),
            test: self.test.clone().unwrap_or_default().to_settings(),
            coverage: self.coverage.clone().unwrap_or_default().to_settings(),
        }
    }
}

#[derive(
    Debug, Default, Clone, Eq, PartialEq, Serialize, Deserialize, OptionsMetadata, Combine,
)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
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

    /// A list of files and directories to check.
    /// Including a file or directory will make it so that it (and its contents)
    /// are tested.
    ///
    /// - `tests` matches a directory named `tests`
    /// - `tests/test.py` matches a file named `test.py` in the `tests` directory
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
    pub(crate) fn to_settings(&self) -> SrcSettings {
        SrcSettings {
            respect_ignore_files: self.respect_ignore_files.unwrap_or(true),
            include_paths: self.include.clone().unwrap_or_default(),
        }
    }
}

#[derive(
    Debug, Default, Clone, Eq, PartialEq, Combine, Serialize, Deserialize, OptionsMetadata,
)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
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

    /// Live progress display rendered while tests run.
    ///
    /// `none` (the default) leaves output untouched. `counter` prints a
    /// `N/M tests` line on stderr that refreshes periodically. `bar` shows
    /// a visual progress bar with completion stats. The display is rendered
    /// on stderr so it does not interfere with per-test result lines on
    /// stdout (gated by [`status_level`](#status-level)).
    ///
    /// Defaults to `none`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"none"#,
        value_type = "none | counter | bar",
        example = r#"
            show-progress = "bar"
        "#
    )]
    pub show_progress: Option<ProgressMode>,
}

impl TerminalOptions {
    pub fn to_settings(&self) -> TerminalSettings {
        TerminalSettings {
            output_format: self.output_format.unwrap_or_default(),
            show_python_output: self.show_python_output.unwrap_or_default(),
            status_level: self.status_level.unwrap_or_default(),
            final_status_level: self.final_status_level.unwrap_or_default(),
            show_progress: self.show_progress.unwrap_or_default(),
        }
    }
}

#[derive(
    Debug, Default, Clone, Eq, PartialEq, Combine, Serialize, Deserialize, OptionsMetadata,
)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
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
}

impl TestOptions {
    pub fn to_settings(&self) -> TestSettings {
        let max_fail = self
            .max_fail
            .or_else(|| self.fail_fast.map(MaxFail::from_fail_fast))
            .unwrap_or_default();

        TestSettings {
            test_function_prefix: self
                .test_function_prefix
                .clone()
                .unwrap_or_else(|| "test".to_string()),
            max_fail,
            try_import_fixtures: self.try_import_fixtures.unwrap_or_default(),
            retry: self.retry.unwrap_or_default(),
            filter: FiltersetSet::default(),
            run_ignored: RunIgnoredMode::default(),
            no_tests: self.no_tests.unwrap_or_default(),
            slow_timeout: self.slow_timeout.and_then(SlowTimeoutSecs::as_duration),
            timeout: self.timeout.and_then(TestTimeoutSecs::as_duration),
        }
    }
}

#[derive(
    Debug, Default, Clone, Eq, PartialEq, Combine, Serialize, Deserialize, OptionsMetadata,
)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CoverageOptions {
    /// Source paths to measure coverage for.
    ///
    /// Equivalent to passing `--cov=<path>` on the command line; may be
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

    /// Coverage terminal report type.
    ///
    /// `term` (default) prints a compact terminal table.
    /// `term-missing` extends it with a `Missing` column listing the
    /// uncovered line numbers per file.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[option(
        default = r#"term"#,
        value_type = "term | term-missing",
        example = r#"
            report = "term-missing"
        "#
    )]
    pub report: Option<CovReport>,

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

impl CoverageOptions {
    pub fn to_settings(&self) -> CoverageSettings {
        let sources = if self.disabled.unwrap_or(false) {
            Vec::new()
        } else {
            self.sources.clone().unwrap_or_default()
        };
        CoverageSettings {
            sources,
            report: self.report.unwrap_or_default(),
            fail_under: self.fail_under.map(|t| t.0),
        }
    }
}

/// Coverage terminal report type.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum CovReport {
    /// Compact terminal table (default).
    #[default]
    Term,

    /// Terminal table with a `Missing` column listing uncovered line numbers.
    TermMissing,
}

impl Combine for CovReport {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

/// The diagnostic output format.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum OutputFormat {
    #[default]
    Full,

    Concise,
}

impl OutputFormat {
    /// Returns `true` if this format is intended for users to read directly, in contrast to
    /// machine-readable or structured formats.
    ///
    /// This can be used to check whether information beyond the diagnostics, such as a header or
    /// `Found N diagnostics` footer, should be included.
    pub fn is_human_readable(self) -> bool {
        matches!(self, Self::Full | Self::Concise)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Concise => "concise",
        }
    }
}

impl From<OutputFormat> for DiagnosticFormat {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Full => Self::Full,
            OutputFormat::Concise => Self::Concise,
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
        unknown field `nonsense`, expected one of `test-function-prefix`, `fail-fast`, `max-fail`, `try-import-fixtures`, `retry`, `no-tests`, `slow-timeout`, `timeout`
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
        unknown field `bogus`, expected `required-version` or `profile`
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
        unknown field `test`, expected `required-version` or `profile`
        "
        );
    }

    #[test]
    fn from_toml_str_empty_is_default() {
        assert_debug_snapshot!(Config::from_toml_str("").expect("parse"), @"
        Config {
            required_version: None,
            profile: {},
        }
        ");
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
        assert_debug_snapshot!(cli.combine(file), @r#"
        TestOptions {
            test_function_prefix: Some(
                "cli_prefix",
            ),
            fail_fast: None,
            max_fail: None,
            try_import_fixtures: Some(
                true,
            ),
            retry: Some(
                5,
            ),
            no_tests: None,
            slow_timeout: None,
            timeout: None,
        }
        "#);
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
        assert_debug_snapshot!(cli.combine(file), @r#"
        TestOptions {
            test_function_prefix: Some(
                "from_file",
            ),
            fail_fast: Some(
                true,
            ),
            max_fail: None,
            try_import_fixtures: None,
            retry: Some(
                3,
            ),
            no_tests: None,
            slow_timeout: None,
            timeout: None,
        }
        "#);
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
        assert_debug_snapshot!(cli.combine(file), @r#"
        SrcOptions {
            respect_ignore_files: Some(
                false,
            ),
            include: Some(
                [
                    "file_only",
                    "cli_only",
                ],
            ),
        }
        "#);
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
        assert_debug_snapshot!(overrides.apply_to(config).expect("resolves").test, @r#"
        Some(
            TestOptions {
                test_function_prefix: Some(
                    "cli",
                ),
                fail_fast: None,
                max_fail: None,
                try_import_fixtures: None,
                retry: Some(
                    2,
                ),
                no_tests: None,
                slow_timeout: None,
                timeout: None,
            },
        )
        "#);
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
        assert_debug_snapshot!(resolved.test, @r#"
        Some(
            TestOptions {
                test_function_prefix: Some(
                    "base",
                ),
                fail_fast: Some(
                    true,
                ),
                max_fail: None,
                try_import_fixtures: None,
                retry: Some(
                    5,
                ),
                no_tests: None,
                slow_timeout: None,
                timeout: None,
            },
        )
        "#);
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
sources = ["src", "tests"]
report = "term-missing"
"#;
        let resolved = Config::from_toml_str(toml)
            .expect("parse")
            .resolve_profile(None)
            .expect("resolves");
        assert_debug_snapshot!(resolved.coverage, @r#"
        Some(
            CoverageOptions {
                sources: Some(
                    [
                        "src",
                        "tests",
                    ],
                ),
                report: Some(
                    TermMissing,
                ),
                fail_under: None,
                disabled: None,
            },
        )
        "#);
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
        assert_debug_snapshot!(cli.combine(file), @r#"
        CoverageOptions {
            sources: Some(
                [
                    "src",
                    "tests",
                ],
            ),
            report: Some(
                TermMissing,
            ),
            fail_under: None,
            disabled: None,
        }
        "#);
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
            @r"
        TOML parse error at line 3, column 1
          |
        3 | disabled = true
          | ^^^^^^^^
        unknown field `disabled`, expected one of `sources`, `report`, `fail-under`
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
            @r"
        TOML parse error at line 4, column 1
          |
        4 | nonsense = 1
          | ^^^^^^^^
        unknown field `nonsense`, expected one of `sources`, `report`, `fail-under`
        "
        );
    }
}
