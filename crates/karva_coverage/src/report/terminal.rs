use std::io::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use colored::Colorize;
use fs_err as fs;

use super::CoverageFilters;
use super::combined_rows;
use super::html::build_html_report;
use super::json::build_json_report;
use super::shared::{FileRow, row_percent, total_percent};
use super::xml::build_cobertura_xml;

pub fn combine_and_report(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    show_missing: bool,
    filters: &CoverageFilters,
) -> Result<Option<f64>> {
    let Some((_, rows)) = combined_rows(cwd, files, show_missing, filters)? else {
        return Ok(None);
    };
    let total = print_report(&rows, show_missing, &mut std::io::stdout().lock())?;
    Ok(Some(total))
}

pub fn write_cobertura_xml(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    output: &Utf8Path,
    filters: &CoverageFilters,
) -> Result<Option<f64>> {
    let Some((cwd_real, rows)) = combined_rows(cwd, files, false, filters)? else {
        return Ok(None);
    };
    let total_pct = total_percent(&rows);

    if let Some(parent) = output.parent()
        && !parent.as_str().is_empty()
    {
        fs::create_dir_all(parent.as_std_path())
            .with_context(|| format!("failed to create coverage output directory {parent}"))?;
    }

    let xml = build_cobertura_xml(cwd, &cwd_real, &rows)?;
    fs::write(output.as_std_path(), xml)
        .with_context(|| format!("failed to write coverage xml {output}"))?;

    Ok(Some(total_pct))
}

pub fn write_json_report(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    output: &Utf8Path,
    filters: &CoverageFilters,
) -> Result<Option<f64>> {
    let Some((_, rows)) = combined_rows(cwd, files, false, filters)? else {
        return Ok(None);
    };
    let total_pct = total_percent(&rows);

    if let Some(parent) = output.parent()
        && !parent.as_str().is_empty()
    {
        fs::create_dir_all(parent.as_std_path())
            .with_context(|| format!("failed to create coverage output directory {parent}"))?;
    }

    let json = build_json_report(&rows)?;
    fs::write(output.as_std_path(), json)
        .with_context(|| format!("failed to write coverage json {output}"))?;

    Ok(Some(total_pct))
}

pub fn write_html_report(
    cwd: &Utf8Path,
    files: &[impl AsRef<Utf8Path>],
    output_dir: &Utf8Path,
    filters: &CoverageFilters,
) -> Result<Option<f64>> {
    let Some((_, rows)) = combined_rows(cwd, files, true, filters)? else {
        return Ok(None);
    };
    let total_pct = total_percent(&rows);

    fs::create_dir_all(output_dir.as_std_path())
        .with_context(|| format!("failed to create coverage html directory {output_dir}"))?;

    let html = build_html_report(&rows);
    let output_file = output_dir.join("index.html");
    fs::write(output_file.as_std_path(), html)
        .with_context(|| format!("failed to write coverage html {output_file}"))?;

    Ok(Some(total_pct))
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

    let mut total_stmts: u32 = 0;
    let mut total_miss: u32 = 0;
    let mut total_branches: u32 = 0;
    let mut total_branch_miss: u32 = 0;
    let mut total_branch_partial: u32 = 0;

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
        total_stmts = total_stmts.saturating_add(row.stmts);
        total_miss = total_miss.saturating_add(row.miss);
        total_branches = total_branches.saturating_add(row.branches);
        total_branch_miss = total_branch_miss.saturating_add(row.branch_miss);
        total_branch_partial = total_branch_partial.saturating_add(row.branch_partial);
    }

    writeln!(out, "{rule}")?;
    let total_pct = total_percent(rows);
    let total_cover = format!(
        "{:.0}%",
        super::shared::percent(
            total_stmts.saturating_add(total_branches),
            total_miss.saturating_add(total_branch_miss),
        )
    );
    let total_stmts_str = total_stmts.to_string();
    let total_miss_str = total_miss.to_string();
    let total_branches_str = total_branches.to_string();
    let total_branch_partial_str = total_branch_partial.to_string();
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
            arcs: Vec::new(),
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
