use std::time::Duration;

use karva_combine::Combine;
use karva_logging::{FinalStatusLevel, StatusLevel};
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::filter::{EvalContext, FiltersetSet, ValidatedFilter};
use crate::max_fail::MaxFail;
use crate::options::{CovReport, OutputFormat};

/// Project-relative native coverage artifact used when no path is configured.
pub const DEFAULT_COVERAGE_DATA_FILE: &str = ".karva/coverage/data.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
/// Validated regular expression used to exclude coverage opportunities.
pub struct CoverageExcludePattern(String);

impl CoverageExcludePattern {
    /// Returns original configured regular expression.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for CoverageExcludePattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let pattern = String::deserialize(deserializer)?;
        Regex::new(&pattern).map_err(|error| {
            serde::de::Error::custom(format!(
                "invalid coverage exclusion pattern `{pattern}`: {error}"
            ))
        })?;
        Ok(Self(pattern))
    }
}

macro_rules! impl_duration_secs_deserialize {
    ($type:ident, $option:literal) => {
        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let seconds = f64::deserialize(deserializer)?;
                if !seconds.is_finite() || (seconds > 0.0 && Self(seconds).as_duration().is_none())
                {
                    return Err(serde::de::Error::custom(concat!(
                        $option,
                        " must be a finite duration supported by this platform"
                    )));
                }
                Ok(Self(seconds))
            }
        }
    };
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
/// Runtime selection for tests marked ignored or skipped.
pub enum RunIgnoredMode {
    /// Preserve configured skip behavior.
    #[default]
    Default,

    /// Run ignored tests and exclude normal tests.
    Only,

    /// Run ignored and normal tests.
    All,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum NoTestsMode {
    /// Fail without filters, but pass when explicit filters match nothing.
    #[default]
    Auto,

    /// Exit successfully without a diagnostic.
    Pass,

    /// Warn and exit successfully.
    Warn,

    /// Report an error and return failure.
    Fail,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum FlakyResult {
    /// Tests that eventually pass remain successful.
    #[default]
    Pass,

    /// Any test that needed a retry fails the run.
    Fail,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum JunitFlakyFailStatus {
    /// Record failed attempts as `JUnit` failures.
    #[default]
    Failure,

    /// Represent eventually passing tests as successful in `JUnit`.
    Success,
}

impl JunitFlakyFailStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Failure => "failure",
            Self::Success => "success",
        }
    }
}

impl FlakyResult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

impl Combine for JunitFlakyFailStatus {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

impl Combine for FlakyResult {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct SlowTimeoutSecs(pub f64);

impl Eq for SlowTimeoutSecs {}

impl SlowTimeoutSecs {
    /// Converts positive, platform-representable seconds; non-positive values disable tracking.
    pub fn as_duration(self) -> Option<Duration> {
        if self.0.is_finite() && self.0 > 0.0 {
            Duration::try_from_secs_f64(self.0)
                .ok()
                .filter(|duration| !duration.is_zero())
        } else {
            None
        }
    }
}

impl_duration_secs_deserialize!(SlowTimeoutSecs, "slow-timeout");

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
/// `TestSettings::timeout`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct TestTimeoutSecs(pub f64);

impl Eq for TestTimeoutSecs {}

impl TestTimeoutSecs {
    /// Converts positive, platform-representable seconds; non-positive values disable timeout.
    pub fn as_duration(self) -> Option<Duration> {
        if self.0.is_finite() && self.0 > 0.0 {
            Duration::try_from_secs_f64(self.0)
                .ok()
                .filter(|duration| !duration.is_zero())
        } else {
            None
        }
    }
}

impl_duration_secs_deserialize!(TestTimeoutSecs, "timeout");

impl Combine for TestTimeoutSecs {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

/// A per-test duration budget expressed in seconds.
///
/// Wraps `f64` for the same reason as [`SlowTimeoutSecs`]. Unlike
/// [`SlowTimeoutSecs`] (informational) or [`TestTimeoutSecs`] (kills the
/// test mid-flight), a test exceeding this budget is allowed to finish its
/// full lifecycle — including fixture teardown — and is then reported as a
/// failure (see `TestSettings::fail_slow`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct FailSlowSecs(pub f64);

impl Eq for FailSlowSecs {}

impl FailSlowSecs {
    /// Converts positive, platform-representable seconds; non-positive values disable budget.
    pub fn as_duration(self) -> Option<Duration> {
        if self.0.is_finite() && self.0 > 0.0 {
            Duration::try_from_secs_f64(self.0)
                .ok()
                .filter(|duration| !duration.is_zero())
        } else {
            None
        }
    }
}

impl_duration_secs_deserialize!(FailSlowSecs, "fail-slow");

impl Combine for FailSlowSecs {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

/// A global run timeout expressed in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct RunTimeoutSecs(pub f64);

impl Eq for RunTimeoutSecs {}

impl RunTimeoutSecs {
    /// Converts positive, platform-representable seconds; non-positive values disable timeout.
    pub fn as_duration(self) -> Option<Duration> {
        if self.0.is_finite() && self.0 > 0.0 {
            Duration::try_from_secs_f64(self.0)
                .ok()
                .filter(|duration| !duration.is_zero())
        } else {
            None
        }
    }
}

impl_duration_secs_deserialize!(RunTimeoutSecs, "run-timeout");

impl Combine for RunTimeoutSecs {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

/// Grace period between graceful termination and force-kill, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct TerminationGracePeriodSecs(pub f64);

impl Eq for TerminationGracePeriodSecs {}

impl TerminationGracePeriodSecs {
    /// Converts non-negative, platform-representable seconds; zero requests immediate kill.
    pub fn as_duration(self) -> Option<Duration> {
        if self.0.is_finite() && self.0 >= 0.0 {
            Duration::try_from_secs_f64(self.0).ok()
        } else {
            None
        }
    }
}

impl_duration_secs_deserialize!(TerminationGracePeriodSecs, "termination-grace-period");

impl Combine for TerminationGracePeriodSecs {
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

/// Decimal places used for coverage percentages.
///
/// The maximum is [`f64::DIGITS`], the number of meaningful decimal digits
/// available from the percentage representation.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct CoveragePrecision(pub usize);

impl CoveragePrecision {
    /// Maximum meaningful precision for an `f64` percentage.
    pub const MAX: usize = f64::DIGITS as usize;
}

impl TryFrom<usize> for CoveragePrecision {
    type Error = String;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        if value <= Self::MAX {
            Ok(Self(value))
        } else {
            Err(format!(
                "precision must be at most {} because coverage percentages use f64, got `{value}`",
                Self::MAX
            ))
        }
    }
}

impl std::str::FromStr for CoveragePrecision {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<usize>()
            .map_err(|error| format!("`{value}` is not a non-negative integer: {error}"))?
            .try_into()
    }
}

impl<'de> Deserialize<'de> for CoveragePrecision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        usize::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl Combine for CoveragePrecision {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

#[derive(Default, Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Fully resolved settings after configuration profiles and CLI overrides combine.
pub struct ProjectSettings {
    pub(crate) src: SrcSettings,
    pub(crate) terminal: TerminalSettings,
    pub(crate) test: TestSettings,
    pub(crate) coverage: CoverageSettings,
    pub(crate) junit: JunitSettings,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) overrides: Vec<OverrideSettings>,
}

/// A compiled per-test override applied when its [filter](Self::filter)
/// matches the running test.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct OverrideSettings {
    /// Compiled selector deciding which tests receive this override.
    pub filter: ValidatedFilter,

