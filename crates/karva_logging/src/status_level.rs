use karva_combine::Combine;
use serde::{Deserialize, Serialize};

/// Which test result statuses to display during the run.
///
/// Modeled after `cargo-nextest`'s `--status-level`. Levels are cumulative:
/// each level displays its own status plus all earlier statuses.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum StatusLevel {
    /// Don't display any test result lines (or the "Starting" header).
    None,
    /// Only display failed test results.
    Fail,
    /// Display failed test results plus a `TRY N FAIL` line for each failed
    /// attempt that was retried.
    Retry,
    /// Display failed, retried, and slow test results. Karva does not yet
    /// have a slow-test threshold, so this currently behaves like `retry`.
    Slow,
    /// Display failed, retried, slow, and passing test results (default).
    #[default]
    Pass,
    /// Additionally display skipped test results.
    Skip,
    /// Display all test result statuses.
    All,
}

impl StatusLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fail => "fail",
            Self::Retry => "retry",
            Self::Slow => "slow",
            Self::Pass => "pass",
            Self::Skip => "skip",
            Self::All => "all",
        }
    }
}

impl Combine for StatusLevel {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

/// Which final summary information to display at the end of the run.
///
/// Modeled after `cargo-nextest`'s `--final-status-level`. Levels are
/// cumulative in the same way as [`StatusLevel`].
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum FinalStatusLevel {
    /// Don't display the summary line or any diagnostic blocks.
    None,
    /// Only display the summary line and diagnostics on failure.
    Fail,
    /// Display the summary line plus diagnostics on failure or when any
    /// test was retried. The summary line gains a `N retried` count whenever
    /// a retry happened.
    Retry,
    /// Same as `retry` until a slow-test threshold is implemented.
    Slow,
    /// Always display the summary line and diagnostics (default).
    #[default]
    Pass,
    /// Same as `pass` until skip-specific summary lines are emitted.
    Skip,
    /// Always display every summary status.
    All,
}

impl FinalStatusLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fail => "fail",
            Self::Retry => "retry",
            Self::Slow => "slow",
            Self::Pass => "pass",
            Self::Skip => "skip",
            Self::All => "all",
        }
    }
}

impl Combine for FinalStatusLevel {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use karva_combine::Combine as _;

    use super::{FinalStatusLevel, StatusLevel};

    #[test]
    fn status_level_as_str_matches_cli_values() {
        assert_eq!(StatusLevel::None.as_str(), "none");
        assert_eq!(StatusLevel::Fail.as_str(), "fail");
        assert_eq!(StatusLevel::Retry.as_str(), "retry");
        assert_eq!(StatusLevel::Slow.as_str(), "slow");
        assert_eq!(StatusLevel::Pass.as_str(), "pass");
        assert_eq!(StatusLevel::Skip.as_str(), "skip");
        assert_eq!(StatusLevel::All.as_str(), "all");
    }

    #[test]
    fn final_status_level_as_str_matches_cli_values() {
        assert_eq!(FinalStatusLevel::None.as_str(), "none");
        assert_eq!(FinalStatusLevel::Fail.as_str(), "fail");
        assert_eq!(FinalStatusLevel::Retry.as_str(), "retry");
        assert_eq!(FinalStatusLevel::Slow.as_str(), "slow");
        assert_eq!(FinalStatusLevel::Pass.as_str(), "pass");
        assert_eq!(FinalStatusLevel::Skip.as_str(), "skip");
        assert_eq!(FinalStatusLevel::All.as_str(), "all");
    }

    #[test]
    fn status_levels_keep_left_hand_precedence_when_combined() {
        assert_eq!(
            StatusLevel::Fail.combine(StatusLevel::All),
            StatusLevel::Fail
        );

        let mut level = StatusLevel::Retry;
        level.combine_with(StatusLevel::None);
        assert_eq!(level, StatusLevel::Retry);
    }

    #[test]
    fn final_status_levels_keep_left_hand_precedence_when_combined() {
        assert_eq!(
            FinalStatusLevel::Fail.combine(FinalStatusLevel::All),
            FinalStatusLevel::Fail
        );

        let mut level = FinalStatusLevel::Retry;
        level.combine_with(FinalStatusLevel::None);
        assert_eq!(level, FinalStatusLevel::Retry);
    }
}
