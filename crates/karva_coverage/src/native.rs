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

use crate::context::{compose_context, prefix_context};
use crate::data::{BranchArc, WorkerFile};

/// Current native coverage schema version.
const FORMAT_VERSION: u32 = 4;

/// Karva coverage data retained after one or more test runs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NativeCoverage {
    /// Native artifact schema version.
    format_version: u32,
    /// Karva version that produced this artifact.
    karva_version: String,
    /// Whether collection measured statements alone or statements and branches.
    pub(super) mode: CoverageMode,
    /// Portable project-relative source roots, or absolute roots outside the project.
    pub source_roots: BTreeSet<Utf8PathBuf>,
    /// User-provided context attached to the whole run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) run_context: Option<String>,
    /// Coverage keyed by `/`-separated project-relative paths.
    ///
    /// Sources outside the project retain normalized absolute paths and require
    /// a report-time path alias when consumed from another machine.
    pub files: BTreeMap<Utf8PathBuf, NativeFileCoverage>,
}

impl NativeCoverage {
    /// Creates an artifact for the current schema and Karva version.
    pub fn new(
        mode: CoverageMode,
        source_roots: BTreeSet<Utf8PathBuf>,
        run_context: Option<String>,
        files: BTreeMap<Utf8PathBuf, NativeFileCoverage>,
    ) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            karva_version: env!("CARGO_PKG_VERSION").to_owned(),
            mode,
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
    #[cfg(test)]
    fn verify_sources(&self, project_root: &Utf8Path) -> Result<()> {
        for (source_path, coverage) in &self.files {
            let path = project_root.join(source_path);
            let source = fs::read(&path)
                .with_context(|| format!("failed to verify coverage source `{path}`"))?;
            if !coverage.source_fingerprint.matches(&source) {
                bail!("coverage source changed since collection: `{path}`");
            }
        }
        Ok(())
    }

    /// Builds one durable artifact by unioning transient worker payloads.
    pub fn from_worker_files(
        project_root: &Utf8Path,
        mode: CoverageMode,
        files: &[impl AsRef<Utf8Path>],
    ) -> Result<Self> {
        let canonical_root = dunce::canonicalize(project_root)
            .with_context(|| format!("failed to resolve coverage project root `{project_root}`"))?;
        let canonical_root = Utf8PathBuf::from_path_buf(canonical_root).map_err(|path| {
            anyhow::anyhow!(
                "coverage project root contains non-Unicode characters: `{}`",
                path.display()
            )
        })?;
        let mut native_files = BTreeMap::new();
        let mut source_roots = BTreeSet::new();

        for worker_path in files {
            let worker_path = worker_path.as_ref();
            let bytes = fs::read(worker_path)
                .with_context(|| format!("failed to read coverage file `{worker_path}`"))?;
            let worker: WorkerFile = serde_json::from_slice(&bytes)
                .with_context(|| format!("failed to parse coverage file `{worker_path}`"))?;
            source_roots.extend(worker.source_roots);
            for (source, file) in worker.files {
                merge_worker_file(
                    &canonical_root,
                    worker_path,
                    &source,
                    file,
                    mode,
                    &mut native_files,
                )?;
            }
        }

        Ok(Self::new(
            mode,
            normalize_source_roots(
                &canonical_root,
                &source_roots.into_iter().collect::<Vec<_>>(),
            )?,
            None,
            native_files,
        ))
    }

    /// Unions compatible observations into this artifact.
    pub fn merge(&mut self, mut other: Self) -> Result<()> {
        if self.mode != other.mode {
            bail!(
                "cannot append coverage collected in {:?} mode to coverage collected in {:?} mode",
                other.mode,
                self.mode
            );
        }
        if self.source_roots != other.source_roots {
            bail!("cannot append coverage collected from different source roots");
        }
        if self.run_context != other.run_context {
            self.materialize_run_context();
            other.materialize_run_context();
        }

        for (path, incoming) in other.files {
            if let Some(current) = self.files.get_mut(&path) {
                merge_native_file(&path, current, incoming)?;
            } else {
                self.files.insert(path, incoming);
            }
        }
        Ok(())
    }

    /// Moves a whole-run context onto every observed line and branch before merging runs.
    fn materialize_run_context(&mut self) {
        let Some(run_context) = self.run_context.take() else {
            return;
        };
        for file in self.files.values_mut() {
            for line in &file.executed {
                prefix_observation_contexts(
                    file.line_contexts.entry(*line).or_default(),
                    &run_context,
                );
            }
            if let Some(branches) = &mut file.branches {
                for arc in &branches.executed {
                    let mut entry = branches
                        .contexts
                        .iter()
                        .find(|entry| entry.arc == *arc)
                        .cloned()
                        .unwrap_or_else(|| NativeArcContexts {
                            arc: *arc,
                            contexts: BTreeSet::new(),
                        });
                    branches.contexts.remove(&entry);
                    prefix_observation_contexts(&mut entry.contexts, &run_context);
                    branches.contexts.insert(entry);
                }
            }
        }
    }

