use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use camino::Utf8Path;

use super::shared::{FileRow, class_filename, escape_xml, rate};

pub fn build_cobertura_xml(
    cwd: &Utf8Path,
    cwd_real: &std::path::Path,
    rows: &[FileRow],
) -> Result<String> {
    let total_stmts = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.stmts));
    let total_hit = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.hit));
    let line_rate = rate(total_hit, total_stmts);
    let timestamp = std::fs::metadata(cwd.as_std_path())
        .with_context(|| format!("failed to read coverage root metadata {cwd}"))?
        .modified()
        .with_context(|| format!("failed to read coverage root modification time {cwd}"))?
        .duration_since(UNIX_EPOCH)
        .with_context(|| format!("coverage root modification time is before UNIX epoch: {cwd}"))?
        .as_secs();
    let source_root = cwd_real.to_string_lossy().trim_end_matches('/').to_string();

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" ?>\n");
    writeln!(
        xml,
        "<coverage version=\"1.0\" timestamp=\"{timestamp}\" lines-valid=\"{total_stmts}\" lines-covered=\"{total_hit}\" line-rate=\"{line_rate:.4}\" branches-covered=\"0\" branches-valid=\"0\" branch-rate=\"0.0000\" complexity=\"0.0\">"
    )?;
    xml.push_str("  <sources>\n");
    writeln!(xml, "    <source>{}</source>", escape_xml(&source_root))?;
    xml.push_str("  </sources>\n");
    xml.push_str("  <packages>\n");
    writeln!(
        xml,
        "    <package name=\".\" line-rate=\"{line_rate:.4}\" branch-rate=\"0.0000\" complexity=\"0.0\">",
    )?;
    xml.push_str("      <classes>\n");

    for row in rows {
        let filename = class_filename(row, cwd_real);
        writeln!(
            xml,
            "        <class name=\"{}\" filename=\"{}\" line-rate=\"{:.4}\" branch-rate=\"0.0000\" complexity=\"0.0\">",
            escape_xml(&row.name),
            escape_xml(&filename),
            rate(row.hit, row.stmts)
        )?;
        xml.push_str("          <methods/>\n");
        xml.push_str("          <lines>\n");
        let executed: BTreeSet<u32> = row.executed.iter().copied().collect();
        for line in &row.executable {
            let hits = i32::from(executed.contains(line));
            writeln!(
                xml,
                "            <line number=\"{line}\" hits=\"{hits}\" branch=\"false\"/>"
            )?;
        }
        xml.push_str("          </lines>\n");
        xml.push_str("        </class>\n");
    }

    xml.push_str("      </classes>\n");
    xml.push_str("    </package>\n");
    xml.push_str("  </packages>\n");
    xml.push_str("</coverage>\n");
    Ok(xml)
}

#[cfg(test)]
mod tests {
    use camino::Utf8Path;

    use super::build_cobertura_xml;

    #[test]
    fn build_cobertura_xml_reports_missing_coverage_root_metadata() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let missing = temp_dir.path().join("missing");
        let missing = Utf8Path::from_path(&missing).expect("temp path should be UTF-8");

        let err = build_cobertura_xml(missing, missing.as_std_path(), &[])
            .expect_err("missing coverage root should fail");

        assert!(
            err.to_string()
                .contains("failed to read coverage root metadata"),
            "{err:?}"
        );
    }
}
