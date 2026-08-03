use std::collections::BTreeMap;
use std::collections::BTreeSet;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::data::BranchArc;

use super::shared::{FileRow, missing_lines, percent, row_percent, totals_row};

#[derive(Serialize)]
struct JsonFileSummary {
    covered_lines: u32,
    num_statements: u32,
    percent_covered: f64,
    missing_lines: Vec<u32>,
    excluded_lines: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_branches: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_partial_branches: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    covered_branches: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    missing_branches: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    percent_branches_covered: Option<f64>,
}

#[derive(Serialize)]
struct JsonFileReport {
    executed_lines: Vec<u32>,
    summary: JsonFileSummary,
    missing_lines: Vec<u32>,
    excluded_lines: Vec<u32>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    contexts: BTreeMap<u32, BTreeSet<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    executed_branches: Option<Vec<[i32; 2]>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    missing_branches: Option<Vec<[i32; 2]>>,
}

#[derive(Serialize)]
struct JsonTotalsSummary {
    covered_lines: u32,
    num_statements: u32,
    percent_covered: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_branches: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_partial_branches: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    covered_branches: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    missing_branches: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    percent_branches_covered: Option<f64>,
}

#[derive(Serialize)]
struct JsonReport {
    meta: JsonMeta,
    files: BTreeMap<String, JsonFileReport>,
    totals: JsonTotalsSummary,
}

#[derive(Serialize)]
struct JsonMeta {
    format: u32,
    version: &'static str,
    #[serde(skip_serializing_if = "is_false")]
    show_contexts: bool,
}

/// Presentation settings for exported JSON coverage data.
#[derive(Debug, Default)]
pub struct JsonReportOptions {
    /// Whether output uses indentation and line breaks.
    pub pretty_print: bool,
    /// Whether per-line execution contexts are included.
    pub show_contexts: bool,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if passes a reference to the field"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

pub(super) fn build_json_report(rows: &[FileRow], options: &JsonReportOptions) -> Result<String> {
    let files = rows
        .iter()
        .map(|row| {
            (
                row.name.clone(),
                JsonFileReport {
                    executed_lines: row.executed.clone(),
                    summary: json_summary(row),
                    missing_lines: missing_lines(row),
                    excluded_lines: row.excluded.clone(),
                    contexts: if options.show_contexts {
                        row.contexts.clone()
                    } else {
                        BTreeMap::new()
                    },
                    executed_branches: row
                        .branches_enabled
                        .then(|| branch_pairs(&row.branch_executed)),
                    missing_branches: row
                        .branches_enabled
                        .then(|| branch_pairs(&row.branch_missing)),
                },
            )
        })
        .collect();

    let totals_row = totals_row(rows);
    let report = JsonReport {
        meta: JsonMeta {
            format: 2,
            version: "karva",
            show_contexts: options.show_contexts,
        },
        files,
        totals: json_totals_summary(&totals_row),
    };

    if options.pretty_print {
        serde_json::to_string_pretty(&report)
    } else {
        serde_json::to_string(&report)
    }
    .context("failed to serialize coverage json")
}

fn json_summary(row: &FileRow) -> JsonFileSummary {
    JsonFileSummary {
        covered_lines: row.hit,
        num_statements: row.stmts,
        percent_covered: row_percent(row),
        missing_lines: missing_lines(row),
        excluded_lines: row.excluded.clone(),
        num_branches: row.branches_enabled.then_some(row.branches),
        num_partial_branches: row.branches_enabled.then_some(row.branch_partial),
        covered_branches: row.branches_enabled.then_some(row.branch_hit),
        missing_branches: row.branches_enabled.then_some(row.branch_miss),
        percent_branches_covered: row
            .branches_enabled
            .then(|| percent(row.branches, row.branch_miss)),
    }
}

fn json_totals_summary(row: &FileRow) -> JsonTotalsSummary {
    JsonTotalsSummary {
        covered_lines: row.hit,
        num_statements: row.stmts,
        percent_covered: row_percent(row),
        num_branches: row.branches_enabled.then_some(row.branches),
        num_partial_branches: row.branches_enabled.then_some(row.branch_partial),
        covered_branches: row.branches_enabled.then_some(row.branch_hit),
        missing_branches: row.branches_enabled.then_some(row.branch_miss),
        percent_branches_covered: row
            .branches_enabled
            .then(|| percent(row.branches, row.branch_miss)),
    }
}

fn branch_pairs(arcs: &[BranchArc]) -> Vec<[i32; 2]> {
    arcs.iter().map(|arc| [arc.from, arc.to]).collect()
}
