use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Serialize;

use super::shared::{FileRow, missing_lines, percent, totals_row};

#[derive(Serialize)]
struct JsonFileSummary {
    covered_lines: u32,
    num_statements: u32,
    percent_covered: f64,
    missing_lines: Vec<u32>,
    excluded_lines: Vec<u32>,
}

#[derive(Serialize)]
struct JsonFileReport {
    executed_lines: Vec<u32>,
    summary: JsonFileSummary,
    missing_lines: Vec<u32>,
    excluded_lines: Vec<u32>,
}

#[derive(Serialize)]
struct JsonTotalsSummary {
    covered_lines: u32,
    num_statements: u32,
    percent_covered: f64,
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
}

pub fn build_json_report(rows: &[FileRow]) -> Result<String> {
    let files = rows
        .iter()
        .map(|row| {
            (
                row.name.clone(),
                JsonFileReport {
                    executed_lines: row.executed.clone(),
                    summary: json_summary(row),
                    missing_lines: missing_lines(row),
                    excluded_lines: Vec::new(),
                },
            )
        })
        .collect();

    let totals_row = totals_row(rows);
    let report = JsonReport {
        meta: JsonMeta {
            format: 2,
            version: "karva",
        },
        files,
        totals: json_totals_summary(&totals_row),
    };

    serde_json::to_string_pretty(&report).context("failed to serialize coverage json")
}

fn json_summary(row: &FileRow) -> JsonFileSummary {
    JsonFileSummary {
        covered_lines: row.hit,
        num_statements: row.stmts,
        percent_covered: percent(row.stmts, row.miss),
        missing_lines: missing_lines(row),
        excluded_lines: Vec::new(),
    }
}

fn json_totals_summary(row: &FileRow) -> JsonTotalsSummary {
    JsonTotalsSummary {
        covered_lines: row.hit,
        num_statements: row.stmts,
        percent_covered: percent(row.stmts, row.miss),
    }
}
