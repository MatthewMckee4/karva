use std::time::Duration;

use karva_combine::Combine;
use karva_logging::{FinalStatusLevel, StatusLevel};
use serde::{Deserialize, Serialize, Serializer};

use crate::filter::{EvalContext, FiltersetSet, ValidatedFilter};
use crate::max_fail::MaxFail;
use crate::options::{CovReport, OutputFormat};

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

/// A global run timeout expressed in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunTimeoutSecs(pub f64);

impl Eq for RunTimeoutSecs {}

impl RunTimeoutSecs {
    pub fn as_duration(self) -> Option<Duration> {
        if self.0.is_finite() && self.0 > 0.0 {
            Some(Duration::from_secs_f64(self.0))
        } else {
            None
        }
    }
}

impl Combine for RunTimeoutSecs {
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

#[derive(Default, Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProjectSettings {
    pub(crate) src: SrcSettings,
    pub(crate) terminal: TerminalSettings,
    pub(crate) test: TestSettings,
    pub(crate) coverage: CoverageSettings,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) overrides: Vec<OverrideSettings>,
}

/// A compiled per-test override applied when its [filter](Self::filter)
/// matches the running test.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct OverrideSettings {
    pub filter: ValidatedFilter,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<TestTimeoutSecs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slow_timeout: Option<SlowTimeoutSecs>,
}

impl OverrideSettings {
    pub fn matches(&self, ctx: &EvalContext<'_>) -> bool {
        self.filter.matches(ctx)
    }
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

    pub fn overrides(&self) -> &[OverrideSettings] {
        &self.overrides
    }

    /// Find the first matching override that sets a value for `field`.
    fn first_matching_override<T>(
        &self,
        ctx: &EvalContext<'_>,
        field: impl Fn(&OverrideSettings) -> Option<T>,
    ) -> Option<T> {
        self.overrides
            .iter()
            .find_map(|ovr| ovr.matches(ctx).then(|| field(ovr)).flatten())
    }

    /// Resolve the retry budget for a single test.
    ///
    /// Walks through the configured overrides in order; the first match
    /// with `retries` set wins. Falls back to the profile-level
    /// [`TestSettings::retry`] when no override matches.
    pub fn retry_for(&self, ctx: &EvalContext<'_>) -> u32 {
        self.first_matching_override(ctx, |ovr| ovr.retries)
            .unwrap_or(self.test.retry)
    }

    /// Resolve the hard per-test timeout for a single test.
    ///
    /// First match wins. A matching override with a non-positive
    /// `timeout` disables the hard limit for that test even when the
    /// profile sets one.
    pub fn timeout_for(&self, ctx: &EvalContext<'_>) -> Option<Duration> {
        if let Some(secs) = self.first_matching_override(ctx, |ovr| ovr.timeout) {
            return secs.as_duration();
        }
        self.test.timeout
    }

    /// Resolve the slow-test threshold for a single test.
    ///
    /// First match wins. A matching override with a non-positive value
    /// disables slow tracking for that test even when the profile sets a
    /// threshold.
    pub fn slow_timeout_for(&self, ctx: &EvalContext<'_>) -> Option<Duration> {
        if let Some(secs) = self.first_matching_override(ctx, |ovr| ovr.slow_timeout) {
            return secs.as_duration();
        }
        self.test.slow_timeout
    }

    pub fn set_filter(&mut self, filter: FiltersetSet) {
        self.test.filter = filter;
    }

    pub fn set_run_ignored(&mut self, mode: RunIgnoredMode) {
        self.test.run_ignored = mode;
    }
}

/// Serialize a `Duration` field as fractional seconds. Unset fields are
/// guarded by `skip_serializing_if = "Option::is_none"`; the `None` arm is
/// preserved so the function is sound when called directly.
///
/// The `&Option<T>` signature is dictated by serde's `serialize_with`.
#[expect(clippy::ref_option)]
fn serialize_duration_secs<S: Serializer>(
    value: &Option<Duration>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(d) => serializer.serialize_f64(d.as_secs_f64()),
        None => serializer.serialize_none(),
    }
}

#[derive(Default, Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct TerminalSettings {
    pub output_format: OutputFormat,
    pub show_python_output: bool,
    pub status_level: StatusLevel,
    pub final_status_level: FinalStatusLevel,
}

#[derive(Default, Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SrcSettings {
    pub respect_ignore_files: bool,
    #[serde(rename = "include")]
    pub include_paths: Vec<String>,
}

#[derive(Default, Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct CoverageSettings {
    pub sources: Vec<String>,
    pub include: Vec<String>,
    pub omit: Vec<String>,
    pub report: CovReport,
    pub report_path: Option<String>,
    /// Minimum total coverage percentage (`0..=100`). When set and the
    /// reported `TOTAL` coverage is below this value, the test command
    /// exits with a non-zero status even if every test passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_under: Option<f64>,
}

#[derive(Default, Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct TestSettings {
    pub test_function_prefix: String,
    /// `MaxFail::unlimited()` wraps `None`, which TOML cannot represent —
    /// omit the field when no limit is configured.
    #[serde(skip_serializing_if = "MaxFail::is_unlimited")]
    pub max_fail: MaxFail,
    pub try_import_fixtures: bool,
    pub retry: u32,
    /// Runtime-only: filters are sourced from CLI flags, never config files.
    #[serde(skip)]
    pub filter: FiltersetSet,
    /// Runtime-only: run-ignored mode is sourced from CLI flags.
    #[serde(skip)]
    pub run_ignored: RunIgnoredMode,
    pub no_tests: NoTestsMode,
    /// Threshold after which a test is flagged as slow. `None` disables
    /// slow-test detection entirely.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_duration_secs"
    )]
    pub slow_timeout: Option<Duration>,
    /// Hard per-test timeout. Tests that run longer than this duration are
    /// killed and reported as failures. `None` disables the hard timeout
    /// (tests may still set their own limit via `@karva.tags.timeout`).
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_duration_secs"
    )]
    pub timeout: Option<Duration>,
    /// Wall-clock limit for the entire run. When the run exceeds this
    /// duration, the remaining workers are stopped and the run fails.
    /// `None` disables the limit.
    pub run_timeout: Option<Duration>,
}
