use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;

use crate::context::{compose_context, prefix_context};
use crate::data::{BranchArc, WorkerFile};
use crate::native::{CoverageMode, NativeCoverage, SourceFingerprint};
use crate::report::CoverageFilters;

#[derive(Debug, Default)]
/// Union of raw coverage observations for one normalized source path.
pub(super) struct CombinedFile {
    source_fingerprint: Option<SourceFingerprint>,
    fingerprint_artifact: Option<Utf8PathBuf>,
    executable: BTreeSet<u32>,
    excluded: BTreeSet<u32>,
    executed: BTreeSet<u32>,
    contexts: BTreeMap<u32, BTreeSet<String>>,
    branches_enabled: bool,
    branch_possible: BTreeSet<BranchArc>,
    branch_executed: BTreeSet<BranchArc>,
    branch_partial_lines: BTreeSet<u32>,
    arc_contexts: BTreeMap<BranchArc, BTreeSet<String>>,
}

/// Report-ready metrics and source data shared by every output format.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FileRow {
    /// Display path, relative to the coverage root when possible.
    pub name: String,

    /// Canonical source path used by machine-readable formats.
    pub absolute_name: String,

    pub stmts: u32,
    pub hit: u32,
    pub miss: u32,
    pub missing: String,
    pub executable: Vec<u32>,
    pub excluded: Vec<u32>,
    pub executed: Vec<u32>,
    pub contexts: BTreeMap<u32, BTreeSet<String>>,

    /// Whether branch metrics should appear, even when this file has no branches.
    pub branches_enabled: bool,

    pub branches: u32,
    pub branch_hit: u32,
    pub branch_miss: u32,
    pub branch_partial: u32,
    pub branch_possible: Vec<BranchArc>,
    pub branch_executed: Vec<BranchArc>,
    pub branch_missing: Vec<BranchArc>,
    pub arc_contexts: BTreeMap<BranchArc, BTreeSet<String>>,
}

/// Unions per-worker payloads so each source path has one coverage record.
pub(super) fn combine(files: &[impl AsRef<Utf8Path>]) -> Result<BTreeMap<String, CombinedFile>> {
    let mut combined: BTreeMap<String, CombinedFile> = BTreeMap::new();

    for path in files {
        let path = path.as_ref();
        let bytes =
            fs::read(path).with_context(|| format!("failed to read coverage file {path}"))?;
        let parsed: WorkerFile = serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse coverage file {path}"))?;

        for (filename, file_entry) in parsed.files {
            let bucket = combined.entry(filename).or_default();
            bucket.executable.extend(file_entry.executable);
            bucket.executed.extend(file_entry.executed);
            for (line, contexts) in file_entry.contexts {
                bucket.contexts.entry(line).or_default().extend(contexts);
            }
            if let Some(branches) = file_entry.branches {
                bucket.branches_enabled = true;
                bucket.branch_possible.extend(branches.possible);
                bucket.branch_executed.extend(branches.executed);
                bucket.branch_partial_lines.extend(branches.partial);
                for entry in branches.contexts {
                    bucket
                        .arc_contexts
                        .entry(entry.arc)
                        .or_default()
                        .extend(entry.contexts);
                }
            }
        }
    }

    Ok(combined)
}

/// Loads, validates, and deterministically unions durable native artifacts.
pub(super) fn combine_native(
    files: &[impl AsRef<Utf8Path>],
    filters: &CoverageFilters,
) -> Result<Option<BTreeMap<String, CombinedFile>>> {
    let Some(first_path) = files.first().map(AsRef::as_ref) else {
        return Ok(None);
    };
    let first = NativeCoverage::read(first_path)?;
    let mut combined = BTreeMap::new();
    merge_native(&mut combined, first_path, &first, filters)?;

    for path in &files[1..] {
        let path = path.as_ref();
        let artifact = NativeCoverage::read(path)?;
        validate_identity(first_path, &first, path, &artifact, filters)?;
        merge_native(&mut combined, path, &artifact, filters)?;
    }

    Ok(Some(combined))
}

