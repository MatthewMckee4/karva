use std::fmt::Write as _;

use super::shared::{FileRow, escape_html, format_percent, totals_row};

pub fn build_html_report(rows: &[FileRow]) -> String {
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    html.push_str("  <meta charset=\"utf-8\">\n");
    html.push_str("  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str("  <title>Coverage report</title>\n");
    html.push_str("  <style>body{font-family:system-ui,sans-serif;margin:2rem;}table{border-collapse:collapse;width:100%;}th,td{padding:.5rem;border-bottom:1px solid #ddd;text-align:left;}td.num{text-align:right;font-variant-numeric:tabular-nums;}code{font-family:ui-monospace,SFMono-Regular,monospace;}thead{background:#f5f5f5;}h1{margin-top:0;}</style>\n");
    html.push_str("</head>\n<body>\n");
    html.push_str("  <h1>Coverage report</h1>\n");
    let total = totals_row(rows);
    let _ = writeln!(
        html,
        "  <p>Total coverage: <strong>{}</strong> ({}/{})</p>",
        format_percent(total.stmts, total.miss),
        total.hit,
        total.stmts
    );
    html.push_str("  <table>\n    <thead>\n      <tr><th>Name</th><th>Stmts</th><th>Miss</th><th>Cover</th><th>Missing</th></tr>\n    </thead>\n    <tbody>\n");
    for row in rows {
        let _ = writeln!(
            html,
            "      <tr><td><code>{}</code></td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td><code>{}</code></td></tr>",
            escape_html(&row.name),
            row.stmts,
            row.miss,
            format_percent(row.stmts, row.miss),
            escape_html(&row.missing)
        );
    }
    let _ = writeln!(
        html,
        "      <tr><td><strong>TOTAL</strong></td><td class=\"num\"><strong>{}</strong></td><td class=\"num\"><strong>{}</strong></td><td class=\"num\"><strong>{}</strong></td><td></td></tr>",
        total.stmts,
        total.miss,
        format_percent(total.stmts, total.miss)
    );
    html.push_str("    </tbody>\n  </table>\n</body>\n</html>\n");
    html
}
