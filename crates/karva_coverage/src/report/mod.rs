//! Combine per-worker JSON files and produce terminal or machine-readable reports.

pub(crate) mod html;
pub(crate) mod json;
pub(crate) mod shared;
mod terminal;
pub(crate) mod xml;

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::RegexSet;

pub use terminal::combine_and_report;
pub use terminal::write_cobertura_xml;
pub use terminal::write_html_report;
pub use terminal::write_json_report;

use self::shared::{FileRow, combine, combine_native, total_percent, verify_combined_sources};

#[derive(Debug, Default)]
/// File globs and context regexes applied before coverage metrics are calculated.
pub struct CoverageFilters {
    include: Option<GlobSet>,
    omit: Option<GlobSet>,
    contexts: Option<RegexSet>,
}

impl CoverageFilters {
    /// Compiles coverage path filters, reporting invalid glob syntax.
    pub fn new(include: &[String], omit: &[String]) -> Result<Self> {
        Ok(Self {
            include: compile_globs("include", include)?,
            omit: compile_globs("omit", omit)?,
            contexts: None,
        })
    }

    /// Adds context patterns used to select executed lines and branches.
    pub fn with_contexts(mut self, contexts: &[String]) -> Result<Self> {
        self.contexts = compile_contexts(contexts)?;
        Ok(self)
    }

    fn matches(&self, path: &str) -> bool {
        self.include
            .as_ref()
            .is_none_or(|include| include.is_match(path))
            && !self.omit.as_ref().is_some_and(|omit| omit.is_match(path))
    }

    fn has_contexts(&self) -> bool {
        self.contexts.is_some()
    }

    fn matches_context(&self, context: &str) -> bool {
        self.contexts
            .as_ref()
            .is_none_or(|contexts| contexts.is_match(context))
    }
}

fn compile_contexts(patterns: &[String]) -> Result<Option<RegexSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    RegexSet::new(patterns)
        .map(Some)
        .map_err(|error| anyhow::anyhow!("invalid coverage context pattern: {error}"))
}

/// Validated, filtered coverage metrics shared by every report renderer.
#[derive(Debug)]
pub struct CoverageAnalysis {
    coverage_root: Utf8PathBuf,
    cwd_real: std::path::PathBuf,
    rows: Vec<FileRow>,
}

impl CoverageAnalysis {
    /// Loads and analyzes durable native coverage artifacts.
    pub fn load_native(
        project_root: &Utf8Path,
        files: &[impl AsRef<Utf8Path>],
        filters: &CoverageFilters,
    ) -> Result<Option<Self>> {
        let Some(combined) = combine_native(files, filters)? else {
            return Ok(None);
        };
        if combined.is_empty() {
            return Ok(None);
        }
        verify_combined_sources(project_root, &combined)?;
        let coverage_root = project_root.to_path_buf();
        let cwd_real = canonical_root(&coverage_root);
        let rows = shared::build_rows(&cwd_real, &combined, true)
            .into_iter()
            .filter(|row| filters.matches(&row.name))
            .collect();
        Ok(Some(Self {
            coverage_root,
            cwd_real,
            rows,
        }))
    }

    /// Returns the combined statement-and-branch coverage percentage.
    pub fn total_percent(&self) -> f64 {
        total_percent(&self.rows)
    }

    /// Returns the number of source files retained by the filters.
    pub fn file_count(&self) -> usize {
        self.rows.len()
    }
}

fn compile_globs(kind: &str, patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }

    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            Glob::new(pattern).map_err(|err| {
                anyhow::anyhow!("invalid coverage {kind} glob `{pattern}`: {err}")
            })?,
        );
    }
    Ok(Some(builder.build()?))
}

fn combined_rows(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    filters: &CoverageFilters,
) -> Result<Option<CoverageAnalysis>> {
    let combined = combine(files)?;
    if combined.is_empty() {
        return Ok(None);
    }

    let cwd_real = canonical_root(cwd);
    let rows = shared::build_rows(&cwd_real, &combined, true)
        .into_iter()
        .filter(|row| filters.matches(&row.name))
        .collect();
    Ok(Some(CoverageAnalysis {
        coverage_root: cwd.to_path_buf(),
        cwd_real,
        rows,
    }))
}

