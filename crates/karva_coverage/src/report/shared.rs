use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

use crate::data::WorkerFile;

#[derive(Debug, Default)]
pub(super) struct CombinedFile {
    executable: BTreeSet<u32>,
    executed: BTreeSet<u32>,
}

pub(super) struct FileRow {
    pub name: String,
    pub absolute_name: String,
    pub stmts: u32,
    pub hit: u32,
    pub miss: u32,
    pub missing: String,
    pub executable: Vec<u32>,
    pub executed: Vec<u32>,
}

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
        }
    }

    Ok(combined)
}

pub(super) fn build_rows(
    cwd_real: &std::path::Path,
    combined: &BTreeMap<String, CombinedFile>,
    show_missing: bool,
) -> Vec<FileRow> {
    combined
        .iter()
        .map(|(filename, data)| {
            let absolute_name = simplify_path(filename);
            let executable: Vec<u32> = data.executable.iter().copied().collect();
            let executed: Vec<u32> = data.executed.iter().copied().collect();
            let stmts = u32::try_from(executable.len()).unwrap_or(u32::MAX);
            let hit = u32::try_from(executed.len()).unwrap_or(u32::MAX);
            let miss = stmts.saturating_sub(hit);
            let missing = if show_missing {
                let uncovered: BTreeSet<u32> = data
                    .executable
                    .difference(&data.executed)
                    .copied()
                    .collect();
                collapse_ranges(&uncovered)
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
                executed,
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
    percent(total_stmts, total_miss)
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
    FileRow {
        name: "TOTAL".to_string(),
        absolute_name: String::new(),
        stmts,
        hit,
        miss,
        missing: collapse_ranges(&missing.iter().copied().collect()),
        executable: Vec::new(),
        executed: Vec::new(),
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

pub(super) fn format_percent(total: u32, miss: u32) -> String {
    let pct = percent(total, miss);
    format!("{pct:.0}%")
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
        assert_eq!(format_percent(10, 0), "100%");
    }

    #[test]
    fn percent_partial() {
        assert_eq!(format_percent(10, 3), "70%");
    }

    #[test]
    fn percent_zero_stmts() {
        assert_eq!(format_percent(0, 0), "100%");
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
            executed: Vec::new(),
        };

        assert_eq!(
            class_filename(&row, std::path::Path::new("/project")),
            "tests/test_partial.py"
        );
    }
}
