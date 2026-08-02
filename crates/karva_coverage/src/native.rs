//! Versioned native coverage artifact.
//!
//! Worker payloads in [`crate::data`] are transient process communication.
//! This module owns Karva's durable interchange format for later combination
//! and reporting. It is deliberately unrelated to coverage.py's `SQLite` schema
//! and to exported `coverage.json` reports.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;
use std::io::Write;

use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;
use serde::{Deserialize, Serialize};
use siphasher::sip128::{Hasher128, SipHasher13};
use tempfile::NamedTempFile;

use crate::data::BranchArc;

/// Current native coverage schema version.
pub const FORMAT_VERSION: u32 = 1;

/// Karva coverage data retained after one or more test runs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NativeCoverage {
    /// Native artifact schema version.
    pub format_version: u32,
    /// Karva version that produced this artifact.
    pub karva_version: String,
    /// Whether collection measured statements alone or statements and branches.
    pub mode: CoverageMode,
    /// Normalized absolute project root used during collection.
    pub project_root: Utf8PathBuf,
    /// Normalized source roots measured during collection.
    pub source_roots: BTreeSet<Utf8PathBuf>,
    /// User-provided context attached to the whole run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_context: Option<String>,
    /// Coverage keyed by normalized project-relative source path.
    pub files: BTreeMap<Utf8PathBuf, NativeFileCoverage>,
}

impl NativeCoverage {
    /// Creates an artifact for the current schema and Karva version.
    pub fn new(
        mode: CoverageMode,
        project_root: Utf8PathBuf,
        source_roots: BTreeSet<Utf8PathBuf>,
        run_context: Option<String>,
        files: BTreeMap<Utf8PathBuf, NativeFileCoverage>,
    ) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            karva_version: env!("CARGO_PKG_VERSION").to_owned(),
            mode,
            project_root,
            source_roots,
            run_context,
            files,
        }
    }

    /// Serializes this artifact deterministically and atomically replaces `path`.
    pub fn write(&self, path: &Utf8Path) -> Result<()> {
        let mut json = serde_json::to_vec_pretty(self)
            .with_context(|| format!("failed to serialize native coverage artifact `{path}`"))?;
        json.push(b'\n');

        let parent = path
            .parent()
            .filter(|parent| !parent.as_str().is_empty())
            .unwrap_or_else(|| Utf8Path::new("."));
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create native coverage directory `{parent}`"))?;

        let mut temporary = NamedTempFile::new_in(parent)
            .with_context(|| format!("failed to create native coverage artifact `{path}`"))?;
        temporary
            .write_all(&json)
            .with_context(|| format!("failed to write native coverage artifact `{path}`"))?;
        temporary
            .flush()
            .with_context(|| format!("failed to flush native coverage artifact `{path}`"))?;
        temporary
            .persist(path.as_std_path())
            .map_err(|error| error.error)
            .with_context(|| format!("failed to replace native coverage artifact `{path}`"))?;
        Ok(())
    }

    /// Reads a supported native artifact, rejecting unknown schema versions.
    pub fn read(path: &Utf8Path) -> Result<Self> {
        let bytes = fs::read(path)
            .with_context(|| format!("failed to read native coverage artifact `{path}`"))?;
        let header: ArtifactHeader = serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse native coverage artifact `{path}`"))?;
        if header.format_version != FORMAT_VERSION {
            bail!(
                "unsupported native coverage artifact `{path}`: found format version {}, supported version is {FORMAT_VERSION}",
                header.format_version
            );
        }
        serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse native coverage artifact `{path}`"))
    }

    /// Fails when any source differs from the bytes measured during collection.
    pub fn verify_sources(&self) -> Result<()> {
        for (source_path, coverage) in &self.files {
            let path = self.project_root.join(source_path);
            let source = fs::read(&path)
                .with_context(|| format!("failed to verify coverage source `{path}`"))?;
            if !coverage.source_fingerprint.matches(&source) {
                bail!("coverage source changed since collection: `{path}`");
            }
        }
        Ok(())
    }
}

/// Coverage opportunities captured by an artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageMode {
    /// Statement coverage only.
    Line,
    /// Statement and branch coverage.
    Branch,
}

/// Durable coverage for one Python source file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NativeFileCoverage {
    /// Fingerprint of the source bytes used to compute executable lines.
    pub source_fingerprint: SourceFingerprint,
    /// Executable source lines.
    pub executable: BTreeSet<u32>,
    /// Executable lines removed by exclusion rules.
    pub excluded: BTreeSet<u32>,
    /// Source lines observed at runtime.
    pub executed: BTreeSet<u32>,
    /// Contexts that executed each source line.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub line_contexts: BTreeMap<u32, BTreeSet<String>>,
    /// Branch data, absent for statement-only collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branches: Option<NativeBranchCoverage>,
}

/// Durable branch coverage for one Python source file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NativeBranchCoverage {
    /// Statically discovered control-flow edges.
    pub possible: BTreeSet<BranchArc>,
    /// Control-flow edges observed at runtime.
    pub executed: BTreeSet<BranchArc>,
    /// Contexts grouped by executed edge, sorted by edge.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub contexts: BTreeSet<NativeArcContexts>,
}

