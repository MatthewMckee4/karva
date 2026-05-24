use karva_combine::Combine;
use serde::{Deserialize, Serialize};

/// How to display run progress while tests are executing.
///
/// Modeled after `cargo-nextest`'s `--progress-bar` setting. The bar/counter
/// are rendered by the orchestrator on stderr; per-test result lines (gated
/// on [`crate::StatusLevel`]) continue to flow through stdout independently.
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
pub enum ProgressMode {
    /// No live progress display (default).
    #[default]
    None,
    /// Print a one-line `N/M tests` counter, refreshed periodically.
    Counter,
    /// Render a visual progress bar with completion stats.
    Bar,
}

impl ProgressMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Counter => "counter",
            Self::Bar => "bar",
        }
    }
}

impl Combine for ProgressMode {
    #[inline(always)]
    fn combine_with(&mut self, _other: Self) {}

    #[inline]
    fn combine(self, _other: Self) -> Self {
        self
    }
}
