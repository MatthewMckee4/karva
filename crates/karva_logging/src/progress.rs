use karva_combine::Combine;
use serde::{Deserialize, Serialize};

/// How to display run progress while tests are executing.
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
    /// No live progress display.
    #[default]
    None,
    /// Print a refreshing `N/M tests` counter on stderr.
    Counter,
    /// Render a visual progress bar on stderr.
    Bar,
}

impl ProgressMode {
    pub const fn as_str(self) -> &'static str {
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