/// Contexts that traversed one branch edge.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct NativeArcContexts {
    /// Executed control-flow edge.
    pub arc: BranchArc,
    /// Context names that traversed the edge.
    pub contexts: BTreeSet<String>,
}

/// Stable 128-bit fingerprint of source content.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceFingerprint(String);

impl SourceFingerprint {
    /// Fingerprints source bytes with fixed-key SipHash-128.
    pub fn from_bytes(source: &[u8]) -> Self {
        let mut hasher = SipHasher13::new_with_keys(0, 0);
        hasher.write(source);
        let hash: u128 = hasher.finish128().into();
        Self(format!("{hash:032x}"))
    }

    /// Returns the lowercase hexadecimal fingerprint.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether `source` has the content captured by this fingerprint.
    pub fn matches(&self, source: &[u8]) -> bool {
        *self == Self::from_bytes(source)
    }
}

#[derive(Deserialize)]
struct ArtifactHeader {
    format_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact() -> NativeCoverage {
        NativeCoverage::new(
            CoverageMode::Branch,
            Utf8PathBuf::from("/project"),
            BTreeSet::from([Utf8PathBuf::from("/project/src")]),
            Some("linux-py313".to_owned()),
            BTreeMap::from([(
                Utf8PathBuf::from("src/package.py"),
                NativeFileCoverage {
                    source_fingerprint: SourceFingerprint::from_bytes(b"if ready:\n    run()\n"),
                    executable: BTreeSet::from([1, 2]),
                    excluded: BTreeSet::from([2]),
                    executed: BTreeSet::from([1]),
                    line_contexts: BTreeMap::from([(
                        1,
                        BTreeSet::from(["tests/test_package.py::test_ready".to_owned()]),
                    )]),
                    branches: Some(NativeBranchCoverage {
                        possible: BTreeSet::from([
                            BranchArc { from: 1, to: 2 },
                            BranchArc { from: 1, to: 0 },
                        ]),
                        executed: BTreeSet::from([BranchArc { from: 1, to: 2 }]),
                        contexts: BTreeSet::from([NativeArcContexts {
                            arc: BranchArc { from: 1, to: 2 },
                            contexts: BTreeSet::from([
                                "tests/test_package.py::test_ready".to_owned()
                            ]),
                        }]),
                    }),
                },
            )]),
        )
    }

    #[test]
    fn artifact_round_trips_without_loss() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = Utf8PathBuf::from_path_buf(directory.path().join("coverage/data.json"))
            .expect("UTF-8 temp path");
        let expected = artifact();

        expected.write(&path).expect("write artifact");
        let actual = NativeCoverage::read(&path).expect("read artifact");

        assert_eq!(actual, expected);
    }

    #[test]
    fn artifact_schema_snapshot() {
        insta::assert_snapshot!(
            serde_json::to_string_pretty(&artifact()).expect("serialize artifact")
        );
    }

    #[test]
    fn equivalent_artifacts_serialize_identically() {
        let first = artifact();
        let mut second = artifact();
        let file = second
            .files
            .get_mut(Utf8Path::new("src/package.py"))
            .expect("fixture file");
        file.executable = [2, 1].into_iter().collect();
        file.line_contexts = BTreeMap::from([(
            1,
            BTreeSet::from(["tests/test_package.py::test_ready".to_owned()]),
        )]);

        assert_eq!(
            serde_json::to_vec_pretty(&first).expect("serialize first"),
            serde_json::to_vec_pretty(&second).expect("serialize second")
        );
    }

    #[test]
    fn unsupported_version_names_path_and_versions() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = Utf8PathBuf::from_path_buf(directory.path().join("data.json"))
            .expect("UTF-8 temp path");
        fs::write(&path, br#"{"format_version":2}"#).expect("write artifact");

        let error = NativeCoverage::read(&path).expect_err("reject future version");
        let message = error.to_string();

        assert!(message.contains(path.as_str()));
        assert!(message.contains("found format version 2"));
        assert!(message.contains("supported version is 1"));
    }

    #[test]
    fn source_fingerprint_detects_source_changes() {
        let original = SourceFingerprint::from_bytes(b"value = 1\n");
        let changed = SourceFingerprint::from_bytes(b"value = 2\n");

        assert_ne!(original, changed);
        assert_eq!(original.as_str().len(), 32);
    }

    #[test]
    fn source_verification_rejects_drift() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let project_root =
            Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).expect("UTF-8 temp path");
        fs::create_dir(project_root.join("src")).expect("create source directory");
        fs::write(project_root.join("src/package.py"), "value = 2\n").expect("write source");
        let mut coverage = artifact();
        coverage.project_root = project_root;

        let error = coverage
            .verify_sources()
            .expect_err("reject changed source");

        assert!(
            error
                .to_string()
                .contains("source changed since collection")
        );
    }
}
