//! Combine per-worker JSON files and produce terminal or XML reports.
//!
//! Pure Rust — runs in the main process, never touches Python. Reads each
//! per-worker JSON file written by the [`tracer`](crate::tracer), unions the
//! per-file line sets, and prints a `Name / Stmts / Miss / Cover` table
//! sorted alphabetically with a `TOTAL` row.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use camino::Utf8Path;
use colored::Colorize;

use crate::data::WorkerFile;

/// Combine the per-worker data files in `files` and print a terminal report
/// to stdout. No-ops if there is no data to report.
///
/// `files` is the list of per-worker `coverage.json` paths to merge. The
/// caller (typically [`karva_cache::RunCache::coverage_files`]) is responsible
/// for resolving the paths; this function only reads them.
///
/// When `show_missing` is true, the report includes a final `Missing` column
/// listing the uncovered line numbers per file (consecutive lines collapsed
/// into `a-b` ranges).
///
/// Returns the total coverage percentage (`0.0..=100.0`) shown in the
/// `TOTAL` row, or `None` if there was no data to report. Files with zero
/// executable lines do not contribute to the total.
pub fn combine_and_report(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    show_missing: bool,
) -> Result<Option<f64>> {
    let combined = combine(files)?;
    if combined.is_empty() {
        return Ok(None);
    }
    let total = print_report(cwd, &combined, show_missing, &mut std::io::stdout().lock())?;
    Ok(Some(total))
}

#[derive(Debug, Default)]
struct CombinedFile {
    executable: BTreeSet<u32>,
    executed: BTreeSet<u32>,
}

fn combine(files: &[impl AsRef<Utf8Path>]) -> Result<BTreeMap<String, CombinedFile>> {
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

pub fn write_cobertura_xml(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    output: &Utf8Path,
) -> Result<Option<f64>> {
    let combined = combine(files)?;
    if combined.is_empty() {
        return Ok(None);
    }

    let cwd_real = std::fs::canonicalize(cwd.as_std_path()).unwrap_or_else(|_| cwd.into());
    let rows = build_rows(&cwd_real, &combined, false);
    let total_pct = total_percent(&rows);

    if let Some(parent) = output.parent()
        && !parent.as_str().is_empty()
    {
        std::fs::create_dir_all(parent.as_std_path())
            .with_context(|| format!("failed to create coverage output directory {parent}"))?;
    }

    let xml = build_cobertura_xml(cwd, &cwd_real, &rows);
    std::fs::write(output.as_std_path(), xml)
        .with_context(|| format!("failed to write coverage xml {output}"))?;

    Ok(Some(total_pct))
}

struct Row<'a> {
    name: &'a str,
    stmts: &'a str,
    miss: &'a str,
    cover: &'a str,
    missing: &'a str,
}

struct FileRow {
    name: String,
    absolute_name: String,
    stmts: u32,
    hit: u32,
    miss: u32,
    missing: String,
    executable: Vec<u32>,
    executed: Vec<u32>,
}

fn print_report(
    cwd: &Utf8Path,
    combined: &BTreeMap<String, CombinedFile>,
    show_missing: bool,
    out: &mut dyn Write,
) -> Result<f64> {
    let cwd_real = std::fs::canonicalize(cwd.as_std_path()).unwrap_or_else(|_| cwd.into());

    let rows = build_rows(&cwd_real, combined, show_missing);

    let name_width = rows
        .iter()
        .map(|row| row.name.len())
        .max()
        .unwrap_or(0)
        .max("Name".len())
        .max("TOTAL".len());

    let header = format_row(
        name_width,
        show_missing,
        &Row {
            name: "Name",
            stmts: "Stmts",
            miss: "Miss",
            cover: "Cover",
            missing: "Missing",
        },
    );
    let rule_len = header.chars().count();
    let rule = "-".repeat(rule_len);

    writeln!(out)?;
    writeln!(out, "{}", header.bold())?;
    writeln!(out, "{rule}")?;

    let mut total_stmts: u32 = 0;
    let mut total_miss: u32 = 0;

    for row in &rows {
        let cover = format_percent(row.stmts, row.miss);
        let stmts_str = row.stmts.to_string();
        let miss_str = row.miss.to_string();
        writeln!(
            out,
            "{}",
            format_row(
                name_width,
                show_missing,
                &Row {
                    name: &row.name,
                    stmts: &stmts_str,
                    miss: &miss_str,
                    cover: &cover,
                    missing: &row.missing,
                },
            )
        )?;
        total_stmts = total_stmts.saturating_add(row.stmts);
        total_miss = total_miss.saturating_add(row.miss);
    }

    writeln!(out, "{rule}")?;
    let total_pct = percent(total_stmts, total_miss);
    let total_cover = format_percent(total_stmts, total_miss);
    let total_stmts_str = total_stmts.to_string();
    let total_miss_str = total_miss.to_string();
    writeln!(
        out,
        "{}",
        format_row(
            name_width,
            show_missing,
            &Row {
                name: "TOTAL",
                stmts: &total_stmts_str,
                miss: &total_miss_str,
                cover: &total_cover,
                missing: "",
            },
        )
    )?;

    Ok(total_pct)
}

fn build_rows(
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

fn total_percent(rows: &[FileRow]) -> f64 {
    let total_stmts = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.stmts));
    let total_miss = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.miss));
    percent(total_stmts, total_miss)
}