fn canonical_root(cwd: &Utf8Path) -> std::path::PathBuf {
    fs::canonicalize(cwd.as_std_path())
        .map(|path| dunce::simplified(&path).to_path_buf())
        .unwrap_or_else(|_| cwd.into())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use camino::{Utf8Path, Utf8PathBuf};
    use fs_err as fs;

    use super::{CoverageAnalysis, CoverageFilters};
    use crate::data::BranchArc;
    use crate::native::{
        CoverageMode, NativeBranchCoverage, NativeCoverage, NativeFileCoverage, SourceFingerprint,
    };

    const SOURCE: &[u8] = b"first = 1\nsecond = 2\n";

    fn write_artifact(
        directory: &tempfile::TempDir,
        name: &str,
        mode: CoverageMode,
        executed: BTreeSet<u32>,
        contexts: BTreeMap<u32, BTreeSet<String>>,
    ) -> Utf8PathBuf {
        let project_root =
            Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).expect("UTF-8 temp path");
        fs::create_dir_all(project_root.join("src")).expect("create source directory");
        fs::write(project_root.join("src/app.py"), SOURCE).expect("write source");
        let branches = (mode == CoverageMode::Branch).then(|| NativeBranchCoverage {
            possible: BTreeSet::from([BranchArc { from: 1, to: 2 }, BranchArc { from: 1, to: 0 }]),
            executed: BTreeSet::new(),
            contexts: BTreeSet::new(),
        });
        let artifact = NativeCoverage::new(
            mode,
            project_root.clone(),
            BTreeSet::from([Utf8PathBuf::from("src")]),
            None,
            BTreeMap::from([(
                Utf8PathBuf::from("src/app.py"),
                NativeFileCoverage {
                    source_fingerprint: SourceFingerprint::from_bytes(SOURCE),
                    executable: BTreeSet::from([1, 2]),
                    excluded: BTreeSet::new(),
                    executed,
                    line_contexts: contexts,
                    branches,
                },
            )]),
        );
        let path = project_root.join(name);
        artifact.write(&path).expect("write native artifact");
        path
    }

    #[test]
    fn filters_apply_include_then_omit() {
        let include = vec!["src/**".to_string()];
        let omit = vec!["**/generated.py".to_string()];
        let filters = CoverageFilters::new(&include, &omit).expect("valid filters");

        assert!(filters.matches("src/package/module.py"));
        assert!(!filters.matches("tests/test_module.py"));
        assert!(!filters.matches("src/package/generated.py"));
    }

    #[test]
    fn filters_reject_invalid_globs() {
        let include = vec!["[".to_string()];
        let err = CoverageFilters::new(&include, &[]).expect_err("invalid glob");

        assert!(
            err.to_string()
                .contains("invalid coverage include glob `[`"),
            "{err:?}"
        );
    }

    #[test]
    fn filters_reject_invalid_context_patterns() {
        let error = CoverageFilters::new(&[], &[])
            .expect("empty file filters")
            .with_contexts(&["[".to_owned()])
            .expect_err("invalid context pattern");

        assert!(
            error
                .to_string()
                .contains("invalid coverage context pattern")
        );
    }

    #[test]
    fn context_filters_search_qualified_names() {
        let filters = CoverageFilters::new(&[], &[])
            .expect("empty file filters")
            .with_contexts(&["test_example".to_owned()])
            .expect("valid context filter");

        assert!(filters.matches_context("tests/test_app.py::test_example[param]"));
        assert!(!filters.matches_context("tests/test_app.py::test_other"));
    }

    #[test]
    fn native_merge_order_does_not_change_analysis() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let first = write_artifact(
            &directory,
            "first.json",
            CoverageMode::Line,
            BTreeSet::from([1]),
            BTreeMap::from([(1, BTreeSet::from(["test_first".to_owned()]))]),
        );
        let second = write_artifact(
            &directory,
            "second.json",
            CoverageMode::Line,
            BTreeSet::from([2]),
            BTreeMap::from([(2, BTreeSet::from(["test_second".to_owned()]))]),
        );
        let filters = CoverageFilters::new(&[], &[]).expect("empty filters");

        let project_root = first.parent().expect("project root");
        let forward = CoverageAnalysis::load_native(project_root, &[&first, &second], &filters)
            .expect("analyze forward")
            .expect("coverage data");
        let reverse = CoverageAnalysis::load_native(project_root, &[&second, &first], &filters)
            .expect("analyze reverse")
            .expect("coverage data");

        assert_eq!(forward.rows, reverse.rows);
    }

    #[test]
    fn native_context_filter_recalculates_coverage() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = write_artifact(
            &directory,
            "data.json",
            CoverageMode::Line,
            BTreeSet::from([1, 2]),
            BTreeMap::from([
                (1, BTreeSet::from(["python=3.14".to_owned()])),
                (2, BTreeSet::from(["python=3.13".to_owned()])),
            ]),
        );
        let filters = CoverageFilters::new(&[], &[])
            .expect("empty file filters")
            .with_contexts(&["python=3.14".to_owned()])
            .expect("valid context filter");

        let project_root = path.parent().expect("project root").to_path_buf();
        let analysis = CoverageAnalysis::load_native(&project_root, &[path], &filters)
            .expect("analyze coverage")
            .expect("coverage data");

        assert_eq!(analysis.rows[0].hit, 1);
        assert_eq!(analysis.rows[0].stmts, 2);
    }

    #[test]
    fn native_loader_uses_current_checkout_for_sources() {
        let collected = tempfile::tempdir().expect("create collection directory");
        let path = write_artifact(
            &collected,
            "data.json",
            CoverageMode::Line,
            BTreeSet::from([1]),
            BTreeMap::new(),
        );
        let current = tempfile::tempdir().expect("create current checkout");
        let current_root = Utf8PathBuf::from_path_buf(current.path().to_path_buf())
            .expect("UTF-8 current checkout");
        fs::create_dir(current_root.join("src")).expect("create current source directory");
        fs::write(current_root.join("src/app.py"), SOURCE).expect("write current source");
        let filters = CoverageFilters::new(&[], &[]).expect("empty filters");

        let analysis = CoverageAnalysis::load_native(&current_root, &[path], &filters)
            .expect("analyze in current checkout")
            .expect("coverage data");

        assert_eq!(analysis.file_count(), 1);
    }

    #[test]
    fn native_analysis_removes_excluded_statements() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = write_artifact(
            &directory,
            "data.json",
            CoverageMode::Line,
            BTreeSet::from([1, 2]),
            BTreeMap::new(),
        );
        let mut artifact = NativeCoverage::read(&path).expect("read native artifact");
        artifact
            .files
            .get_mut(Utf8Path::new("src/app.py"))
            .expect("fixture file")
            .excluded
            .insert(2);
        artifact.write(&path).expect("rewrite native artifact");
        let filters = CoverageFilters::new(&[], &[]).expect("empty filters");
        let project_root = path.parent().expect("project root").to_path_buf();

        let analysis = CoverageAnalysis::load_native(&project_root, &[path], &filters)
            .expect("analyze coverage")
            .expect("coverage data");

        assert_eq!(analysis.rows[0].stmts, 1);
        assert_eq!(analysis.rows[0].hit, 1);
        assert_eq!(analysis.rows[0].excluded, vec![2]);
    }

    #[test]
    fn native_loader_rejects_mixed_collection_modes() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let line = write_artifact(
            &directory,
            "line.json",
            CoverageMode::Line,
            BTreeSet::new(),
            BTreeMap::new(),
        );
        let branch = write_artifact(
            &directory,
            "branch.json",
            CoverageMode::Branch,
            BTreeSet::new(),
            BTreeMap::new(),
        );
        let filters = CoverageFilters::new(&[], &[]).expect("empty filters");

        let project_root = line.parent().expect("project root");
        let error =
            CoverageAnalysis::load_native(project_root, &[line.clone(), branch.clone()], &filters)
                .expect_err("reject mixed modes");

        assert!(error.to_string().contains(branch.as_str()));
        assert!(error.to_string().contains("expected collection mode Line"));
        assert!(error.to_string().contains("found Branch"));
    }

    #[test]
    fn native_loader_rejects_different_source_roots() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let first = write_artifact(
            &directory,
            "first.json",
            CoverageMode::Line,
            BTreeSet::new(),
            BTreeMap::new(),
        );
        let second = write_artifact(
            &directory,
            "second.json",
            CoverageMode::Line,
            BTreeSet::new(),
            BTreeMap::new(),
        );
        let mut artifact = NativeCoverage::read(&second).expect("read second artifact");
        artifact.source_roots = BTreeSet::from([Utf8PathBuf::from("lib")]);
        artifact.write(&second).expect("rewrite second artifact");
        let filters = CoverageFilters::new(&[], &[]).expect("empty filters");
        let project_root = first.parent().expect("project root").to_path_buf();

        let error =
            CoverageAnalysis::load_native(&project_root, &[first, second.clone()], &filters)
                .expect_err("reject different source roots");

        assert!(error.to_string().contains(second.as_str()));
        assert!(error.to_string().contains("source-root identity"));
    }

    #[test]
    fn native_loader_reports_source_drift_with_input_path() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = write_artifact(
            &directory,
            "data.json",
            CoverageMode::Line,
            BTreeSet::new(),
            BTreeMap::new(),
        );
        fs::write(
            path.parent().expect("project root").join("src/app.py"),
            b"changed\n",
        )
        .expect("change source");
        let filters = CoverageFilters::new(&[], &[]).expect("empty filters");

        let project_root = path.parent().expect("project root");
        let error =
            CoverageAnalysis::load_native(project_root, std::slice::from_ref(&path), &filters)
                .expect_err("reject source drift");
        let message = format!("{error:#}");

        assert!(message.contains(path.as_str()));
        assert!(message.contains("expected unchanged source"));
        assert!(message.contains("fingerprint"));
    }

    #[test]
    fn native_loader_reports_corrupt_input_path() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let project_root =
            Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).expect("UTF-8 temp path");
        let path = project_root.join("corrupt.json");
        fs::write(&path, "{}").expect("write corrupt artifact");
        let filters = CoverageFilters::new(&[], &[]).expect("empty filters");

        let error =
            CoverageAnalysis::load_native(&project_root, std::slice::from_ref(&path), &filters)
                .expect_err("reject corrupt artifact");

        assert!(error.to_string().contains(path.as_str()));
        assert!(error.to_string().contains("failed to parse"));
    }
}
