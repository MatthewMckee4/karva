use std::io::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use colored::Colorize;
use fs_err as fs;

use super::combined_rows;
use super::html::{HtmlReportOptions, build_html_report};
use super::json::{JsonReportOptions, build_json_report};
use super::shared::{FileRow, row_percent, total_percent, totals_row};
use super::xml::build_cobertura_xml;
use super::{CoverageAnalysis, CoverageFilters};

/// Terminal coverage report representation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoverageReportFormat {
    /// Human-readable aligned text table.
    #[default]
    Text,
    /// GitHub-flavored Markdown table.
    Markdown,
    /// Numeric total percentage only.
    Total,
}

/// Column used to order displayed coverage rows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoverageReportSort {
    #[default]
    Name,
    Statements,
    Misses,
    Branches,
    PartialBranches,
    Coverage,
}

/// Selection and presentation settings for a terminal coverage report.
#[derive(Debug, Default)]
pub struct CoverageReportOptions {
    /// File paths, directories, or dotted module names to include.
    pub selectors: Vec<String>,
    /// Whether to show missing lines and branch arcs.
    pub show_missing: bool,
    /// Whether to hide fully covered rows without changing totals.
    pub skip_covered: bool,
    /// Whether to hide rows with no statements or branches without changing totals.
    pub skip_empty: bool,
    /// Column used to order displayed rows.
    pub sort: CoverageReportSort,
    /// Decimal places shown for percentages.
    pub precision: usize,
    /// Output representation.
    pub format: CoverageReportFormat,
}

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
        self.report_with_precision(show_missing, 0)
    }

    /// Prints the compact terminal report using `precision` decimal places.
    pub fn report_with_precision(&self, show_missing: bool, precision: usize) -> Result<f64> {
        self.write_report(
            &CoverageReportOptions {
                show_missing,
                precision,
                ..CoverageReportOptions::default()
            },
            &mut std::io::stdout().lock(),
        )
    }

    /// Writes a selected and formatted terminal report and returns total coverage.
    pub fn write_report(
        &self,
        options: &CoverageReportOptions,
        out: &mut dyn Write,
    ) -> Result<f64> {
        let selected = select_rows(&self.rows, &options.selectors)?;
        let mut displayed = selected.clone();
        if options.skip_covered {
            displayed.retain(|row| row_percent(row) < 100.0);
        }
        if options.skip_empty {
            displayed.retain(|row| row.stmts > 0 || row.branches > 0);
        }
        sort_rows(&mut displayed, options.sort);
        match options.format {
            CoverageReportFormat::Text => print_report(
                &displayed,
                &selected,
                options.show_missing,
                options.precision,
                out,
            ),
            CoverageReportFormat::Markdown => print_markdown_report(
                &displayed,
                &selected,
                options.show_missing,
                options.precision,
                out,
            ),
            CoverageReportFormat::Total => {
                let total = total_percent(&selected);
                writeln!(out, "{:.*}", options.precision, total)?;
                Ok(total)
            }
        }
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
        self.write_json_with_options(
            output,
            &JsonReportOptions {
                pretty_print: true,
                show_contexts: self.rows.iter().any(|row| !row.contexts.is_empty()),
            },
        )
    }

    /// Writes configured exported JSON coverage data and returns total coverage.
    pub fn write_json_with_options(
        &self,
        output: &Utf8Path,
        options: &JsonReportOptions,
    ) -> Result<f64> {
        create_output_parent(output)?;
        let json = build_json_report(&self.rows, options)?;
        fs::write(output.as_std_path(), json)
            .with_context(|| format!("failed to write coverage json {output}"))?;
        Ok(self.total_percent())
    }

    /// Writes a standalone HTML report and returns total coverage.
    pub fn write_html(&self, output_dir: &Utf8Path) -> Result<f64> {
        self.write_html_with_options(output_dir, &HtmlReportOptions::default())
    }

    /// Writes an annotated standalone HTML report and returns total coverage.
    pub fn write_html_with_options(
        &self,
        output_dir: &Utf8Path,
        options: &HtmlReportOptions,
    ) -> Result<f64> {
        fs::create_dir_all(output_dir.as_std_path())
            .with_context(|| format!("failed to create coverage html directory {output_dir}"))?;
        let sources = self
            .rows
            .iter()
            .map(|row| {
                let path = Utf8Path::new(&row.absolute_name);
                let path = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    self.coverage_root.join(path)
                };
                fs::read_to_string(path.as_std_path())
                    .with_context(|| format!("failed to read coverage source {path}"))
            })
            .collect::<Result<Vec<_>>>()?;
        let report = build_html_report(&self.rows, &sources, options)?;
        let output_file = output_dir.join("index.html");
        fs::write(output_file.as_std_path(), report.index)
            .with_context(|| format!("failed to write coverage html {output_file}"))?;
        for (filename, html) in report.sources {
            let output_file = output_dir.join(filename);
            fs::write(output_file.as_std_path(), html)
                .with_context(|| format!("failed to write coverage html {output_file}"))?;
        }
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

