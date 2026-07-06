use std::fmt;

use super::shared::{FileRow, escape_html, format_percent, totals_row};

pub(super) fn build_html_report(rows: &[FileRow]) -> String {
    HtmlReport { rows }.to_string()
}

struct HtmlReport<'a> {
    rows: &'a [FileRow],
}

impl fmt::Display for HtmlReport<'_> {
    fn fmt(&self, html: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(html, "<!DOCTYPE html>")?;
        writeln!(html, "<html lang=\"en\">")?;
        writeln!(html, "<head>")?;
        writeln!(html, "  <meta charset=\"utf-8\">")?;
        writeln!(
            html,
            "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">"
        )?;
        writeln!(html, "  <title>Coverage report</title>")?;
        writeln!(
            html,
            "  <style>body{{font-family:system-ui,sans-serif;margin:2rem;}}table{{border-collapse:collapse;width:100%;}}th,td{{padding:.5rem;border-bottom:1px solid #ddd;text-align:left;}}td.num{{text-align:right;font-variant-numeric:tabular-nums;}}code{{font-family:ui-monospace,SFMono-Regular,monospace;}}thead{{background:#f5f5f5;}}h1{{margin-top:0;}}</style>"
        )?;
        writeln!(html, "</head>")?;
        writeln!(html, "<body>")?;
        writeln!(html, "  <h1>Coverage report</h1>")?;

        let total = totals_row(self.rows);
        writeln!(
            html,
            "  <p>Total coverage: <strong>{}</strong> ({}/{})</p>",
            format_percent(total.stmts, total.miss),
            total.hit,
            total.stmts
        )?;
        writeln!(html, "  <table>")?;
        writeln!(html, "    <thead>")?;
        writeln!(
            html,
            "      <tr><th>Name</th><th>Stmts</th><th>Miss</th><th>Cover</th><th>Missing</th></tr>"
        )?;
        writeln!(html, "    </thead>")?;
        writeln!(html, "    <tbody>")?;
        for row in self.rows {
            writeln!(
                html,
                "      <tr><td><code>{}</code></td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td><code>{}</code></td></tr>",
                escape_html(&row.name),
                row.stmts,
                row.miss,
                format_percent(row.stmts, row.miss),
                escape_html(&row.missing)
            )?;
        }
        writeln!(
            html,
            "      <tr><td><strong>TOTAL</strong></td><td class=\"num\"><strong>{}</strong></td><td class=\"num\"><strong>{}</strong></td><td class=\"num\"><strong>{}</strong></td><td></td></tr>",
            total.stmts,
            total.miss,
            format_percent(total.stmts, total.miss)
        )?;
        writeln!(html, "    </tbody>")?;
        writeln!(html, "  </table>")?;
        writeln!(html, "</body>")?;
        writeln!(html, "</html>")
    }
}