    /// Rewrites source identities and merges compatible paths that become equal.
    pub(super) fn map_paths(mut self, map: impl Fn(&str) -> String) -> Result<Self> {
        self.source_roots = self
            .source_roots
            .iter()
            .map(|path| Utf8PathBuf::from(map(path.as_str())))
            .collect();
        let mut files = BTreeMap::new();
        for (path, incoming) in std::mem::take(&mut self.files) {
            let mapped = Utf8PathBuf::from(map(path.as_str()));
            if let Some(current) = files.get_mut(&mapped) {
                merge_native_file(&mapped, current, incoming)?;
            } else {
                files.insert(mapped, incoming);
            }
        }
        self.files = files;
        Ok(self)
    }
}

fn prefix_observation_contexts(contexts: &mut BTreeSet<String>, run_context: &str) {
    if contexts.is_empty() {
        contexts.extend(compose_context(Some(run_context), &[]));
    } else {
        *contexts = contexts
            .iter()
            .map(|context| prefix_context(run_context, context))
            .collect();
    }
}

fn merge_worker_file(
    project_root: &Utf8Path,
    worker_path: &Utf8Path,
    source: &str,
    file: crate::data::FileEntry,
    mode: CoverageMode,
    files: &mut BTreeMap<Utf8PathBuf, NativeFileCoverage>,
) -> Result<()> {
    let source_path = dunce::canonicalize(source).with_context(|| {
        format!("coverage worker artifact `{worker_path}` references unreadable source `{source}`")
    })?;
    let source_path = Utf8PathBuf::from_path_buf(source_path).map_err(|path| {
        anyhow::anyhow!(
            "coverage source contains non-Unicode characters: `{}`",
            path.display()
        )
    })?;
    let stored_path = portable_path(
        source_path
            .strip_prefix(project_root)
            .unwrap_or(&source_path),
    );
    let source_bytes = fs::read(&source_path).with_context(|| {
        format!("coverage worker artifact `{worker_path}` references unreadable source `{source}`")
    })?;
    let fingerprint = SourceFingerprint::from_bytes(&source_bytes);
    let incoming = NativeFileCoverage {
        source_fingerprint: fingerprint,
        executable: file.executable.into_iter().collect(),
        excluded: file.excluded.into_iter().collect(),
        executed: file.executed.into_iter().collect(),
        line_contexts: file.contexts,
        branches: match (mode, file.branches) {
            (CoverageMode::Line, None) => None,
            (CoverageMode::Line, Some(_)) => {
                bail!(
                    "coverage worker artifact `{worker_path}` contains branch data for line-only collection"
                )
            }
            (CoverageMode::Branch, None) => {
                bail!("coverage worker artifact `{worker_path}` lacks branch data for `{source}`")
            }
            (CoverageMode::Branch, Some(branches)) => Some(NativeBranchCoverage {
                possible: branches.possible.into_iter().collect(),
                executed: branches.executed.into_iter().collect(),
                partial: branches.partial.into_iter().collect(),
                contexts: branches
                    .contexts
                    .into_iter()
                    .map(|entry| NativeArcContexts {
                        arc: entry.arc,
                        contexts: entry.contexts,
                    })
                    .collect(),
            }),
        },
    };

    if let Some(current) = files.get_mut(&stored_path) {
        merge_native_file(&stored_path, current, incoming)
    } else {
        files.insert(stored_path, incoming);
        Ok(())
    }
}

fn normalize_source_roots(
    project_root: &Utf8Path,
    source_roots: &[String],
) -> Result<BTreeSet<Utf8PathBuf>> {
    source_roots
        .iter()
        .map(|source| {
            let source = if source.is_empty() { "." } else { source };
            let path = Utf8Path::new(source);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                project_root.join(path)
            };
            let path = dunce::canonicalize(&path)
                .with_context(|| format!("failed to resolve coverage source root `{source}`"))?;
            let path = Utf8PathBuf::from_path_buf(path).map_err(|path| {
                anyhow::anyhow!(
                    "coverage source root contains non-Unicode characters: `{}`",
                    path.display()
                )
            })?;
            Ok(portable_path(
                path.strip_prefix(project_root).unwrap_or(&path),
            ))
        })
        .collect()
}

fn portable_path(path: &Utf8Path) -> Utf8PathBuf {
    Utf8PathBuf::from(path.as_str().replace('\\', "/"))
}

fn merge_native_file(
    path: &Utf8Path,
    current: &mut NativeFileCoverage,
    incoming: NativeFileCoverage,
) -> Result<()> {
    if current.source_fingerprint != incoming.source_fingerprint {
        bail!("cannot merge coverage for changed source `{path}`");
    }
    current.executable.extend(incoming.executable);
    current.excluded.extend(incoming.excluded);
    current.executed.extend(incoming.executed);
    for (line, contexts) in incoming.line_contexts {
        current
            .line_contexts
            .entry(line)
            .or_default()
            .extend(contexts);
    }
    match (&mut current.branches, incoming.branches) {
        (None, None) => {}
        (Some(current), Some(incoming)) => {
            current.possible.extend(incoming.possible);
            current.executed.extend(incoming.executed);
            current.partial.extend(incoming.partial);
            for entry in incoming.contexts {
                merge_arc_contexts(&mut current.contexts, entry);
            }
        }
        _ => bail!("cannot merge line and branch coverage for `{path}`"),
    }
    Ok(())
}

