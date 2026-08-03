use std::collections::BTreeSet;
use std::fmt::{self, Write};

use super::shared::{FileRow, escape_html, row_percent, total_percent, totals_row};

/// Presentation settings for an annotated HTML coverage report.
#[derive(Debug)]
pub struct HtmlReportOptions {
    /// Browser title and report heading.
    pub title: String,
    /// Whether line and branch contexts appear on source pages.
    pub show_contexts: bool,
    /// Whether fully covered files are omitted from the index and source pages.
    pub skip_covered: bool,
    /// Whether files without statements or branches are omitted.
    pub skip_empty: bool,
    /// Decimal places shown for percentages.
    pub precision: usize,
}

impl Default for HtmlReportOptions {
    fn default() -> Self {
        Self {
            title: "Coverage report".to_owned(),
            show_contexts: false,
            skip_covered: false,
            skip_empty: false,
            precision: 0,
        }
    }
}

/// Fully rendered report files, relative to the requested output directory.
pub(super) struct HtmlReport {
    pub(super) index: String,
    pub(super) sources: Vec<(String, String)>,
}

pub(super) fn build_html_report(
    rows: &[FileRow],
    sources: &[String],
    options: &HtmlReportOptions,
) -> Result<HtmlReport, fmt::Error> {
    let visible: Vec<(&FileRow, &String)> = rows
        .iter()
        .zip(sources)
        .filter(|(row, _)| !options.skip_covered || row_percent(row) < 100.0)
        .filter(|(row, _)| !options.skip_empty || row.stmts > 0 || row.branches > 0)
        .collect();
    let index = render_index(rows, &visible, options)?;
    let mut pages = Vec::with_capacity(visible.len());
    for (row, source) in visible {
        pages.push((
            source_filename(&row.name),
            render_source(row, source, options)?,
        ));
    }
    Ok(HtmlReport {
        index,
        sources: pages,
    })
}

fn render_index(
    all_rows: &[FileRow],
    visible: &[(&FileRow, &String)],
    options: &HtmlReportOptions,
) -> Result<String, fmt::Error> {
    let mut html = String::new();
    document_start(&mut html, &options.title)?;
    writeln!(html, "  <h1>{}</h1>", escape_html(&options.title))?;
    let total = totals_row(all_rows);
    let total_hit = total.hit.saturating_add(total.branch_hit);
    let total_valid = total.stmts.saturating_add(total.branches);
    writeln!(
        html,
        "  <p>Total coverage: <strong>{:.precision$}%</strong> ({total_hit}/{total_valid})</p>",
        row_percent(&total),
        precision = options.precision
    )?;
    let show_branches = all_rows.iter().any(|row| row.branches_enabled);
    writeln!(html, "  <table>")?;
    write!(
        html,
        "    <thead><tr><th>Name</th><th>Stmts</th><th>Miss</th>"
    )?;
    if show_branches {
        write!(html, "<th>Branch</th><th>BrPart</th>")?;
    }
    writeln!(html, "<th>Cover</th><th>Missing</th></tr></thead>")?;
    writeln!(html, "    <tbody>")?;
    for (row, _) in visible {
        write!(
            html,
            "      <tr><td><a href=\"{}\"><code>{}</code></a></td><td class=\"num\">{}</td><td class=\"num\">{}</td>",
            source_filename(&row.name),
            escape_html(&row.name),
            row.stmts,
            row.miss
        )?;
        if show_branches {
            write!(
                html,
                "<td class=\"num\">{}</td><td class=\"num\">{}</td>",
                row.branches, row.branch_partial
            )?;
        }
        writeln!(
            html,
            "<td class=\"num\">{:.precision$}%</td><td><code>{}</code></td></tr>",
            row_percent(row),
            escape_html(&row.missing),
            precision = options.precision
        )?;
    }
    write!(
        html,
        "      <tr><td><strong>TOTAL</strong></td><td class=\"num\"><strong>{}</strong></td><td class=\"num\"><strong>{}</strong></td>",
        total.stmts, total.miss
    )?;
    if show_branches {
        write!(
            html,
            "<td class=\"num\"><strong>{}</strong></td><td class=\"num\"><strong>{}</strong></td>",
            total.branches, total.branch_partial
        )?;
    }
    writeln!(
        html,
        "<td class=\"num\"><strong>{:.precision$}%</strong></td><td></td></tr>",
        total_percent(all_rows),
        precision = options.precision
    )?;
    writeln!(html, "    </tbody>")?;
    writeln!(html, "  </table>")?;
    document_end(&mut html)?;
    Ok(html)
}

