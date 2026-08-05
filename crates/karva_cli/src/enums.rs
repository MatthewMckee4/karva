use std::str::FromStr;

use camino::Utf8PathBuf;

use karva_metadata::{
    FlakyResult as FlakyResultMode, JunitFlakyFailStatus as JunitFlakyFailStatusMode, NoTestsMode,
    RunIgnoredMode,
};

/// Coverage report selection parsed from `--cov-report`.
#[derive(Clone, Hash, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum CovReport {
    /// Persist native data without rendering a report.
    None,

    /// Compact terminal table (default).
    #[default]
    Term,

    /// Terminal table with a `Missing` column listing uncovered line numbers.
    TermMissing,

    /// Cobertura XML written to disk, optionally to a custom path.
    Xml { path: Option<Utf8PathBuf> },

    /// JSON coverage report written to disk, optionally to a custom path.
    Json { path: Option<Utf8PathBuf> },

    /// HTML coverage report written to disk, optionally to a custom directory.
    Html { path: Option<Utf8PathBuf> },

    /// LCOV tracefile written to disk, optionally to a custom path.
    Lcov { path: Option<Utf8PathBuf> },
}

impl FromStr for CovReport {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.split_once(':') {
            None => match raw {
                "" => Ok(Self::None),
                "term" => Ok(Self::Term),
                "term-missing" => Ok(Self::TermMissing),
                "xml" => Ok(Self::Xml { path: None }),
                "json" => Ok(Self::Json { path: None }),
                "html" => Ok(Self::Html { path: None }),
                "lcov" => Ok(Self::Lcov { path: None }),
                _ => Err(format!(
                    "invalid value `{raw}`; expected one of `term`, `term-missing`, `xml[:PATH]`, `json[:PATH]`, `html[:DIR]`, or `lcov[:PATH]`"
                )),
            },
            Some(("xml", path)) if !path.is_empty() => Ok(Self::Xml {
                path: Some(Utf8PathBuf::from(path)),
            }),
            Some(("json", path)) if !path.is_empty() => Ok(Self::Json {
                path: Some(Utf8PathBuf::from(path)),
            }),
            Some(("html", path)) if !path.is_empty() => Ok(Self::Html {
                path: Some(Utf8PathBuf::from(path)),
            }),
            Some(("lcov", path)) if !path.is_empty() => Ok(Self::Lcov {
                path: Some(Utf8PathBuf::from(path)),
            }),
            Some(("xml", _)) => Err("`xml` report path cannot be empty".to_string()),
            Some(("json", _)) => Err("`json` report path cannot be empty".to_string()),
            Some(("html", _)) => Err("`html` report path cannot be empty".to_string()),
            Some(("lcov", _)) => Err("`lcov` report path cannot be empty".to_string()),
            Some((kind, _)) => Err(format!(
                "report `{kind}` does not accept a path; expected `term`, `term-missing`, `xml[:PATH]`, `json[:PATH]`, `html[:DIR]`, or `lcov[:PATH]`"
            )),
        }
    }
}

/// The diagnostic output format.
#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq, PartialOrd, Ord, Default, clap::ValueEnum)]
pub enum OutputFormat {
    /// Print diagnostics verbosely, with context and helpful hints (default).
    #[default]
    #[value(name = "full")]
    Full,

    /// Print diagnostics concisely, one per line.
    #[value(name = "concise")]
    Concise,
}

/// Machine-readable test result report format.
#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq, PartialOrd, Ord, Default, clap::ValueEnum)]
pub enum ResultFormat {
    /// Write one JSON document with the full run result.
    #[default]
    #[value(name = "json")]
    Json,

    /// Write newline-delimited JSON records.
    #[value(name = "jsonl")]
    Jsonl,
}

impl From<OutputFormat> for karva_metadata::OutputFormat {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Full => Self::Full,
            OutputFormat::Concise => Self::Concise,
        }
    }
}

impl From<CovReport> for karva_metadata::CovReport {
    fn from(value: CovReport) -> Self {
        match value {
            CovReport::None => Self::None,
            CovReport::Term => Self::Term,
            CovReport::TermMissing => Self::TermMissing,
            CovReport::Xml { .. } => Self::Xml,
            CovReport::Json { .. } => Self::Json,
            CovReport::Html { .. } => Self::Html,
            CovReport::Lcov { .. } => Self::Lcov,
        }
    }
}

/// Coverage context selection parsed from `--cov-context`.
#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum CovContext {
    /// Record the current test name for each covered line.
    Test,
}

impl CovContext {
    /// Returns the value forwarded to coverage configuration.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Test => "test",
        }
    }
}

/// Whether to run ignored/skipped tests.
#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum RunIgnored {
    /// Run only ignored tests.
    Only,

    /// Run both ignored and non-ignored tests.
    All,
}

impl RunIgnored {
    /// Returns the canonical CLI spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Only => "only",
            Self::All => "all",
        }
    }
}

impl From<RunIgnored> for RunIgnoredMode {
    fn from(value: RunIgnored) -> Self {
        match value {
            RunIgnored::Only => Self::Only,
            RunIgnored::All => Self::All,
        }
    }
}

/// Behavior when no tests match filters.
#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum NoTests {
    /// Automatically determine behavior: fail if no filter expressions were
    /// given, pass silently if filters were given.
    Auto,

    /// Silently exit with code 0.
    Pass,

    /// Produce a warning and exit with code 0.
    Warn,

    /// Produce an error message and exit with a non-zero code.
    Fail,
}

impl From<NoTests> for NoTestsMode {
    fn from(value: NoTests) -> Self {
        match value {
            NoTests::Auto => Self::Auto,
            NoTests::Pass => Self::Pass,
            NoTests::Warn => Self::Warn,
            NoTests::Fail => Self::Fail,
        }
    }
}

/// Whether flaky tests pass or fail the run.
#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum FlakyResult {
    /// A test that passes after retrying does not fail the run.
    Pass,

    /// Any test requiring a retry fails the run.
    Fail,
}

impl From<FlakyResult> for FlakyResultMode {
    fn from(value: FlakyResult) -> Self {
        match value {
            FlakyResult::Pass => Self::Pass,
            FlakyResult::Fail => Self::Fail,
        }
    }
}

/// `JUnit` outcome assigned to attempts that fail before a later retry passes.
#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum JunitFlakyFailStatus {
    /// Emit failed attempts as `JUnit` failures.
    Failure,

    /// Preserve the final successful outcome without `JUnit` failures.
    Success,
}

impl From<JunitFlakyFailStatus> for JunitFlakyFailStatusMode {
    fn from(value: JunitFlakyFailStatus) -> Self {
        match value {
            JunitFlakyFailStatus::Failure => Self::Failure,
            JunitFlakyFailStatus::Success => Self::Success,
        }
    }
}