fn validate_identity(
    expected_path: &Utf8Path,
    expected: &NativeCoverage,
    path: &Utf8Path,
    artifact: &NativeCoverage,
    filters: &CoverageFilters,
) -> Result<()> {
    if artifact.mode != expected.mode {
        anyhow::bail!(
            "incompatible native coverage artifact `{path}`: expected collection mode {:?} from `{expected_path}`, found {:?}",
            expected.mode,
            artifact.mode
        );
    }
    let expected_roots: BTreeSet<String> = expected
        .source_roots
        .iter()
        .map(|root| filters.map_path(root.as_str()))
        .collect();
    let artifact_roots: BTreeSet<String> = artifact
        .source_roots
        .iter()
        .map(|root| filters.map_path(root.as_str()))
        .collect();
    if artifact_roots != expected_roots {
        anyhow::bail!(
            "incompatible native coverage artifact `{path}`: expected source-root identity from `{expected_path}`"
        );
    }
    Ok(())
}

fn merge_native(
    combined: &mut BTreeMap<String, CombinedFile>,
    path: &Utf8Path,
    artifact: &NativeCoverage,
    filters: &CoverageFilters,
) -> Result<()> {
    for (source_path, file) in &artifact.files {
        let mapped_path = filters.map_path(source_path.as_str());
        let bucket = combined.entry(mapped_path.clone()).or_default();
        if let Some(expected) = &bucket.source_fingerprint
            && expected != &file.source_fingerprint
        {
            anyhow::bail!(
                "coverage path collision in native artifact `{path}`: `{source_path}` maps to `{mapped_path}` with fingerprint {}, expected {}",
                file.source_fingerprint.as_str(),
                expected.as_str(),
            );
        }
        bucket.source_fingerprint = Some(file.source_fingerprint.clone());
        bucket
            .fingerprint_artifact
            .get_or_insert_with(|| path.to_path_buf());
        bucket.executable.extend(&file.executable);
        bucket.excluded.extend(&file.excluded);
        bucket.branches_enabled = artifact.mode == CoverageMode::Branch;
        merge_line_observations(bucket, file, artifact.run_context.as_deref(), filters);
        if let Some(branches) = &file.branches {
            bucket.branches_enabled = true;
            bucket.branch_possible.extend(&branches.possible);
            bucket.branch_partial_lines.extend(&branches.partial);
            merge_branch_observations(bucket, branches, artifact.run_context.as_deref(), filters);
        }
    }
    Ok(())
}

/// Verifies combined source fingerprints against the current project checkout.
pub(super) fn verify_combined_sources(
    project_root: &Utf8Path,
    combined: &BTreeMap<String, CombinedFile>,
) -> Result<()> {
    for (source_path, file) in combined {
        let path = project_root.join(source_path);
        let artifact = file
            .fingerprint_artifact
            .as_deref()
            .unwrap_or_else(|| Utf8Path::new("<unknown>"));
        let source = fs::read(&path).with_context(|| {
            format!("native coverage artifact `{artifact}` expected source `{path}` to exist")
        })?;
        if let Some(fingerprint) = &file.source_fingerprint
            && !fingerprint.matches(&source)
        {
            anyhow::bail!(
                "native coverage artifact `{artifact}` expected unchanged source `{path}` with fingerprint {}",
                fingerprint.as_str()
            );
        }
    }
    Ok(())
}

fn merge_line_observations(
    bucket: &mut CombinedFile,
    file: &crate::native::NativeFileCoverage,
    run_context: Option<&str>,
    filters: &CoverageFilters,
) {
    for line in &file.executed {
        let contexts = observation_contexts(run_context, file.line_contexts.get(line));
        let matching: BTreeSet<String> = contexts
            .iter()
            .filter(|context| filters.matches_context(context))
            .cloned()
            .collect();
        if !filters.has_contexts() || !matching.is_empty() {
            bucket.executed.insert(*line);
        }
        if !matching.is_empty() {
            bucket.contexts.entry(*line).or_default().extend(matching);
        }
    }
}

fn merge_branch_observations(
    bucket: &mut CombinedFile,
    branches: &crate::native::NativeBranchCoverage,
    run_context: Option<&str>,
    filters: &CoverageFilters,
) {
    let dynamic_by_arc: BTreeMap<BranchArc, &BTreeSet<String>> = branches
        .contexts
        .iter()
        .map(|entry| (entry.arc, &entry.contexts))
        .collect();
    for arc in &branches.executed {
        let dynamic = dynamic_by_arc.get(arc).copied();
        let contexts = observation_contexts(run_context, dynamic);
        let matching: BTreeSet<String> = contexts
            .iter()
            .filter(|context| filters.matches_context(context))
            .cloned()
            .collect();
        if !filters.has_contexts() || !matching.is_empty() {
            bucket.branch_executed.insert(*arc);
        }
        if !matching.is_empty() {
            bucket
                .arc_contexts
                .entry(*arc)
                .or_default()
                .extend(matching);
        }
    }
}

