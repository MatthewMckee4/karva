//! Versioned, atomically replaced mutation results and source-bound identities.

use std::collections::BTreeMap;
use std::hash::Hasher;
use std::io::Write;

use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use siphasher::sip128::{Hasher128, SipHasher13};
use tempfile::NamedTempFile;

/// Current native mutation result schema.
const FORMAT_VERSION: u32 = 1;

/// A stable digest of length-delimited byte strings, independent of Rust's `Hash` format.
pub fn fingerprint(parts: &[&[u8]]) -> String {
    let mut hash = SipHasher13::new();
    for part in parts {
        hash.write(part.len().to_string().as_bytes());
        hash.write(b":");
        hash.write(part);
    }
    format!("{:032x}", hash.finish128().as_u128())
}

/// Independently selectable mutation families.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operator {
    /// Replace a boolean, number, or string literal.
    Literal,

    /// Invert a comparison or move its inclusive boundary.
    Comparison,

    /// Exchange conjunction/disjunction or remove negation.
    Boolean,

    /// Replace an arithmetic operator.
    Arithmetic,

    /// Force a branch or exchange loop termination behavior.
    ControlFlow,

    /// Replace an explicit return value.
    Return,
}

impl Operator {
    /// Names accepted by configuration and CLI filters.
    pub const NAMES: &[&str] = &[
        "literal",
        "comparison",
        "boolean",
        "arithmetic",
        "control-flow",
        "return",
    ];

    /// Parses the public spelling, reporting both the request and allowed values.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "literal" => Ok(Self::Literal),
            "comparison" => Ok(Self::Comparison),
            "boolean" => Ok(Self::Boolean),
            "arithmetic" => Ok(Self::Arithmetic),
            "control-flow" => Ok(Self::ControlFlow),
            "return" => Ok(Self::Return),
            _ => bail!(
                "unknown mutation operator `{value}`; allowed: {}",
                Self::NAMES.join(", ")
            ),
        }
    }
}

/// One exact edit in a project-relative source file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Mutant {
    /// Deterministic identity binding path, source, span, operator, and replacement.
    pub id: String,

    /// Portable project-relative file path.
    pub path: Utf8PathBuf,

    /// Fingerprint of the complete original file.
    pub source_fingerprint: String,

    /// Inclusive UTF-8 byte offset in the original source.
    pub start: usize,

    /// Exclusive UTF-8 byte offset in the original source.
    pub end: usize,

    /// One-based original source line.
    pub line: usize,

    /// Qualified containing class/function, or an empty string for module scope.
    pub symbol: String,

    /// Mutation family selected by this candidate.
    pub operator: Operator,

    /// Exact original bytes in the span.
    pub original: String,

    /// Exact replacement bytes.
    pub replacement: String,
}

impl Mutant {
    /// Validates the complete source and applies only this edit in memory.
    pub fn apply(&self, source: &str) -> Result<String> {
        if fingerprint(&[source.as_bytes()]) != self.source_fingerprint {
            bail!(
                "mutant {}: stale source `{}`; rerun `uv run karva mutate run` before applying",
                self.id,
                self.path
            );
        }
        if source.get(self.start..self.end) != Some(self.original.as_str()) {
            bail!(
                "mutant {}: source span {}..{} conflicts with `{}`",
                self.id,
                self.start,
                self.end,
                self.path
            );
        }
        let mut mutated = source.to_owned();
        mutated.replace_range(self.start..self.end, &self.replacement);
        Ok(mutated)
    }
}

/// Durable lifecycle and classification for one candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// Not completed; safe to execute after interruption.
    Pending,

    /// A test failed under the mutation.
    Killed,

    /// The complete safety check passed under the mutation.
    Survived,

    /// The measured execution budget expired.
    TimedOut,

    /// Python rejected the mutated source during compilation.
    Invalid,

    /// Execution or transport failed without a trustworthy test classification.
    Errored,
}

/// Classification and evidence retained for one candidate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MutationRecord {
    /// Source-bound candidate description.
    pub mutant: Mutant,

    /// Most recent completed outcome, or pending work.
    pub outcome: Outcome,

    /// Qualified test selectors actually executed.
    pub selected_tests: Vec<String>,

    /// Wall time spent executing the candidate, in seconds.
    pub duration_seconds: f64,

    /// Why these tests were selected or the result was reused.
    pub reason: String,

    /// Failure, timeout, or infrastructure detail, when present.
    pub diagnostic: Option<String>,
}

/// A resumable run, keyed by deterministic mutant identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MutationResults {
    format_version: u32,

    /// Identity of source, tests, dependencies, interpreter, environment, and configuration.
    pub context_fingerprint: String,

    /// Effective run configuration for later inspection.
    pub configuration: serde_json::Value,

    /// Measured full baseline wall time used to size mutant budgets.
    pub baseline_seconds: f64,

    /// All known candidates and their most recent outcomes.
    pub records: BTreeMap<String, MutationRecord>,
}

impl MutationResults {
    /// Creates an empty artifact in the current format.
    pub fn new(context_fingerprint: String, configuration: serde_json::Value) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            context_fingerprint,
            configuration,
            baseline_seconds: 0.0,
            records: BTreeMap::new(),
        }
    }

    /// Atomically replaces an artifact; a failed write leaves the previous file intact.
    pub fn write(&self, path: &Utf8Path) -> Result<()> {
        let parent = path
            .parent()
            .context("mutation results require a parent directory")?;
        std::fs::create_dir_all(parent)?;
        let mut temporary = NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut temporary, self)?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|error| error.error)?;
        Ok(())
    }

    /// Rejects incompatible data before deserializing the current record schema.
    pub fn read(path: &Utf8Path) -> Result<Self> {
        let source = std::fs::read(path).with_context(|| {
            format!("failed to read mutation results `{path}`; run `uv run karva mutate run` first")
        })?;
        let header: serde_json::Value = serde_json::from_slice(&source)?;
        let version = header
            .get("format_version")
            .and_then(serde_json::Value::as_u64);
        if version != Some(u64::from(FORMAT_VERSION)) {
            bail!(
                "mutation results `{path}` have format version {version:?}; supported version is {FORMAT_VERSION}; remove this artifact and rerun `uv run karva mutate run`"
            );
        }
        serde_json::from_slice(&source).context("invalid native mutation result data")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_incompatible_data_before_decoding_records() {
        let temp = tempfile::tempdir().expect("directory");
        let path = Utf8Path::from_path(temp.path())
            .expect("UTF-8")
            .join("results.json");
        std::fs::write(&path, r#"{"format_version":999}"#).expect("write");
        let error = MutationResults::read(&path).expect_err("incompatible");
        assert!(error.to_string().contains("supported version is 1"));
    }

    #[test]
    fn completed_results_survive_atomic_replacement() {
        let temp = tempfile::tempdir().expect("directory");
        let path = Utf8Path::from_path(temp.path())
            .expect("UTF-8")
            .join("results.json");
        let mut results = MutationResults::new("context".into(), serde_json::json!({}));
        results.write(&path).expect("first write");
        results.baseline_seconds = 0.25;
        results.write(&path).expect("replace");
        assert_eq!(
            MutationResults::read(&path).expect("read").baseline_seconds,
            0.25
        );
    }
}