fn print_report(
    rows: &[FileRow],
    total_rows: &[FileRow],
    show_missing: bool,
    precision: usize,
    out: &mut dyn Write,
) -> Result<f64> {
    let show_branches = total_rows.iter().any(|row| row.branches_enabled);
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
        let cover = format!("{:.*}%", precision, row_percent(row));
        let stmts_str = row.stmts.to_string();
        let miss_str = row.miss.to_string();
        let branches_str = row.branches.to_string();
        let branch_partial_str = row.branch_partial.to_string();
        let missing = missing_display(row);
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
                    missing: &missing,
                },
            )
        )?;
    }

    writeln!(out, "{rule}")?;
    let total_pct = total_percent(total_rows);
    let total = totals_row(total_rows);
    let total_cover = format!("{:.*}%", precision, row_percent(&total));
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

fn print_markdown_report(
    rows: &[FileRow],
    total_rows: &[FileRow],
    show_missing: bool,
    precision: usize,
    out: &mut dyn Write,
) -> Result<f64> {
    let show_branches = total_rows.iter().any(|row| row.branches_enabled);
    write!(out, "| Name | Stmts | Miss")?;
    if show_branches {
        write!(out, " | Branch | BrPart")?;
    }
    write!(out, " | Cover")?;
    if show_missing {
        write!(out, " | Missing")?;
    }
    writeln!(out, " |")?;
    write!(out, "| --- | ---: | ---:")?;
    if show_branches {
        write!(out, " | ---: | ---:")?;
    }
    write!(out, " | ---:")?;
    if show_missing {
        write!(out, " | ---")?;
    }
    writeln!(out, " |")?;
    for row in rows {
        write!(out, "| {} | {} | {}", row.name, row.stmts, row.miss)?;
        if show_branches {
            write!(out, " | {} | {}", row.branches, row.branch_partial)?;
        }
        write!(out, " | {:.*}%", precision, row_percent(row))?;
        if show_missing {
            write!(out, " | {}", missing_display(row))?;
        }
        writeln!(out, " |")?;
    }
    let total = totals_row(total_rows);
    write!(out, "| **TOTAL** | {} | {}", total.stmts, total.miss)?;
    if show_branches {
        write!(out, " | {} | {}", total.branches, total.branch_partial)?;
    }
    let total_percent = total_percent(total_rows);
    write!(out, " | **{total_percent:.precision$}%**")?;
    if show_missing {
        write!(out, " | ")?;
    }
    writeln!(out, " |")?;
    Ok(total_percent)
}

fn select_rows(rows: &[FileRow], selectors: &[String]) -> Result<Vec<FileRow>> {
    if selectors.is_empty() {
        return Ok(rows.to_vec());
    }
    let mut matched = vec![false; selectors.len()];
    let selected = rows
        .iter()
        .filter(|row| {
            let mut include = false;
            for (index, selector) in selectors.iter().enumerate() {
                if selector_matches(&row.name, selector) {
                    matched[index] = true;
                    include = true;
                }
            }
            include
        })
        .cloned()
        .collect();
    if let Some((index, _)) = matched.iter().enumerate().find(|(_, matched)| !**matched) {
        anyhow::bail!(
            "coverage selector `{}` matched no source files",
            selectors[index]
        );
    }
    Ok(selected)
}