fn observation_contexts(
    run_context: Option<&str>,
    dynamic: Option<&BTreeSet<String>>,
) -> BTreeSet<String> {
    match (run_context, dynamic) {
        (Some(run_context), Some(dynamic)) if !dynamic.is_empty() => dynamic
            .iter()
            .map(|context| prefix_context(run_context, context))
            .collect(),
        (Some(run_context), _) => compose_context(Some(run_context), &[])
            .into_iter()
            .collect(),
        (None, Some(dynamic)) => dynamic.clone(),
        (None, None) => BTreeSet::new(),
    }
}

/// Converts combined observations into normalized metrics consumed by report writers.
pub(super) fn build_rows(
    cwd_real: &std::path::Path,
    combined: &BTreeMap<String, CombinedFile>,
    show_missing: bool,
) -> Vec<FileRow> {
    combined
        .iter()
        .map(|(filename, data)| {
            let absolute_name = simplify_path(filename);
            let executable_set: BTreeSet<u32> = data
                .executable
                .difference(&data.excluded)
                .copied()
                .collect();
            let executed_set: BTreeSet<u32> = data
                .executed
                .intersection(&executable_set)
                .copied()
                .collect();
            let executable: Vec<u32> = executable_set.iter().copied().collect();
            let excluded: Vec<u32> = data.excluded.iter().copied().collect();
            let executed: Vec<u32> = executed_set.iter().copied().collect();
            let stmts = u32::try_from(executable.len()).unwrap_or(u32::MAX);
            let hit = u32::try_from(executed.len()).unwrap_or(u32::MAX);
            let miss = stmts.saturating_sub(hit);
            let branch_executed: BTreeSet<BranchArc> = data
                .branch_executed
                .intersection(&data.branch_possible)
                .copied()
                .collect();
            let branch_missing: BTreeSet<BranchArc> = data
                .branch_possible
                .difference(&branch_executed)
                .filter(|arc| {
                    u32::try_from(arc.from)
                        .ok()
                        .is_none_or(|line| !data.branch_partial_lines.contains(&line))
                })
                .copied()
                .collect();
            let branches = u32::try_from(data.branch_possible.len()).unwrap_or(u32::MAX);
            let branch_hit =
                branches.saturating_sub(u32::try_from(branch_missing.len()).unwrap_or(u32::MAX));
            let branch_miss = branches.saturating_sub(branch_hit);
            let effective_executed = data
                .branch_possible
                .difference(&branch_missing)
                .copied()
                .collect();
            let branch_partial = partial_branch_count(&data.branch_possible, &effective_executed);
            let missing = if show_missing {
                let uncovered: BTreeSet<u32> =
                    executable_set.difference(&executed_set).copied().collect();
                collapse_missing(&uncovered, &branch_missing)
            } else {
                String::new()
            };
            FileRow {
                name: display_path(&absolute_name, cwd_real),
                absolute_name,
                stmts,
                hit,
                miss,
                missing,
                executable,
                excluded,
                executed,
                contexts: data.contexts.clone(),
                branches_enabled: data.branches_enabled,
                branches,
                branch_hit,
                branch_miss,
                branch_partial,
                branch_possible: data.branch_possible.iter().copied().collect(),
                branch_executed: branch_executed.iter().copied().collect(),
                branch_missing: branch_missing.iter().copied().collect(),
                arc_contexts: data.arc_contexts.clone(),
            }
        })
        .collect()
}

pub(super) fn total_percent(rows: &[FileRow]) -> f64 {
    let total_stmts = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.stmts));
    let total_miss = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.miss));
    let total_branches = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.branches));
    let total_branch_miss = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.branch_miss));
    percent(
        total_stmts.saturating_add(total_branches),
        total_miss.saturating_add(total_branch_miss),
    )
}

pub(super) fn totals_row(rows: &[FileRow]) -> FileRow {
    let stmts = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.stmts));
    let hit = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.hit));
    let miss = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.miss));
    let missing = rows.iter().flat_map(missing_lines).collect::<Vec<_>>();
    let branches = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.branches));
    let branch_hit = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.branch_hit));
    let branch_miss = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.branch_miss));
    let branch_partial = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.branch_partial));
    FileRow {
        name: "TOTAL".to_string(),
        absolute_name: String::new(),
        stmts,
        hit,
        miss,
        missing: collapse_ranges(&missing.iter().copied().collect()),
        executable: Vec::new(),
        excluded: Vec::new(),
        executed: Vec::new(),
        contexts: BTreeMap::new(),
        branches_enabled: rows.iter().any(|row| row.branches_enabled),
        branches,
        branch_hit,
        branch_miss,
        branch_partial,
        branch_possible: Vec::new(),
        branch_executed: Vec::new(),
        branch_missing: Vec::new(),
        arc_contexts: BTreeMap::new(),
    }
}

