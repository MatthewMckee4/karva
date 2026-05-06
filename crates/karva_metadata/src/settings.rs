use std::time::Duration;

use karva_combine::Combine;
use karva_logging::{FinalStatusLevel, StatusLevel};
use serde::{Deserialize, Serialize};

use crate::filter::FiltersetSet;
use crate::max_fail::MaxFail;
use crate::options::{
    CovReport, CoverageOptions, Options, OutputFormat, SrcOptions, TerminalOptions, TestOptions,
};

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunIgnoredMode {
    #[default]
    Default,
    Only,
    All,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum NoTestsMode {
    #[default]
    Auto,
    Pass,
    Warn,
    Fail,
}

impl Combine for NoTestsMode {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

/// A slow-test threshold expressed in seconds.
///
/// Wraps `f64` so the surrounding [`crate::options::TestOptions`] can keep
/// deriving `Eq`/`Combine` without pulling `f64` into those bounds. Bit-wise
/// equality is used (`NaN` is not a valid value because the option is
/// validated at parse time).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SlowTimeoutSecs(pub f64);

impl Eq for SlowTimeoutSecs {}

impl SlowTimeoutSecs {
    pub fn as_duration(self) -> Option<Duration> {
        if self.0.is_finite() && self.0 > 0.0 {
            Some(Duration::from_secs_f64(self.0))
        } else {
            None
        }
    }
}

impl Combine for SlowTimeoutSecs {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

/// A per-test timeout expressed in seconds.
///
/// Wraps `f64` for the same reason as [`SlowTimeoutSecs`]. Tests exceeding
/// this duration are killed and reported as failures (see
/// [`crate::settings::TestSettings::timeout`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TestTimeoutSecs(pub f64);

impl Eq for TestTimeoutSecs {}

impl TestTimeoutSecs {
    pub fn as_duration(self) -> Option<Duration> {
        if self.0.is_finite() && self.0 > 0.0 {
            Some(Duration::from_secs_f64(self.0))
        } else {
            None
        }
    }
}

impl Combine for TestTimeoutSecs {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

/// A coverage threshold expressed as a percentage (`0..=100`).
///
/// Wraps `f64` for the same reason as [`SlowTimeoutSecs`]: keeps the
/// surrounding [`crate::options::CoverageOptions`] `Eq`/`Combine` derives
/// straightforward. `NaN` is rejected at parse time so bit-wise equality is
/// safe here.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CovFailUnder(pub f64);

impl Eq for CovFailUnder {}

impl Combine for CovFailUnder {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

#[derive(Default, Debug, Clone)]
pub struct ProjectSettings {
    pub(crate) terminal: TerminalSettings,
    pub(crate) src: SrcSettings,
    pub(crate) test: TestSettings,
    pub(crate) coverage: CoverageSettings,
}

impl ProjectSettings {
    pub fn terminal(&self) -> &TerminalSettings {
        &self.terminal
    }

    pub fn src(&self) -> &SrcSettings {
        &self.src
    }

    pub fn test(&self) -> &TestSettings {
        &self.test
    }

    pub fn coverage(&self) -> &CoverageSettings {
        &self.coverage
    }

    pub fn max_fail(&self) -> MaxFail {
        self.test.max_fail
    }

    pub fn set_filter(&mut self, filter: FiltersetSet) {
        self.test.filter = filter;
    }

    pub fn set_run_ignored(&mut self, mode: RunIgnoredMode) {
        self.test.run_ignored = mode;
    }

    /// Round-trip the resolved settings back to a fully-populated [`Options`].
    ///
    /// Every field is `Some(...)` so a serialized form reflects the values
    /// karva is actually running with, including defaults. Runtime-only
    /// fields (`filter`, `run_ignored`, coverage `disabled`) are excluded:
    /// they do not come from configuration files. `fail_fast` is also
    /// omitted since `max_fail` is the canonical form.
    pub fn to_options(&self) -> Options {
        Options {
            src: Some(SrcOptions {
                respect_ignore_files: Some(self.src.respect_ignore_files),
                include: Some(self.src.include_paths.clone()),
            }),
            terminal: Some(TerminalOptions {
                output_format: Some(self.terminal.output_format),
                show_python_output: Some(self.terminal.show_python_output),
                status_level: Some(self.terminal.status_level),
                final_status_level: Some(self.terminal.final_status_level),
            }),
            test: Some(TestOptions {
                test_function_prefix: Some(self.test.test_function_prefix.clone()),
                fail_fast: None,
                // `MaxFail::unlimited()` wraps `None`, which TOML cannot
                // represent. Omit the field in that case so the TOML matches
                // "no limit set".
                max_fail: self.test.max_fail.has_limit().then_some(self.test.max_fail),
                try_import_fixtures: Some(self.test.try_import_fixtures),
                retry: Some(self.test.retry),
                no_tests: Some(self.test.no_tests),
                slow_timeout: self
                    .test
                    .slow_timeout
                    .map(|d| SlowTimeoutSecs(d.as_secs_f64())),
                timeout: self.test.timeout.map(|d| TestTimeoutSecs(d.as_secs_f64())),
            }),
            coverage: Some(CoverageOptions {
                sources: Some(self.coverage.sources.clone()),
                report: Some(self.coverage.report),
                fail_under: self.coverage.fail_under.map(CovFailUnder),
                disabled: None,
            }),
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct TerminalSettings {
    pub output_format: OutputFormat,
    pub show_python_output: bool,
    pub status_level: StatusLevel,
    pub final_status_level: FinalStatusLevel,
}

#[derive(Default, Debug, Clone)]
pub struct SrcSettings {
    pub respect_ignore_files: bool,
    pub include_paths: Vec<String>,
}

#[derive(Default, Debug, Clone)]
pub struct CoverageSettings {
    pub sources: Vec<String>,
    pub report: CovReport,
    pub report_path: Option<String>,
    /// Minimum total coverage percentage (`0..=100`). When set and the
    /// reported `TOTAL` coverage is below this value, the test command
    /// exits with a non-zero status even if every test passed.
    pub fail_under: Option<f64>,
}

#[derive(Default, Debug, Clone)]
pub struct TestSettings {
    pub test_function_prefix: String,
    pub max_fail: MaxFail,
    pub try_import_fixtures: bool,
    pub retry: u32,
    pub filter: FiltersetSet,
    pub run_ignored: RunIgnoredMode,
    pub no_tests: NoTestsMode,
    /// Threshold after which a test is flagged as slow. `None` disables
    /// slow-test detection entirely.
    pub slow_timeout: Option<Duration>,
    /// Hard per-test timeout. Tests that run longer than this duration are
    /// killed and reported as failures. `None` disables the hard timeout
    /// (tests may still set their own limit via `@karva.tags.timeout`).
    pub timeout: Option<Duration>,
}