fn selector_matches(name: &str, selector: &str) -> bool {
    let selector_path = selector.replace('.', "/");
    name == selector
        || name == format!("{selector_path}.py")
        || name.starts_with(&format!("{}/", selector.trim_end_matches('/')))
        || name.starts_with(&format!("{selector_path}/"))
}

fn sort_rows(rows: &mut [FileRow], sort: CoverageReportSort) {
    rows.sort_by(|left, right| match sort {
        CoverageReportSort::Name => left.name.cmp(&right.name),
        CoverageReportSort::Statements => left
            .stmts
            .cmp(&right.stmts)
            .then(left.name.cmp(&right.name)),
        CoverageReportSort::Misses => left.miss.cmp(&right.miss).then(left.name.cmp(&right.name)),
        CoverageReportSort::Branches => left
            .branches
            .cmp(&right.branches)
            .then(left.name.cmp(&right.name)),
        CoverageReportSort::PartialBranches => left
            .branch_partial
            .cmp(&right.branch_partial)
            .then(left.name.cmp(&right.name)),
        CoverageReportSort::Coverage => row_percent(left)
            .total_cmp(&row_percent(right))
            .then(left.name.cmp(&right.name)),
    });
}

fn missing_display(row: &FileRow) -> String {
    let mut missing = row.missing.clone();
    for arc in &row.branch_missing {
        if !missing.is_empty() {
            missing.push_str(", ");
        }
        missing.push_str(&arc.from.to_string());
        missing.push_str("->");
        missing.push_str(&arc.to.to_string());
    }
    missing
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
        let total = print_report(&rows, &rows, false, 0, &mut buf).unwrap();
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
        print_report(&rows, &rows, true, 0, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();

        assert!(out.contains("Missing"));
        assert!(out.contains("2-4, 6-8"));
    }

    #[test]
    fn skip_covered_hides_row_without_changing_total() {
        let analysis = CoverageAnalysis {
            coverage_root: camino::Utf8PathBuf::from("/proj"),
            cwd_real: std::path::PathBuf::from("/proj"),
            rows: vec![
                row("empty.py", 0, 0, 0, ""),
                row("covered.py", 2, 2, 0, ""),
                row("partial.py", 2, 1, 1, "2"),
            ],
        };
        let mut output = Vec::new();

        let total = analysis
            .write_report(
                &CoverageReportOptions {
                    skip_covered: true,
                    skip_empty: true,
                    format: CoverageReportFormat::Markdown,
                    ..CoverageReportOptions::default()
                },
                &mut output,
            )
            .expect("write report");

        insta::assert_snapshot!(String::from_utf8(output).expect("UTF-8 report"), @r"
        | Name | Stmts | Miss | Cover |
        | --- | ---: | ---: | ---: |
        | partial.py | 2 | 1 | 50% |
        | **TOTAL** | 4 | 1 | **75%** |
        ");
        assert!((total - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn missing_display_includes_branch_arcs() {
        let mut branch_row = row("branch.py", 2, 1, 1, "2");
        branch_row.branch_missing = vec![
            crate::data::BranchArc { from: 1, to: 2 },
            crate::data::BranchArc { from: 1, to: 3 },
        ];

        assert_eq!(missing_display(&branch_row), "2, 1->2, 1->3");
    }

    #[test]
    fn selectors_accept_dotted_modules_and_name_failures() {
        let rows = vec![row("src/package/module.py", 1, 1, 0, "")];

        assert_eq!(
            select_rows(&rows, &["src.package.module".to_owned()])
                .expect("select module")
                .len(),
            1
        );
        let error = select_rows(&rows, &["missing.module".to_owned()])
            .expect_err("reject unmatched selector");
        assert!(error.to_string().contains("missing.module"));
    }

    #[test]
    fn rows_sort_by_requested_metric_then_name() {
        let mut rows = vec![
            row("b.py", 4, 3, 1, "4"),
            row("a.py", 2, 1, 1, "2"),
            row("c.py", 2, 2, 0, ""),
        ];

        sort_rows(&mut rows, CoverageReportSort::Coverage);

        assert_eq!(
            rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
            ["a.py", "b.py", "c.py"]
        );
    }
}