pub(super) fn missing_lines(row: &FileRow) -> Vec<u32> {
    let executed: BTreeSet<u32> = row.executed.iter().copied().collect();
    row.executable
        .iter()
        .copied()
        .filter(|line| !executed.contains(line))
        .collect()
}

pub(super) fn class_filename(row: &FileRow, cwd_real: &std::path::Path) -> String {
    if let Ok(relative) = std::path::Path::new(&row.absolute_name).strip_prefix(cwd_real) {
        normalize_report_path(&relative.to_string_lossy())
    } else {
        normalize_report_path(&row.absolute_name)
    }
}

pub(super) fn percent(total: u32, miss: u32) -> f64 {
    if total == 0 {
        return 100.0;
    }
    let hit = total - miss.min(total);
    f64::from(hit) / f64::from(total) * 100.0
}

pub(super) fn rate(hit: u32, total: u32) -> f64 {
    if total == 0 {
        1.0
    } else {
        f64::from(hit) / f64::from(total)
    }
}

pub(super) fn row_percent(row: &FileRow) -> f64 {
    percent(
        row.stmts.saturating_add(row.branches),
        row.miss.saturating_add(row.branch_miss),
    )
}

pub(super) fn collapse_ranges(lines: &BTreeSet<u32>) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut iter = lines.iter().copied();
    let Some(mut start) = iter.next() else {
        return String::new();
    };
    let mut end = start;
    for line in iter {
        if line != end + 1 {
            parts.push(format_range(start, end));
            start = line;
        }
        end = line;
    }
    parts.push(format_range(start, end));
    parts.join(", ")
}

fn format_range(start: u32, end: u32) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    }
}

fn partial_branch_count(possible: &BTreeSet<BranchArc>, executed: &BTreeSet<BranchArc>) -> u32 {
    let mut by_source: BTreeMap<i32, (u32, u32)> = BTreeMap::new();
    for arc in possible {
        let entry = by_source.entry(arc.from).or_default();
        entry.0 = entry.0.saturating_add(1);
        if executed.contains(arc) {
            entry.1 = entry.1.saturating_add(1);
        }
    }
    u32::try_from(
        by_source
            .values()
            .filter(|(possible, executed)| *executed > 0 && executed < possible)
            .count(),
    )
    .unwrap_or(u32::MAX)
}

fn collapse_missing(lines: &BTreeSet<u32>, branches: &BTreeSet<BranchArc>) -> String {
    let mut parts = Vec::new();
    let lines_collapsed = collapse_ranges(lines);
    if !lines_collapsed.is_empty() {
        parts.push(lines_collapsed);
    }
    parts.extend(
        branches
            .iter()
            .copied()
            .filter(|arc| arc.to <= 0 || !lines.contains(&(u32::try_from(arc.to).unwrap_or(0))))
            .map(format_branch_arc),
    );
    parts.join(", ")
}

pub(super) fn format_branch_arc(arc: BranchArc) -> String {
    let to = if arc.to < 0 {
        "exit".to_string()
    } else {
        arc.to.to_string()
    };
    format!("{}->{to}", arc.from)
}

fn simplify_path(path: &str) -> String {
    dunce::simplified(std::path::Path::new(path))
        .to_string_lossy()
        .into_owned()
}

fn display_path(absolute: &str, cwd: &std::path::Path) -> String {
    let path = if let Ok(rel) = std::path::Path::new(absolute).strip_prefix(cwd) {
        rel.to_string_lossy().into_owned()
    } else {
        absolute.to_string()
    };
    normalize_report_path(&path)
}

fn normalize_report_path(path: &str) -> String {
    path.replace('\\', "/")
}

