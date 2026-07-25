use std::time::Duration;

use anyhow::{Context as _, Result};
use camino::Utf8Path;
use karva_cache::AggregatedResults;
use karva_cli::ResultFormat;
use karva_diagnostic::{
    CapturedTestOutput, RenderedDiagnostic, TestCaseAttempt, TestCaseOutcome, TestCaseResult,
    TestCaseRetry,
};
use karva_project::path::absolute;
use serde::Serialize;

use crate::ExitStatus;

const SCHEMA_VERSION: u8 = 3;

pub(super) fn write_result_report(
    path: Option<&Utf8Path>,
    format: ResultFormat,
    results: &AggregatedResults,
    project_root: &Utf8Path,
    elapsed: Duration,
    exit_status: ExitStatus,
) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };

    let output_path = absolute(path, project_root);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create result report directory `{parent}`"))?;
    }

    let report = RunReport::new(results, elapsed, RunStatus::from(exit_status));
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
        push_jsonl_record(&mut output, "test", test)?;
    }
    if let Some(diagnostics) = &report.run_diagnostics {
        for diagnostic in diagnostics {
            push_jsonl_record(
                &mut output,
                "run_diagnostic",
                &RunDiagnosticRecord {
                    diagnostic: *diagnostic,
                },
            )?;
        }
    }
    push_jsonl_record(
        &mut output,
        "run_finished",
        &RunFinishedRecord {
            status: report.status,
            elapsed_seconds: report.elapsed_seconds,
            stats: report.stats,
        },
    )?;
    Ok(output)
}

fn push_jsonl_record<T: Serialize>(
    output: &mut String,
    kind: &'static str,
    data: &T,
) -> Result<()> {
    let record = JsonlRecord {
        schema_version: SCHEMA_VERSION,
        kind,
        data,
    };
    output.push_str(&serde_json::to_string(&record)?);
    output.push('\n');
    Ok(())
}

#[derive(Serialize)]
struct JsonlRecord<'a, T> {
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
    run_diagnostics: Option<Vec<DiagnosticReport<'a>>>,
}

impl<'a> RunReport<'a> {
    fn new(results: &'a AggregatedResults, elapsed: Duration, status: RunStatus) -> Self {
        let tests = results.test_cases.iter().map(TestReport::new).collect();
        let run_diagnostics = (!results.run_diagnostics.is_empty()).then(|| {
            results
                .run_diagnostics
                .iter()
                .map(DiagnosticReport::new)
                .collect()
        });

        Self {
            schema_version: SCHEMA_VERSION,
            status,
            elapsed_seconds: elapsed.as_secs_f64(),
            stats: StatsReport::new(results),
            tests,
            run_diagnostics,
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum RunStatus {
    Passed,
    Failed,
}

impl From<ExitStatus> for RunStatus {
    fn from(value: ExitStatus) -> Self {
        match value {
            ExitStatus::Success => Self::Passed,
            ExitStatus::Failure | ExitStatus::Error => Self::Failed,
        }
    }
}

#[derive(Clone, Copy, Serialize)]
struct StatsReport {
    total: usize,
    passed: usize,
    failed: usize,
    errors: usize,
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
            errors: results.stats.errors(),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic: Option<DiagnosticReport<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    related_diagnostics: Vec<DiagnosticReport<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attempts: Vec<AttemptReport<'a>>,
}

impl<'a> TestReport<'a> {
    fn new(case: &'a TestCaseResult) -> Self {
        let (status, skip_reason, diagnostic) = match case.outcome() {
            TestCaseOutcome::Passed => (TestStatus::Passed, None, None),
            TestCaseOutcome::Failed { diagnostic, .. } => {
                (TestStatus::Failed, None, Some(diagnostic))
            }
            TestCaseOutcome::Error { diagnostic, .. } => {
                (TestStatus::Error, None, Some(diagnostic))
            }
            TestCaseOutcome::Skipped { reason } => (TestStatus::Skipped, reason.as_deref(), None),
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
            captured_output: case.captured_output().map(CapturedOutputReport::new),
            diagnostic: diagnostic.map(DiagnosticReport::new),
            related_diagnostics: case
                .outcome()
                .related_diagnostics()
                .iter()
                .map(DiagnosticReport::new)
                .collect(),
            attempts: case.attempts().iter().map(AttemptReport::new).collect(),
        }
    }
}

#[derive(Serialize)]
struct AttemptReport<'a> {
    attempt: u32,
    status: TestStatus,
    duration_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic: Option<DiagnosticReport<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    related_diagnostics: Vec<DiagnosticReport<'a>>,
}

impl<'a> AttemptReport<'a> {
    fn new(attempt: &'a TestCaseAttempt) -> Self {
        let (status, diagnostic) = match attempt.outcome() {
            TestCaseOutcome::Passed => (TestStatus::Passed, None),
            TestCaseOutcome::Failed { diagnostic, .. } => {
                (TestStatus::Failed, Some(DiagnosticReport::new(diagnostic)))
            }
            TestCaseOutcome::Error { diagnostic, .. } => {
                (TestStatus::Error, Some(DiagnosticReport::new(diagnostic)))
            }
            TestCaseOutcome::Skipped { .. } => (TestStatus::Skipped, None),
        };
        Self {
            attempt: attempt.attempt(),
            status,
            duration_seconds: attempt.duration().as_secs_f64(),
            diagnostic,
            related_diagnostics: attempt
                .outcome()
                .related_diagnostics()
                .iter()
                .map(DiagnosticReport::new)
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum TestStatus {
    Passed,
    Failed,
    Error,
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
struct RunDiagnosticRecord<'a> {
    diagnostic: DiagnosticReport<'a>,
}

#[derive(Clone, Copy, Serialize)]
struct DiagnosticReport<'a> {
    code: &'a str,
    severity: &'static str,
    message: &'a str,
    rendered: &'a str,
}

impl<'a> DiagnosticReport<'a> {
    fn new(diagnostic: &'a RenderedDiagnostic) -> Self {
        Self {
            code: diagnostic.code(),
            severity: diagnostic.severity_name(),
            message: diagnostic.message(),
            rendered: diagnostic.rendered(),
        }
    }
}

#[derive(Serialize)]
struct RunFinishedRecord {
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