fn build_cobertura_xml(cwd: &Utf8Path, cwd_real: &std::path::Path, rows: &[FileRow]) -> String {
    let total_stmts = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.stmts));
    let total_hit = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.hit));
    let line_rate = rate(total_hit, total_stmts);
    let timestamp = std::fs::metadata(cwd.as_std_path())
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs());
    let source_root = cwd_real.to_string_lossy().trim_end_matches('/').to_string();

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" ?>\n");
    let _ = writeln!(
        xml,
        "<coverage version=\"1.0\" timestamp=\"{timestamp}\" lines-valid=\"{total_stmts}\" lines-covered=\"{total_hit}\" line-rate=\"{line_rate:.4}\" branches-covered=\"0\" branches-valid=\"0\" branch-rate=\"0.0000\" complexity=\"0.0\">"
    );
    xml.push_str("  <sources>\n");
    let _ = writeln!(xml, "    <source>{}</source>", escape_xml(&source_root));
    xml.push_str("  </sources>\n");
    xml.push_str("  <packages>\n");
    xml.push_str(
        "    <package name=\".\" line-rate=\"0.0000\" branch-rate=\"0.0000\" complexity=\"0.0\">\n",
    );
    xml.push_str("      <classes>\n");

    for row in rows {
        let filename = class_filename(row, cwd_real);
        let _ = writeln!(
            xml,
            "        <class name=\"{}\" filename=\"{}\" line-rate=\"{:.4}\" branch-rate=\"0.0000\" complexity=\"0.0\">",
            escape_xml(&row.name),
            escape_xml(&filename),
            rate(row.hit, row.stmts)
        );
        xml.push_str("          <methods/>\n");
        xml.push_str("          <lines>\n");
        let executed: BTreeSet<u32> = row.executed.iter().copied().collect();
        for line in &row.executable {
            let hits = i32::from(executed.contains(line));
            let _ = writeln!(
                xml,
                "            <line number=\"{line}\" hits=\"{hits}\" branch=\"false\"/>"
            );
        }
        xml.push_str("          </lines>\n");
        xml.push_str("        </class>\n");
    }

    xml.push_str("      </classes>\n");
    xml.push_str("    </package>\n");
    xml.push_str("  </packages>\n");
    xml.push_str("</coverage>\n");
    xml
}

fn class_filename(row: &FileRow, cwd_real: &std::path::Path) -> String {
    if let Ok(relative) = std::path::Path::new(&row.absolute_name).strip_prefix(cwd_real) {
        relative.to_string_lossy().replace('\\', "/")
    } else {
        row.absolute_name.replace('\\', "/")
    }
}

fn rate(hit: u32, total: u32) -> f64 {
    if total == 0 {
        1.0
    } else {
        f64::from(hit) / f64::from(total)
    }
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn format_row(name_width: usize, show_missing: bool, row: &Row<'_>) -> String {
    let base = format!(
        "{name:<name_width$}   {stmts:>stmts_w$}   {miss:>miss_w$}   {cover:>cover_w$}",
        name = row.name,
        stmts = row.stmts,
        miss = row.miss,
        cover = row.cover,
        stmts_w = "Stmts".len(),
        miss_w = "Miss".len(),
        cover_w = "Cover".len(),
    );
    if show_missing && !row.missing.is_empty() {
        format!("{base}   {missing}", missing = row.missing)
    } else {
        base
    }
}

fn collapse_ranges(lines: &BTreeSet<u32>) -> String {
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

fn percent(total: u32, miss: u32) -> f64 {
    if total == 0 {
        return 100.0;
    }
    let hit = total - miss.min(total);
    f64::from(hit) / f64::from(total) * 100.0
}

fn format_percent(total: u32, miss: u32) -> String {
    let pct = percent(total, miss);
    format!("{pct:.0}%")
}

fn display_path(absolute: &str, cwd: &std::path::Path) -> String {
    if let Ok(rel) = std::path::Path::new(absolute).strip_prefix(cwd) {
        rel.to_string_lossy().into_owned()
    } else {
        absolute.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cf(executable: &[u32], executed: &[u32]) -> CombinedFile {
        CombinedFile {
            executable: executable.iter().copied().collect(),
            executed: executed.iter().copied().collect(),
        }
    }

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
    fn report_contains_total_row() {
        let mut data = BTreeMap::new();
        data.insert("/proj/a.py".to_string(), cf(&[1, 2, 3, 4], &[1, 2]));
        data.insert("/proj/b.py".to_string(), cf(&[1, 2], &[1, 2]));

        let mut buf: Vec<u8> = Vec::new();
        let total = print_report(Utf8Path::new("/proj"), &data, false, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();

        assert!(out.contains("a.py"));
        assert!(out.contains("b.py"));
        assert!(out.contains("TOTAL"));
        assert!(out.contains("67%"));
        assert!(!out.contains("Missing"));
        // 4/6 hit lines ≈ 66.67%; displayed as a rounded `67%` but the
        // returned float is preserved for threshold checks.
        assert!(total > 66.0 && total < 67.0);
    }

    #[test]
    fn report_with_missing_shows_uncovered_lines() {
        let mut data = BTreeMap::new();
        data.insert(
            "/proj/a.py".to_string(),
            cf(&[1, 2, 3, 4, 5, 6, 7, 8, 9], &[1, 5, 9]),
        );

        let mut buf: Vec<u8> = Vec::new();
        print_report(Utf8Path::new("/proj"), &data, true, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();

        assert!(out.contains("Missing"));
        assert!(out.contains("2-4, 6-8"));
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
}
