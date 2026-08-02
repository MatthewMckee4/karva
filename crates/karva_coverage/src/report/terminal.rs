use std::io::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use colored::Colorize;
use fs_err as fs;

use super::combined_rows;
use super::html::build_html_report;
use super::json::build_json_report;
use super::shared::{FileRow, row_percent, total_percent, totals_row};
use super::xml::build_cobertura_xml;
use super::{CoverageAnalysis, CoverageFilters};

/// Prints the terminal coverage table and returns its total percentage.
///
/// Returns `None` when no worker coverage artifacts contain source data.
pub fn combine_and_report(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    show_missing: bool,
    filters: &CoverageFilters,
) -> Result<Option<f64>> {
    let Some(analysis) = combined_rows(cwd, files, filters)? else {
        return Ok(None);
    };
    analysis.report(show_missing).map(Some)
}

/// Writes a Cobertura-compatible XML report and returns its total percentage.
pub fn write_cobertura_xml(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    output: &Utf8Path,
    filters: &CoverageFilters,
) -> Result<Option<f64>> {
    let Some(analysis) = combined_rows(cwd, files, filters)? else {
        return Ok(None);
    };
    analysis.write_cobertura_xml(output).map(Some)
}

/// Writes a coverage.py-compatible JSON report and returns its total percentage.
pub fn write_json_report(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    output: &Utf8Path,
    filters: &CoverageFilters,
) -> Result<Option<f64>> {
    let Some(analysis) = combined_rows(cwd, files, filters)? else {
        return Ok(None);
    };
    analysis.write_json(output).map(Some)
}

/// Writes a standalone HTML coverage report and returns its total percentage.
pub fn write_html_report(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    output_dir: &Utf8Path,
    filters: &CoverageFilters,
) -> Result<Option<f64>> {
    let Some(analysis) = combined_rows(cwd, files, filters)? else {
        return Ok(None);
    };
    analysis.write_html(output_dir).map(Some)
}

impl CoverageAnalysis {
    /// Prints the compact terminal report and returns total coverage.
    pub fn report(&self, show_missing: bool) -> Result<f64> {
        print_report(&self.rows, show_missing, &mut std::io::stdout().lock())
    }

    /// Writes a Cobertura-compatible XML report and returns total coverage.
    pub fn write_cobertura_xml(&self, output: &Utf8Path) -> Result<f64> {
        create_output_parent(output)?;
        let xml = build_cobertura_xml(&self.coverage_root, &self.cwd_real, &self.rows)?;
        fs::write(output.as_std_path(), xml)
            .with_context(|| format!("failed to write coverage xml {output}"))?;
        Ok(self.total_percent())
    }

    /// Writes a coverage.py-compatible JSON report and returns total coverage.
    pub fn write_json(&self, output: &Utf8Path) -> Result<f64> {
        create_output_parent(output)?;
        let json = build_json_report(&self.rows)?;
        fs::write(output.as_std_path(), json)
            .with_context(|| format!("failed to write coverage json {output}"))?;
        Ok(self.total_percent())
    }

    /// Writes a standalone HTML report and returns total coverage.
    pub fn write_html(&self, output_dir: &Utf8Path) -> Result<f64> {
        fs::create_dir_all(output_dir.as_std_path())
            .with_context(|| format!("failed to create coverage html directory {output_dir}"))?;
        let html = build_html_report(&self.rows);
        let output_file = output_dir.join("index.html");
        fs::write(output_file.as_std_path(), html)
            .with_context(|| format!("failed to write coverage html {output_file}"))?;
        Ok(self.total_percent())
    }
}

fn create_output_parent(output: &Utf8Path) -> Result<()> {
    if let Some(parent) = output.parent()
        && !parent.as_str().is_empty()
    {
        fs::create_dir_all(parent.as_std_path())
            .with_context(|| format!("failed to create coverage output directory {parent}"))?;
    }
    Ok(())
}

struct Row<'a> {
    name: &'a str,
    stmts: &'a str,
    miss: &'a str,
    branches: &'a str,
    branch_partial: &'a str,
    cover: &'a str,
    missing: &'a str,
}