    /// Retry count replacing profile-level retry policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,

    /// Flaky-result policy replacing profile-level policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flaky_result: Option<FlakyResult>,

    /// `JUnit` flaky policy replacing profile-level policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub junit_flaky_fail_status: Option<JunitFlakyFailStatus>,

    /// Hard timeout in seconds; non-positive values disable it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<TestTimeoutSecs>,

    /// Slow-test threshold in seconds; non-positive values disable it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slow_timeout: Option<SlowTimeoutSecs>,

    /// Post-run duration budget in seconds; non-positive values disable it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_slow: Option<FailSlowSecs>,
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

    pub fn junit(&self) -> &JunitSettings {
        &self.junit
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
    /// `TestSettings::retry` when no override matches.
    pub fn retry_for(&self, ctx: &EvalContext<'_>) -> u32 {
        self.first_matching_override(ctx, |ovr| ovr.retries)
            .unwrap_or(self.test.retry)
    }

    /// Resolves first matching flaky-result override, then profile policy.
    pub fn flaky_result_for(&self, ctx: &EvalContext<'_>) -> FlakyResult {
        self.first_matching_override(ctx, |ovr| ovr.flaky_result)
            .unwrap_or(self.test.flaky_result)
    }

    /// Resolves first matching `JUnit` flaky override, then report policy.
    pub fn junit_flaky_fail_status_for(&self, ctx: &EvalContext<'_>) -> JunitFlakyFailStatus {
        self.first_matching_override(ctx, |ovr| ovr.junit_flaky_fail_status)
            .unwrap_or(self.junit.flaky_fail_status)
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

    /// Resolve the fail-slow duration budget for a single test.
    ///
    /// First match wins. A matching override with a non-positive value
    /// disables the budget for that test even when the profile sets one.
    /// Unlike [`Self::timeout_for`], this never kills a running test — it
    /// only determines, after the fact, whether the full lifecycle
    /// (setup + call + teardown) took too long.
    pub fn fail_slow_for(&self, ctx: &EvalContext<'_>) -> Option<Duration> {
        if let Some(secs) = self.first_matching_override(ctx, |ovr| ovr.fail_slow) {
            return secs.as_duration();
        }
        self.test.fail_slow
    }

    /// Replaces runtime-only CLI filter selection after configuration resolution.
    pub fn set_filter(&mut self, filter: FiltersetSet) {
        self.test.filter = filter;
    }

    /// Replaces runtime-only ignored-test selection after configuration resolution.
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
/// Resolved terminal presentation settings.
pub struct TerminalSettings {
    /// Human or machine diagnostic rendering.
    pub output_format: OutputFormat,

    /// Whether captured Python streams are printed for successful tests.
    pub show_python_output: bool,

    /// Minimum status shown while tests execute.
    pub status_level: StatusLevel,

    /// Minimum status shown in final output.
    pub final_status_level: FinalStatusLevel,
}

#[derive(Default, Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Resolved source-discovery settings.
pub struct SrcSettings {
    /// Whether filesystem discovery honors ignore files.
    pub respect_ignore_files: bool,

    /// User selections passed into project test-path resolution.
    #[serde(rename = "include")]
    pub include_paths: Vec<String>,
}

#[derive(Default, Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Resolved coverage collection and reporting settings.
pub struct CoverageSettings {
    /// Native coverage artifact path, relative to the project root when not absolute.
    pub data_file: String,

    /// Ordered `FROM=TO` mappings applied to paths loaded from native artifacts.
    pub path_aliases: Vec<String>,

    /// Source roots measured by worker tracers.
    pub sources: Vec<String>,

    /// Report-path globs that must match for inclusion.
    pub include: Vec<String>,

    /// Report-path globs excluded after inclusion.
    pub omit: Vec<String>,

    /// Regular expressions excluding matched source lines and clauses.
    pub exclude_lines: Vec<CoverageExcludePattern>,

    /// Regular expressions selecting execution contexts for reports.
    pub contexts: Vec<String>,

    /// Decimal places shown in coverage percentages.
    pub precision: CoveragePrecision,

    /// Whether a test run unions observations with an existing native artifact.
    pub append: bool,

    /// Selected report backend.
    pub report: CovReport,

    /// Optional output path for file-based reports.
    pub report_path: Option<String>,

    /// Whether workers record branch arcs.
    #[serde(skip_serializing_if = "is_false")]
    pub branch: bool,

    /// Minimum total coverage percentage (`0..=100`). When set and the
    /// reported `TOTAL` coverage is below this value, the test command
    /// exits with a non-zero status even if every test passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_under: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Resolved `JUnit` XML report settings.
pub struct JunitSettings {
    /// Output file; `None` disables `JUnit` generation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// Root testsuites name written into XML.
    pub report_name: String,

    /// Whether passing-test captured streams are written into XML.
    #[serde(skip_serializing_if = "is_false")]
    pub store_success_output: bool,

    /// Whether failed-test captured streams are written into XML.
    pub store_failure_output: bool,

    /// Outcome assigned to failed attempts before an eventual pass.
    pub flaky_fail_status: JunitFlakyFailStatus,
}

impl Default for JunitSettings {
    fn default() -> Self {
        Self {
            path: None,
            report_name: "karva-tests".to_string(),
            store_success_output: false,
            store_failure_output: true,
            flaky_fail_status: JunitFlakyFailStatus::default(),
        }
    }
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if passes a reference to the field"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Default, Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Resolved test selection, retry, timeout, and failure settings.
pub struct TestSettings {
    /// Prefix identifying Python test functions during syntax collection.
    pub test_function_prefix: String,

    /// `MaxFail::unlimited()` wraps `None`, which TOML cannot represent —
    /// omit the field when no limit is configured.
    #[serde(skip_serializing_if = "MaxFail::is_unlimited")]
    pub max_fail: MaxFail,

    /// Whether collection imports modules to discover fixtures dynamically.
    pub try_import_fixtures: bool,

    /// Additional attempts permitted after first failure.
    pub retry: u32,

    /// Whether eventual success after retry passes the run.
    pub flaky_result: FlakyResult,

    /// Runtime-only: filters are sourced from CLI flags, never config files.
    #[serde(skip)]
    pub filter: FiltersetSet,

    /// Runtime-only: run-ignored mode is sourced from CLI flags.
    #[serde(skip)]
    pub run_ignored: RunIgnoredMode,

    /// Behavior when selection produces no runnable tests.
    pub no_tests: NoTestsMode,

    /// Threshold after which a test is flagged as slow. `None` disables
    /// slow-test detection entirely.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_duration_secs"
    )]
    pub slow_timeout: Option<Duration>,

    /// Duration budget for a test's full lifecycle (setup + call +
    /// teardown). `None` disables budget checking. Unlike `timeout`, a test
    /// exceeding this budget is allowed to finish (including teardown)
    /// before being reported as a failure.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_duration_secs"
    )]
    pub fail_slow: Option<Duration>,

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

    /// Grace period between graceful termination and force-kill when karva
    /// stops workers because of Ctrl+C, fail-fast, or run timeout.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_duration_secs"
    )]
    pub termination_grace_period: Option<Duration>,
}

impl TestSettings {
    /// Returns configured shutdown grace period or platform-independent default.
    pub fn termination_grace_period(&self) -> Duration {
        self.termination_grace_period
            .unwrap_or_else(default_termination_grace_period)
    }
}

/// Default delay between graceful worker termination and forced kill.
pub fn default_termination_grace_period() -> Duration {
    Duration::from_secs(10)
}