fn merge_arc_contexts(current: &mut BTreeSet<NativeArcContexts>, mut incoming: NativeArcContexts) {
    if let Some(existing) = current
        .iter()
        .find(|entry| entry.arc == incoming.arc)
        .cloned()
    {
        current.remove(&existing);
        incoming.contexts.extend(existing.contexts);
    }
    current.insert(incoming);
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
    pub(super) possible: BTreeSet<BranchArc>,
    /// Control-flow edges observed at runtime.
    pub(super) executed: BTreeSet<BranchArc>,
    /// Branch source lines whose unobserved destinations are intentional.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) partial: BTreeSet<u32>,
    /// Contexts grouped by executed edge, sorted by edge.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) contexts: BTreeSet<NativeArcContexts>,
}

/// Contexts that traversed one branch edge.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct NativeArcContexts {
    /// Executed control-flow edge.
    pub(super) arc: BranchArc,
    /// Context names that traversed the edge.
    pub(super) contexts: BTreeSet<String>,
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
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether `source` has the content captured by this fingerprint.
    pub(super) fn matches(&self, source: &[u8]) -> bool {
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
            BTreeSet::from([Utf8PathBuf::from("src")]),
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
                        partial: BTreeSet::new(),
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
    fn merge_preserves_different_run_contexts() {
        let mut first = artifact();
        let mut second = artifact();
        second.run_context = Some("windows-py313".to_owned());

        first.merge(second).expect("merge static contexts");

        assert_eq!(first.run_context, None);
        let file = first
            .files
            .get(Utf8Path::new("src/package.py"))
            .expect("fixture file");
        assert_eq!(
            file.line_contexts.get(&1),
            Some(&BTreeSet::from([
                "linux-py313|tests/test_package.py::test_ready".to_owned(),
                "windows-py313|tests/test_package.py::test_ready".to_owned(),
            ]))
        );
        let branch_contexts = &file
            .branches
            .as_ref()
            .expect("branch coverage")
            .contexts
            .first()
            .expect("branch context")
            .contexts;
        assert_eq!(
            branch_contexts,
            &BTreeSet::from([
                "linux-py313|tests/test_package.py::test_ready".to_owned(),
                "windows-py313|tests/test_package.py::test_ready".to_owned(),
            ])
        );
    }

    #[test]
    fn unsupported_version_names_path_and_versions() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = Utf8PathBuf::from_path_buf(directory.path().join("data.json"))
            .expect("UTF-8 temp path");
        fs::write(&path, br#"{"format_version":5}"#).expect("write artifact");

        let error = NativeCoverage::read(&path).expect_err("reject future version");
        let message = error.to_string();

        assert!(message.contains(path.as_str()));
        assert!(message.contains("found format version 5"));
        assert!(message.contains("supported version is 4"));
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
        let coverage = artifact();

        let error = coverage
            .verify_sources(&project_root)
            .expect_err("reject changed source");

        assert!(
            error
                .to_string()
                .contains("source changed since collection")
        );
    }

    #[test]
    fn worker_artifacts_and_appended_runs_union_observations() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let project_root =
            Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).expect("UTF-8 temp path");
        let source = project_root.join("app.py");
        fs::write(&source, "first = 1\nsecond = 2\n").expect("write source");
        let first_worker = project_root.join("first-worker.json");
        let second_worker = project_root.join("second-worker.json");
        let worker = |executed, context: &str| WorkerFile {
            source_roots: BTreeSet::from([source.to_string()]),
            files: BTreeMap::from([(
                source.to_string(),
                crate::data::FileEntry {
                    executable: vec![1, 2],
                    excluded: Vec::new(),
                    executed: vec![executed],
                    contexts: BTreeMap::from([(executed, BTreeSet::from([context.to_owned()]))]),
                    branches: None,
                },
            )]),
        };
        fs::write(
            &first_worker,
            serde_json::to_vec(&worker(1, "first")).expect("serialize worker"),
        )
        .expect("write first worker");
        fs::write(
            &second_worker,
            serde_json::to_vec(&worker(2, "second")).expect("serialize worker"),
        )
        .expect("write second worker");

        let mut first =
            NativeCoverage::from_worker_files(&project_root, CoverageMode::Line, &[first_worker])
                .expect("build first artifact");
        let second =
            NativeCoverage::from_worker_files(&project_root, CoverageMode::Line, &[second_worker])
                .expect("build second artifact");

        first.merge(second).expect("append compatible artifact");

        let file = first.files.get(Utf8Path::new("app.py")).expect("app data");
        assert_eq!(file.executed, BTreeSet::from([1, 2]));
        assert_eq!(
            file.line_contexts,
            BTreeMap::from([
                (1, BTreeSet::from(["first".to_owned()])),
                (2, BTreeSet::from(["second".to_owned()])),
            ])
        );
    }
}
