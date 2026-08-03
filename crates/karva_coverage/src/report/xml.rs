use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

use super::shared::{FileRow, class_filename, escape_xml, rate};

pub(super) fn build_cobertura_xml(
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
    let total_branches = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.branches));
    let total_branch_hit = rows
        .iter()
        .fold(0_u32, |acc, row| acc.saturating_add(row.branch_hit));
    let branch_mode = rows.iter().any(|row| row.branches_enabled);
    let branch_rate = if branch_mode {
        rate(total_branch_hit, total_branches)
    } else {
        0.0
    };
    let timestamp = fs::metadata(cwd)
        .with_context(|| format!("failed to read coverage root metadata {cwd}"))?
        .modified()
        .with_context(|| format!("failed to read coverage root modification time {cwd}"))?
        .duration_since(UNIX_EPOCH)
        .with_context(|| format!("coverage root modification time is before UNIX epoch: {cwd}"))?
        .as_secs();
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" ?>\n");
    writeln!(
        xml,
        "<coverage version=\"1.0\" timestamp=\"{timestamp}\" lines-valid=\"{total_stmts}\" lines-covered=\"{total_hit}\" line-rate=\"{line_rate:.4}\" branches-covered=\"{total_branch_hit}\" branches-valid=\"{total_branches}\" branch-rate=\"{branch_rate:.4}\" complexity=\"0.0\">"
    )?;
    xml.push_str("  <sources>\n");
    xml.push_str("    <source>.</source>\n");
    xml.push_str("  </sources>\n");
    xml.push_str("  <packages>\n");
    writeln!(
        xml,
        "    <package name=\".\" line-rate=\"{line_rate:.4}\" branch-rate=\"{branch_rate:.4}\" complexity=\"0.0\">",
    )?;
    xml.push_str("      <classes>\n");

    for row in rows {
        let filename = class_filename(row, cwd_real);
        writeln!(
            xml,
            "        <class name=\"{}\" filename=\"{}\" line-rate=\"{:.4}\" branch-rate=\"{:.4}\" complexity=\"0.0\">",
            escape_xml(&row.name),
            escape_xml(&filename),
            rate(row.hit, row.stmts),
            if row.branches_enabled {
                rate(row.branch_hit, row.branches)
            } else {
                0.0
            }
        )?;
        xml.push_str("          <methods/>\n");
        xml.push_str("          <lines>\n");
        let executed: BTreeSet<u32> = row.executed.iter().copied().collect();
        let branch_lines = branch_lines(row);
        for line in &row.executable {
            let hits = i32::from(executed.contains(line));
            if let Some((covered, total)) = branch_lines.get(line) {
                let pct = rate(*covered, *total) * 100.0;
                writeln!(
                    xml,
                    "            <line number=\"{line}\" hits=\"{hits}\" branch=\"true\" condition-coverage=\"{pct:.0}% ({covered}/{total})\"/>"
                )?;
            } else {
                writeln!(
                    xml,
                    "            <line number=\"{line}\" hits=\"{hits}\" branch=\"false\"/>"
                )?;
            }
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

fn branch_lines(row: &FileRow) -> BTreeMap<u32, (u32, u32)> {
    let missing: BTreeSet<_> = row.branch_missing.iter().copied().collect();
    let mut lines: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
    for arc in &row.branch_possible {
        let Ok(line) = u32::try_from(arc.from) else {
            continue;
        };
        let entry = lines.entry(line).or_default();
        if !missing.contains(arc) {
            entry.0 = entry.0.saturating_add(1);
        }
        entry.1 = entry.1.saturating_add(1);
    }
    lines
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use camino::Utf8Path;
    use pyo3::prelude::*;
    use pyo3::types::PyAnyMethods;

    use super::{branch_lines, build_cobertura_xml};
    use crate::data::BranchArc;
    use crate::report::shared::FileRow;

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

    #[test]
    fn intentionally_partial_arcs_are_covered_in_xml() {
        let taken = BranchArc { from: 1, to: 2 };
        let suppressed = BranchArc { from: 1, to: 3 };
        let row = FileRow {
            name: "src/app.py".to_owned(),
            absolute_name: "/project/src/app.py".to_owned(),
            stmts: 1,
            hit: 1,
            miss: 0,
            missing: String::new(),
            executable: vec![1],
            excluded: Vec::new(),
            executed: vec![1],
            contexts: BTreeMap::new(),
            branches_enabled: true,
            branches: 2,
            branch_hit: 2,
            branch_miss: 0,
            branch_partial: 0,
            branch_possible: vec![taken, suppressed],
            branch_executed: vec![taken],
            branch_missing: Vec::new(),
            arc_contexts: BTreeMap::new(),
        };

        assert_eq!(branch_lines(&row), BTreeMap::from([(1, (2, 2))]));
    }

    #[test]
    fn standard_xml_parser_accepts_portable_branch_report() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let cwd = Utf8Path::from_path(directory.path()).expect("UTF-8 temporary path");
        let taken = BranchArc { from: 2, to: 3 };
        let missing = BranchArc { from: 2, to: 5 };
        let row = FileRow {
            name: "src/app.py".to_owned(),
            absolute_name: "/workspace/src/app.py".to_owned(),
            stmts: 3,
            hit: 2,
            miss: 1,
            missing: "3".to_owned(),
            executable: vec![1, 2, 3],
            excluded: vec![4],
            executed: vec![1, 2],
            contexts: BTreeMap::new(),
            branches_enabled: true,
            branches: 2,
            branch_hit: 1,
            branch_miss: 1,
            branch_partial: 1,
            branch_possible: vec![taken, missing],
            branch_executed: vec![taken],
            branch_missing: vec![missing],
            arc_contexts: BTreeMap::new(),
        };
        let uncovered = FileRow {
            name: "src/uncovered.py".to_owned(),
            absolute_name: "/workspace/src/uncovered.py".to_owned(),
            stmts: 1,
            hit: 0,
            miss: 1,
            missing: "1".to_owned(),
            executable: vec![1],
            excluded: Vec::new(),
            executed: Vec::new(),
            contexts: BTreeMap::new(),
            branches_enabled: true,
            branches: 0,
            branch_hit: 0,
            branch_miss: 0,
            branch_partial: 0,
            branch_possible: Vec::new(),
            branch_executed: Vec::new(),
            branch_missing: Vec::new(),
            arc_contexts: BTreeMap::new(),
        };
        let report = build_cobertura_xml(cwd, Path::new("/workspace"), &[row, uncovered])
            .expect("build Cobertura report");
        Python::initialize();
        Python::attach(|py| -> PyResult<()> {
            let root = py
                .import("xml.etree.ElementTree")?
                .call_method1("fromstring", (&report,))?;
            assert_eq!(root.getattr("tag")?.extract::<String>()?, "coverage");
            assert_eq!(
                root.call_method1("findtext", ("./sources/source",))?
                    .extract::<String>()?,
                "."
            );
            let mut filenames = Vec::new();
            for class in root.call_method1("findall", (".//class",))?.try_iter()? {
                filenames.push(
                    class?
                        .getattr("attrib")?
                        .get_item("filename")?
                        .extract::<String>()?,
                );
            }
            assert_eq!(filenames, ["src/app.py", "src/uncovered.py"]);
            let branch = root.call_method1("find", (".//line[@branch='true']",))?;
            assert!(!branch.is_none());
            assert_eq!(
                branch
                    .getattr("attrib")?
                    .get_item("condition-coverage")?
                    .extract::<String>()?,
                "50% (1/2)"
            );
            assert!(
                root.call_method1("find", (".//line[@number='4']",))?
                    .is_none()
            );
            Ok(())
        })
        .expect("standard XML parser accepts report");
    }
}
