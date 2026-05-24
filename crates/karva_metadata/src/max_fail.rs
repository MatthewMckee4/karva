use std::num::NonZeroU32;

use karva_combine::Combine;
use serde::{Deserialize, Serialize};

/// Controls how many tests may fail before karva stops scheduling new ones.
///
/// Modelled on nextest's `--max-fail` flag. A value of `None` means karva
/// never stops scheduling tests; a value of `Some(n)` stops once `n` tests
/// have failed, which generalizes the legacy `--fail-fast` flag (equivalent
/// to `Some(1)`).
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord,
)]
#[serde(transparent)]
pub struct MaxFail(Option<NonZeroU32>);

impl MaxFail {
    /// Returns a `MaxFail` that never stops scheduling tests.
    pub const fn unlimited() -> Self {
        Self(None)
    }

    /// Returns a `MaxFail` that stops after the given number of failures,
    /// treating `0` as unlimited.
    pub fn from_count(count: u32) -> Self {
        Self(NonZeroU32::new(count))
    }

    /// The legacy `fail-fast` boolean: `true` maps to stopping after a single
    /// failure, `false` maps to never stopping.
    pub fn from_fail_fast(fail_fast: bool) -> Self {
        if fail_fast {
            Self::from_count(1)
        } else {
            Self::unlimited()
        }
    }

    /// Returns `true` when the given number of failures meets or exceeds the
    /// configured budget.
    pub fn is_exceeded_by(self, failures: u32) -> bool {
        matches!(self.0, Some(limit) if failures >= limit.get())
    }

    /// Returns `true` when this configuration imposes any limit at all.
    pub fn has_limit(self) -> bool {
        self.0.is_some()
    }

    /// Returns `true` when no failure limit is configured.
    ///
    /// `MaxFail::unlimited()` wraps `None`, which serializers like TOML
    /// cannot represent — this is exposed primarily so `serde`'s
    /// `skip_serializing_if` can omit the field.
    pub fn is_unlimited(&self) -> bool {
        self.0.is_none()
    }

    /// Returns `true` when the configuration would stop after a single failure.
    ///
    /// This is how the legacy `--fail-fast` boolean is surfaced internally.
    pub fn is_fail_fast(self) -> bool {
        matches!(self.0, Some(limit) if limit.get() == 1)
    }

    /// Returns the configured limit as a raw `u32`, if any.
    pub fn limit(self) -> Option<NonZeroU32> {
        self.0
    }
}

impl From<Option<NonZeroU32>> for MaxFail {
    fn from(value: Option<NonZeroU32>) -> Self {
        Self(value)
    }
}

impl From<NonZeroU32> for MaxFail {
    fn from(value: NonZeroU32) -> Self {
        Self(Some(value))
    }
}

impl Combine for MaxFail {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exceeds_budget() {
        let two = MaxFail::from_count(2);
        assert!(!two.is_exceeded_by(0));
        assert!(!two.is_exceeded_by(1));
        assert!(two.is_exceeded_by(2));
        assert!(two.is_exceeded_by(3));

        assert!(!MaxFail::unlimited().is_exceeded_by(u32::MAX));
    }

    #[test]
    fn from_fail_fast() {
        assert_eq!(MaxFail::from_fail_fast(true), MaxFail::from_count(1));
        assert_eq!(MaxFail::from_fail_fast(false), MaxFail::unlimited());
    }

    #[test]
    fn from_count_zero_is_unlimited() {
        assert_eq!(MaxFail::from_count(0), MaxFail::unlimited());
        assert!(!MaxFail::from_count(0).has_limit());
    }
}