fn print_report(rows: &[FileRow], show_missing: bool, out: &mut dyn Write) -> Result<f64> {
    let show_branches = rows.iter().any(|row| row.branches_enabled);
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
        show_branches,
        &Row {
            name: "Name",
            stmts: "Stmts",
            miss: "Miss",
            branches: "Branch",
            branch_partial: "BrPart",
            cover: "Cover",
            missing: "Missing",
        },
    );
    let rule_len = header.chars().count();
    let rule = "-".repeat(rule_len);

    writeln!(out)?;
    writeln!(out, "{}", header.bold())?;
    writeln!(out, "{rule}")?;

    for row in rows {
        let cover = format!("{:.0}%", row_percent(row));
        let stmts_str = row.stmts.to_string();
        let miss_str = row.miss.to_string();
        let branches_str = row.branches.to_string();
        let branch_partial_str = row.branch_partial.to_string();
        writeln!(
            out,
            "{}",
            format_row(
                name_width,
                show_missing,
                show_branches,
                &Row {
                    name: &row.name,
                    stmts: &stmts_str,
                    miss: &miss_str,
                    branches: &branches_str,
                    branch_partial: &branch_partial_str,
                    cover: &cover,
                    missing: &row.missing,
                },
            )
        )?;
    }

    writeln!(out, "{rule}")?;
    let total_pct = total_percent(rows);
    let total = totals_row(rows);
    let total_cover = format!("{:.0}%", row_percent(&total));
    let total_stmts_str = total.stmts.to_string();
    let total_miss_str = total.miss.to_string();
    let total_branches_str = total.branches.to_string();
    let total_branch_partial_str = total.branch_partial.to_string();
    writeln!(
        out,
        "{}",
        format_row(
            name_width,
            show_missing,
            show_branches,
            &Row {
                name: "TOTAL",
                stmts: &total_stmts_str,
                miss: &total_miss_str,
                branches: &total_branches_str,
                branch_partial: &total_branch_partial_str,
                cover: &total_cover,
                missing: "",
            },
        )
    )?;

    Ok(total_pct)
}

fn format_row(name_width: usize, show_missing: bool, show_branches: bool, row: &Row<'_>) -> String {
    let base = if show_branches {
        format!(
            "{name:<name_width$}   {stmts:>stmts_w$}   {miss:>miss_w$}   {branches:>branches_w$}   {branch_partial:>branch_partial_w$}   {cover:>cover_w$}",
            name = row.name,
            stmts = row.stmts,
            miss = row.miss,
            branches = row.branches,
            branch_partial = row.branch_partial,
            cover = row.cover,
            stmts_w = "Stmts".len(),
            miss_w = "Miss".len(),
            branches_w = "Branch".len(),
            branch_partial_w = "BrPart".len(),
            cover_w = "Cover".len(),
        )
    } else {
        format!(
            "{name:<name_width$}   {stmts:>stmts_w$}   {miss:>miss_w$}   {cover:>cover_w$}",
            name = row.name,
            stmts = row.stmts,
            miss = row.miss,
            cover = row.cover,
            stmts_w = "Stmts".len(),
            miss_w = "Miss".len(),
            cover_w = "Cover".len(),
        )
    };
    if show_missing && !row.missing.is_empty() {
        format!("{base}   {missing}", missing = row.missing)
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn row(name: &str, stmts: u32, hit: u32, miss: u32, missing: &str) -> FileRow {
        FileRow {
            name: name.to_string(),
            absolute_name: format!("/proj/{name}"),
            stmts,
            hit,
            miss,
            missing: missing.to_string(),
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
        }
    }

    #[test]
    fn report_contains_total_row() {
        let rows = [row("a.py", 4, 2, 2, ""), row("b.py", 2, 2, 0, "")];

        let mut buf: Vec<u8> = Vec::new();
        let total = print_report(&rows, false, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();

        assert!(out.contains("a.py"));
        assert!(out.contains("b.py"));
        assert!(out.contains("TOTAL"));
        assert!(out.contains("67%"));
        assert!(!out.contains("Missing"));
        assert!(total > 66.0 && total < 67.0);
    }

    #[test]
    fn report_with_missing_shows_uncovered_lines() {
        let rows = [row("a.py", 9, 3, 6, "2-4, 6-8")];

        let mut buf: Vec<u8> = Vec::new();
        print_report(&rows, true, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();

        assert!(out.contains("Missing"));
        assert!(out.contains("2-4, 6-8"));
    }
}
