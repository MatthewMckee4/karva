use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use anyhow::{Result, bail};

use super::shared::FileRow;
use crate::data::BranchArc;

pub(super) fn build_lcov_report(rows: &[FileRow]) -> Result<String> {
    let mut report = String::new();
    for row in rows {
        writeln!(report, "SF:{}", row.name)?;
        let executed: BTreeSet<u32> = row.executed.iter().copied().collect();
        for line in &row.executable {
            writeln!(report, "DA:{line},{}", u8::from(executed.contains(line)))?;
        }
        if row.stmts > 0 {
            writeln!(report, "LF:{}", row.stmts)?;
            writeln!(report, "LH:{}", row.hit)?;
        }
        write_branches(&mut report, row)?;
        report.push_str("end_of_record\n");
    }
    Ok(report)
}

fn write_branches(report: &mut String, row: &FileRow) -> Result<()> {
    if row.branches == 0 {
        return Ok(());
    }
    let missing: BTreeSet<BranchArc> = row.branch_missing.iter().copied().collect();
    let mut by_line: BTreeMap<u32, Vec<BranchArc>> = BTreeMap::new();
    for arc in &row.branch_possible {
        let Ok(line) = u32::try_from(arc.from) else {
            bail!("cannot write LCOV branch with negative origin {}", arc.from);
        };
        by_line.entry(line).or_default().push(*arc);
    }
    for (line, mut arcs) in by_line {
        arcs.sort_by_key(|arc| (arc.to < 0, arc.to));
        let any_taken = arcs.iter().any(|arc| !missing.contains(arc));
        for (branch, arc) in arcs.iter().enumerate() {
            let taken = if !missing.contains(arc) {
                "1"
            } else if any_taken {
                "0"
            } else {
                "-"
            };
            writeln!(report, "BRDA:{line},0,{branch},{taken}")?;
        }
    }
    writeln!(report, "BRF:{}", row.branches)?;
    writeln!(report, "BRH:{}", row.branch_hit)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::build_lcov_report;
    use crate::data::BranchArc;
    use crate::report::shared::FileRow;

    #[test]
    fn report_contains_deterministic_line_and_branch_records() {
        let taken = BranchArc { from: 1, to: 2 };
        let missing = BranchArc { from: 1, to: -1 };
        let row = FileRow {
            name: "src/app.py".to_owned(),
            absolute_name: "/project/src/app.py".to_owned(),
            stmts: 2,
            hit: 1,
            miss: 1,
            missing: "2".to_owned(),
            executable: vec![1, 2],
            excluded: vec![3],
            executed: vec![1],
            contexts: BTreeMap::new(),
            branches_enabled: true,
            branches: 2,
            branch_hit: 1,
            branch_miss: 1,
            branch_partial: 1,
            branch_possible: vec![missing, taken],
            branch_executed: vec![taken],
            branch_missing: vec![missing],
            arc_contexts: BTreeMap::new(),
        };
        let uncovered = FileRow {
            name: "src/uncovered.py".to_owned(),
            absolute_name: "/project/src/uncovered.py".to_owned(),
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

        let report = build_lcov_report(&[row, uncovered]).expect("build LCOV report");

        insta::assert_snapshot!(report, @r"
        SF:src/app.py
        DA:1,1
        DA:2,0
        LF:2
        LH:1
        BRDA:1,0,0,1
        BRDA:1,0,1,0
        BRF:2
        BRH:1
        end_of_record
        SF:src/uncovered.py
        DA:1,0
        LF:1
        LH:0
        end_of_record
        ");
        let records = lcov::Reader::new(report.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .expect("standard LCOV parser accepts report");
        insta::assert_debug_snapshot!(records, @r#"
        [
            SourceFile {
                path: "src/app.py",
            },
            LineData {
                line: 1,
                count: 1,
                checksum: None,
            },
            LineData {
                line: 2,
                count: 0,
                checksum: None,
            },
            LinesFound {
                found: 2,
            },
            LinesHit {
                hit: 1,
            },
            BranchData {
                line: 1,
                block: 0,
                branch: 0,
                taken: Some(
                    1,
                ),
            },
            BranchData {
                line: 1,
                block: 0,
                branch: 1,
                taken: Some(
                    0,
                ),
            },
            BranchesFound {
                found: 2,
            },
            BranchesHit {
                hit: 1,
            },
            EndOfRecord,
            SourceFile {
                path: "src/uncovered.py",
            },
            LineData {
                line: 1,
                count: 0,
                checksum: None,
            },
            LinesFound {
                found: 1,
            },
            LinesHit {
                hit: 0,
            },
            EndOfRecord,
        ]
        "#);
    }

    #[test]
    fn report_rejects_negative_branch_origins() {
        let row = FileRow {
            name: "src/app.py".to_owned(),
            absolute_name: "/project/src/app.py".to_owned(),
            stmts: 0,
            hit: 0,
            miss: 0,
            missing: String::new(),
            executable: Vec::new(),
            excluded: Vec::new(),
            executed: Vec::new(),
            contexts: BTreeMap::new(),
            branches_enabled: true,
            branches: 1,
            branch_hit: 0,
            branch_miss: 1,
            branch_partial: 0,
            branch_possible: vec![BranchArc { from: -1, to: 1 }],
            branch_executed: Vec::new(),
            branch_missing: vec![BranchArc { from: -1, to: 1 }],
            arc_contexts: BTreeMap::new(),
        };

        let error = build_lcov_report(&[row]).expect_err("reject invalid branch origin");

        assert!(error.to_string().contains("negative origin -1"));
    }

    #[test]
    fn intentionally_partial_arcs_are_covered_in_lcov() {
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

        insta::assert_snapshot!(build_lcov_report(&[row]).expect("build LCOV report"), @r"
        SF:src/app.py
        DA:1,1
        LF:1
        LH:1
        BRDA:1,0,0,1
        BRDA:1,0,1,1
        BRF:2
        BRH:2
        end_of_record
        ");
    }
}
