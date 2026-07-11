use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context as _, Result};
use camino::Utf8Path;
use karva_cache::AggregatedResults;
use karva_cli::ResultFormat;
use karva_diagnostic::{CapturedTestOutput, TestCaseOutcome, TestCaseResult, TestCaseRetry};
use karva_project::path::absolute;
use serde::Serialize;

const SCHEMA_VERSION: u8 = 1;

pub(super) fn write_result_report(
    path: Option<&Utf8Path>,
    format: ResultFormat,
    results: &AggregatedResults,
    project_root: &Utf8Path,
    elapsed: Duration,
) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };

    let output_path = absolute(path, project_root);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create result report directory `{parent}`"))?;
    }

    let report = RunReport::new(results, elapsed);
    let content = match format {
        ResultFormat::Json => {
            let mut content = serde_json::to_string_pretty(&report)?;
            content.push('\n');
            content
        }
        ResultFormat::Jsonl => build_jsonl_report(&report)?,
    };

    std::fs::write(&output_path, content)
        .with_context(|| format!("failed to write result report `{output_path}`"))?;

    Ok(())
}

fn build_jsonl_report(report: &RunReport<'_>) -> Result<String> {
    let mut output = String::new();
    for test in &report.tests {
        push_jsonl_event(&mut output, "test", test)?;
    }
    if let Some(diagnostics) = report.diagnostics {
        push_jsonl_event(
            &mut output,
            "diagnostics",
            &DiagnosticsEvent { diagnostics },
        )?;
    }
    push_jsonl_event(
        &mut output,
        "run_finished",
        &RunFinishedEvent {
            status: report.status,
            elapsed_seconds: report.elapsed_seconds,
            stats: report.stats,
        },
    )?;
    Ok(output)
}

fn push_jsonl_event<T: Serialize>(output: &mut String, kind: &'static str, data: &T) -> Result<()> {
    let event = JsonlEvent {
        schema_version: SCHEMA_VERSION,
        kind,
        data,
    };
    output.push_str(&serde_json::to_string(&event)?);
    output.push('\n');
    Ok(())
}

#[derive(Serialize)]
struct JsonlEvent<'a, T> {
    schema_version: u8,
    #[serde(rename = "type")]
    kind: &'a str,
    #[serde(flatten)]
    data: T,
}

#[derive(Serialize)]
struct RunReport<'a> {
    schema_version: u8,
    status: RunStatus,
    elapsed_seconds: f64,
    stats: StatsReport,
    tests: Vec<TestReport<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a str>,
}

impl<'a> RunReport<'a> {
    fn new(results: &'a AggregatedResults, elapsed: Duration) -> Self {
        let captured_outputs = captured_outputs_by_test(results);
        let tests = results
            .test_cases
            .iter()
            .map(|case| TestReport::new(case, captured_outputs.get(case.full_name()).copied()))
            .collect();
        let diagnostics = (!results.diagnostics.is_empty()).then_some(results.diagnostics.as_str());
        let status = if results.stats.is_success() && diagnostics.is_none() {
            RunStatus::Passed
        } else {
            RunStatus::Failed
        };

        Self {
            schema_version: SCHEMA_VERSION,
            status,
            elapsed_seconds: elapsed.as_secs_f64(),
            stats: StatsReport::new(results),
            tests,
            diagnostics,
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum RunStatus {
    Passed,
    Failed,
}

#[derive(Clone, Copy, Serialize)]
struct StatsReport {
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    flaky: usize,
    slow: usize,
}

impl StatsReport {
    fn new(results: &AggregatedResults) -> Self {
        Self {
            total: results.stats.total(),
            passed: results.stats.passed(),
            failed: results.stats.failed(),
            skipped: results.stats.skipped(),
            flaky: results.stats.flaky(),
            slow: results.stats.slow(),
        }
    }
}

#[derive(Serialize)]
struct TestReport<'a> {
    module: &'a str,
    name: &'a str,
    full_name: &'a str,
    status: TestStatus,
    duration_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_reason: Option<&'a str>,
    #[serde(skip_serializing_if = "is_false")]
    flaky: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry: Option<RetryReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    captured_output: Option<CapturedOutputReport<'a>>,
}

impl<'a> TestReport<'a> {
    fn new(case: &'a TestCaseResult, output: Option<&'a CapturedTestOutput>) -> Self {
        let (status, skip_reason) = match case.outcome() {
            TestCaseOutcome::Passed => (TestStatus::Passed, None),
            TestCaseOutcome::Failed => (TestStatus::Failed, None),
            TestCaseOutcome::Skipped { reason } => (TestStatus::Skipped, reason.as_deref()),
        };
        let retry = case.retry().map(RetryReport::new);
        let flaky = matches!(case.outcome(), TestCaseOutcome::Passed) && retry.is_some();

        Self {
            module: case.module_name(),
            name: case.name(),
            full_name: case.full_name(),
            status,
            duration_seconds: case.duration().as_secs_f64(),
            skip_reason,
            flaky,
            retry,
            captured_output: output
                .filter(|output| !output.is_empty())
                .map(CapturedOutputReport::new),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum TestStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Clone, Copy, Serialize)]
struct RetryReport {
    attempts: u32,
    max_attempts: u32,
}

impl RetryReport {
    fn new(retry: &TestCaseRetry) -> Self {
        Self {
            attempts: retry.attempts(),
            max_attempts: retry.max_attempts(),
        }
    }
}

#[derive(Serialize)]
struct CapturedOutputReport<'a> {
    #[serde(skip_serializing_if = "str::is_empty")]
    stdout: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    stderr: &'a str,
}

impl<'a> CapturedOutputReport<'a> {
    fn new(output: &'a CapturedTestOutput) -> Self {
        Self {
            stdout: output.stdout(),
            stderr: output.stderr(),
        }
    }
}

#[derive(Serialize)]
struct DiagnosticsEvent<'a> {
    diagnostics: &'a str,
}

#[derive(Serialize)]
struct RunFinishedEvent {
    status: RunStatus,
    elapsed_seconds: f64,
    stats: StatsReport,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if passes a reference to the field"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

fn captured_outputs_by_test(results: &AggregatedResults) -> HashMap<&str, &CapturedTestOutput> {
    results
        .captured_outputs
        .iter()
        .map(|output| (output.test_name(), output))
        .collect()
}