pub(super) fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub(super) fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_full_coverage() {
        assert!((percent(10, 0) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percent_partial() {
        assert!((percent(10, 3) - 70.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percent_zero_stmts() {
        assert!((percent(0, 0) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn partial_branch_lines_suppress_only_missing_arcs() {
        let possible = BTreeSet::from([BranchArc { from: 1, to: 2 }, BranchArc { from: 1, to: 3 }]);
        let executed = BTreeSet::from([BranchArc { from: 1, to: 2 }]);
        let combined = BTreeMap::from([(
            "app.py".to_owned(),
            CombinedFile {
                executable: BTreeSet::from([1, 2, 3]),
                executed: BTreeSet::from([1, 2]),
                branches_enabled: true,
                branch_possible: possible,
                branch_executed: executed,
                branch_partial_lines: BTreeSet::from([1]),
                ..CombinedFile::default()
            },
        )]);

        let rows = build_rows(std::path::Path::new("."), &combined, true);
        let row = rows.first().expect("coverage row");

        assert_eq!(row.branches, 2);
        assert_eq!(row.branch_hit, 2);
        assert_eq!(row.branch_miss, 0);
        assert_eq!(row.branch_partial, 0);
        assert_eq!(row.branch_executed, vec![BranchArc { from: 1, to: 2 }]);
        assert!(row.branch_missing.is_empty());
    }

    #[test]
    fn collapse_empty() {
        let set: BTreeSet<u32> = BTreeSet::new();
        assert_eq!(collapse_ranges(&set), "");
    }

    #[test]
    fn collapse_singletons() {
        let set: BTreeSet<u32> = [3, 7, 12].into_iter().collect();
        assert_eq!(collapse_ranges(&set), "3, 7, 12");
    }

    #[test]
    fn collapse_mixed_ranges() {
        let set: BTreeSet<u32> = [26, 87, 94, 95, 119, 120, 121, 157].into_iter().collect();
        assert_eq!(collapse_ranges(&set), "26, 87, 94-95, 119-121, 157");
    }

    #[test]
    fn collapse_single_contiguous_range() {
        let set: BTreeSet<u32> = [10, 11, 12, 13].into_iter().collect();
        assert_eq!(collapse_ranges(&set), "10-13");
    }

    #[test]
    fn combine_merges_contexts_for_same_line() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = dir.path().join("worker-0.json");
        let second = dir.path().join("worker-1.json");
        fs::write(
            &first,
            r#"{"files":{"/project/src/app.py":{"executable":[1,2],"executed":[2],"contexts":{"2":["test_a"]}}}}"#,
        )
        .expect("write first worker file");
        fs::write(
            &second,
            r#"{"files":{"/project/src/app.py":{"executable":[1,2],"executed":[2],"contexts":{"2":["test_b"]}}}}"#,
        )
        .expect("write second worker file");

        let first = camino::Utf8PathBuf::from_path_buf(first).expect("utf8 path");
        let second = camino::Utf8PathBuf::from_path_buf(second).expect("utf8 path");
        let combined = combine(&[first, second]).expect("combine coverage files");

        assert_eq!(
            combined
                .get("/project/src/app.py")
                .expect("combined file")
                .contexts
                .get(&2),
            Some(&BTreeSet::from([
                "test_a".to_string(),
                "test_b".to_string()
            ]))
        );
    }

    #[test]
    #[cfg(windows)]
    fn simplify_path_strips_windows_verbatim_prefix() {
        assert_eq!(
            simplify_path(r"\\?\C:\Users\runneradmin\project\test.py"),
            r"C:\Users\runneradmin\project\test.py"
        );
    }

    #[test]
    fn display_path_uses_forward_slashes() {
        assert_eq!(
            display_path(
                "/project/tests\\test_partial.py",
                std::path::Path::new("/project")
            ),
            "tests/test_partial.py"
        );
    }

    #[test]
    fn class_filename_uses_forward_slashes() {
        let row = FileRow {
            name: "tests\\test_partial.py".to_string(),
            absolute_name: "/project/tests\\test_partial.py".to_string(),
            stmts: 0,
            hit: 0,
            miss: 0,
            missing: String::new(),
            executable: Vec::new(),
            excluded: Vec::new(),
            executed: Vec::new(),
            contexts: BTreeMap::new(),
            branches_enabled: false,
            branches: 0,
            branch_hit: 0,
            branch_miss: 0,
            branch_partial: 0,
            branch_possible: Vec::new(),
            branch_executed: Vec::new(),
            branch_missing: Vec::new(),
            arc_contexts: BTreeMap::new(),
        };

        assert_eq!(
            class_filename(&row, std::path::Path::new("/project")),
            "tests/test_partial.py"
        );
    }
}