fn render_source(
    row: &FileRow,
    source: &str,
    options: &HtmlReportOptions,
) -> Result<String, fmt::Error> {
    let mut html = String::new();
    let page_title = format!("{} — {}", row.name, options.title);
    document_start(&mut html, &page_title)?;
    writeln!(html, "  <p><a href=\"index.html\">← Index</a></p>")?;
    writeln!(html, "  <h1><code>{}</code></h1>", escape_html(&row.name))?;
    writeln!(
        html,
        "  <p>Coverage: <strong>{:.precision$}%</strong></p>",
        row_percent(row),
        precision = options.precision
    )?;
    writeln!(html, "  <pre class=\"source\">")?;
    for (index, text) in source.lines().enumerate() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let state = line_state(row, line);
        let missing_branches: Vec<String> = row
            .branch_missing
            .iter()
            .filter(|arc| arc.from == i32::try_from(line).unwrap_or(i32::MAX))
            .map(|arc| arc.to.to_string())
            .collect();
        let contexts = line_contexts(row, line);
        write!(
            html,
            "<span class=\"line {state}\"><a id=\"L{line}\" href=\"#L{line}\" class=\"number\">{line:>5}</a> <code>{}</code>",
            escape_html(text)
        )?;
        if !missing_branches.is_empty() {
            write!(
                html,
                " <span class=\"branches\">missing → {}</span>",
                escape_html(&missing_branches.join(", "))
            )?;
        }
        if options.show_contexts && !contexts.is_empty() {
            write!(
                html,
                " <span class=\"contexts\">{}</span>",
                escape_html(&contexts.into_iter().collect::<Vec<_>>().join(", "))
            )?;
        }
        writeln!(html, "</span>")?;
    }
    writeln!(html, "  </pre>")?;
    document_end(&mut html)?;
    Ok(html)
}

fn line_state(row: &FileRow, line: u32) -> &'static str {
    if row.excluded.contains(&line) {
        "excluded"
    } else if row.executed.contains(&line) {
        if row.branch_missing.iter().any(|arc| {
            arc.from == i32::try_from(line).unwrap_or(i32::MAX)
                && row.branch_executed.iter().any(|hit| hit.from == arc.from)
        }) {
            "partial"
        } else {
            "executed"
        }
    } else if row.executable.contains(&line) {
        "missing"
    } else {
        "neutral"
    }
}

fn line_contexts(row: &FileRow, line: u32) -> BTreeSet<String> {
    let mut contexts = row.contexts.get(&line).cloned().unwrap_or_default();
    for (arc, arc_contexts) in &row.arc_contexts {
        if arc.from == i32::try_from(line).unwrap_or(i32::MAX) {
            contexts.extend(arc_contexts.iter().cloned());
        }
    }
    contexts
}

fn source_filename(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut filename = String::with_capacity(12 + path.len() * 2);
    filename.push_str("source-");
    for byte in path.bytes() {
        filename.push(char::from(HEX[usize::from(byte >> 4)]));
        filename.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    filename.push_str(".html");
    filename
}

fn document_start(html: &mut String, title: &str) -> Result<(), fmt::Error> {
    writeln!(html, "<!DOCTYPE html>")?;
    writeln!(html, "<html lang=\"en\">")?;
    writeln!(html, "<head>")?;
    writeln!(html, "  <meta charset=\"utf-8\">")?;
    writeln!(
        html,
        "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">"
    )?;
    writeln!(html, "  <title>{}</title>", escape_html(title))?;
    writeln!(
        html,
        "  <style>body{{font-family:system-ui,sans-serif;margin:2rem;color:#202124}}table{{border-collapse:collapse;width:100%}}th,td{{padding:.5rem;border-bottom:1px solid #ddd;text-align:left}}td.num{{text-align:right;font-variant-numeric:tabular-nums}}code,.source{{font-family:ui-monospace,SFMono-Regular,monospace}}thead{{background:#f5f5f5}}h1{{margin-top:0}}a{{color:#0969da}}.source{{display:block;overflow:auto;background:#f6f8fa;padding:1rem}}.line{{display:block;min-height:1.35em}}.number{{display:inline-block;width:3rem;text-align:right;color:#6e7781;text-decoration:none}}.executed{{background:#dafbe1}}.missing{{background:#ffebe9}}.excluded{{background:#f0f0f0;color:#6e7781}}.partial{{background:#fff8c5}}.branches{{color:#9a6700}}.contexts{{float:right;color:#57606a;font-size:.85em}}</style>"
    )?;
    writeln!(html, "</head>")?;
    writeln!(html, "<body>")
}

fn document_end(html: &mut String) -> Result<(), fmt::Error> {
    writeln!(html, "</body>")?;
    writeln!(html, "</html>")
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::data::BranchArc;

    use super::*;

    #[test]
    fn source_filenames_are_deterministic_and_path_safe() {
        assert_eq!(
            source_filename("src/app.py"),
            "source-7372632f6170702e7079.html"
        );
    }

    #[test]
    fn source_page_annotates_and_escapes_coverage_details() {
        let missing_arc = BranchArc { from: 3, to: 5 };
        let row = FileRow {
            name: "src/<app>.py".to_owned(),
            absolute_name: "/project/src/<app>.py".to_owned(),
            stmts: 3,
            hit: 2,
            miss: 1,
            missing: "2".to_owned(),
            executable: vec![1, 2, 3],
            excluded: vec![4],
            executed: vec![1, 3],
            contexts: BTreeMap::from([(1, BTreeSet::from(["test<&>".to_owned()]))]),
            branches_enabled: true,
            branches: 2,
            branch_hit: 1,
            branch_miss: 1,
            branch_partial: 1,
            branch_possible: vec![BranchArc { from: 3, to: 4 }, missing_arc],
            branch_executed: vec![BranchArc { from: 3, to: 4 }],
            branch_missing: vec![missing_arc],
            arc_contexts: BTreeMap::new(),
        };
        let page = render_source(
            &row,
            "hit = '<&>'\nmiss = 2\nif hit:\n    excluded = 4\n",
            &HtmlReportOptions {
                title: "Coverage <report>".to_owned(),
                show_contexts: true,
                ..HtmlReportOptions::default()
            },
        )
        .expect("render source page");

        insta::assert_snapshot!(page);
    }
}
