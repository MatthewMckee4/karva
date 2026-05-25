use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use camino::Utf8Path;

use crate::data::WorkerFile;

#[derive(Debug, Default)]
pub struct CombinedFile {
    pub executable: BTreeSet<u32>,
    pub executed: BTreeSet<u32>,
}

pub struct FileRow {
    pub name: String,
    pub absolute_name: String,
    pub stmts: u32,
    pub hit: u32,
    pub miss: u32,
    pub missing: String,
    pub executable: Vec<u32>,
    pub executed: Vec<u32>,
}

pub fn combine(files: &[impl AsRef<Utf8Path>]) -> Result<BTreeMap<String, CombinedFile>> {
    let mut combined: BTreeMap<String, CombinedFile> = BTreeMap::new();

    for path in files {
        let path = path.as_ref();
        let bytes = std::fs::read(path.as_std_path())
            .with_context(|| format!("failed to read coverage file {path}"))?;
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

pub fn build_rows(
    cwd_real: &std::path::Path,
    combined: &BTreeMap<String, CombinedFile>,
    show_missing: bool,
) -> Vec<FileRow> {
    combined
        .iter()
        .map(|(filename, data)| {
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
                name: display_path(filename, cwd_real),
                absolute_name: filename.clone(),
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

pub fn total_percent(rows: &[FileRow]) -> f64 {
    let total_stmts = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.stmts));
    let total_miss = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.miss));
    percent(total_stmts, total_miss)
}

pub fn totals_row(rows: &[FileRow]) -> FileRow {
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

pub fn missing_lines(row: &FileRow) -> Vec<u32> {
    let executed: BTreeSet<u32> = row.executed.iter().copied().collect();
    row.executable
        .iter()
        .copied()
        .filter(|line| !executed.contains(line))
        .collect()
}

pub fn class_filename(row: &FileRow, cwd_real: &std::path::Path) -> String {
    if let Ok(relative) = std::path::Path::new(&row.absolute_name).strip_prefix(cwd_real) {
        relative.to_string_lossy().replace('\\', "/")
    } else {
        row.absolute_name.replace('\\', "/")
    }
}

pub fn percent(total: u32, miss: u32) -> f64 {
    if total == 0 {
        return 100.0;
    }
    let hit = total - miss.min(total);
    f64::from(hit) / f64::from(total) * 100.0
}

pub fn rate(hit: u32, total: u32) -> f64 {
    if total == 0 {
        1.0
    } else {
        f64::from(hit) / f64::from(total)
    }
}

pub fn format_percent(total: u32, miss: u32) -> String {
    let pct = percent(total, miss);
    format!("{pct:.0}%")
}

pub fn collapse_ranges(lines: &BTreeSet<u32>) -> String {
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

fn display_path(absolute: &str, cwd: &std::path::Path) -> String {
    if let Ok(rel) = std::path::Path::new(absolute).strip_prefix(cwd) {
        rel.to_string_lossy().into_owned()
    } else {
        absolute.to_string()
    }
}

pub fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
