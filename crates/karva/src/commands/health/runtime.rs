//! Explicit worker execution for health inspection. Static health never enters here.

use std::time::Instant;

use anyhow::Result;
use karva_cli::{HealthTimings, SubTestCommand};
use karva_diagnostic::{AggregatedResults, LifecycleTiming};
use karva_logging::{FinalStatusLevel, Printer, StatusLevel};
use karva_project::Project;
use karva_runner::{
    FailurePriority, LastFailedSelection, ParallelTestConfig, RunOutput, TestOrdering,
    TestResultRetention,
};
use serde::Serialize;

use super::{Finding, Location};

/// Measurements and their population-derived threshold, without a universal slow limit.
#[derive(Serialize)]
pub(super) struct RuntimeReport {
    pub(super) success: bool,
    /// Serial execution keeps shared fixture lifecycle accounting directly observable.
    workers: usize,
    ordering: &'static str,
    elapsed_seconds: f64,
    /// Callback serialization and IPC time; excludes clocks and source-coordinate construction.
    pub(super) reporting_overhead_seconds: f64,
    /// Upper median of measured invocation durations; only values above it are hotspots.
    pub(super) median_seconds: Option<f64>,
    pub(super) measurements: Vec<LifecycleTiming>,
    pub(super) errors: Vec<String>,
}

pub(super) fn durations(project: &Project, profile: Option<String>) -> Result<RuntimeReport> {
    let start = Instant::now();
    let output = run(project, profile, true, TestOrdering::Stable)?;
    let elapsed_seconds = start.elapsed().as_secs_f64();
    let success = output.results.is_success() && !output.timed_out;
    let errors = execution_errors(&output);
    let mut measurements = output.results.timings;
    for timing in &mut measurements {
        timing.path = timing
            .path
            .strip_prefix(project.cwd())
            .unwrap_or(&timing.path)
            .to_path_buf();
    }
    measurements.sort_by(|a, b| {
        (&a.path, a.start, a.phase, a.attempt).cmp(&(&b.path, b.start, b.phase, b.attempt))
    });
    let mut durations = measurements
        .iter()
        .map(|timing| timing.duration_seconds)
        .collect::<Vec<_>>();
    durations.sort_by(f64::total_cmp);
    let median_seconds = durations.get(durations.len() / 2).copied();
    Ok(RuntimeReport {
        success,
        workers: 1,
        ordering: "stable",
        elapsed_seconds,
        reporting_overhead_seconds: output.results.timing_reporting_seconds,
        median_seconds,
        measurements,
        errors,
    })
}

impl RuntimeReport {
    pub(super) fn hotspots(&self) -> Vec<Finding> {
        let Some(median) = self.median_seconds else {
            return Vec::new();
        };
        let total = self
            .measurements
            .iter()
            .map(|timing| timing.duration_seconds)
            .sum::<f64>();
        let mut ranked = self.measurements.iter().collect::<Vec<_>>();
        ranked.sort_by(|a, b| {
            b.duration_seconds
                .total_cmp(&a.duration_seconds)
                .then_with(|| a.name.cmp(&b.name))
        });
        ranked.into_iter().enumerate().filter(|(_, timing)| timing.duration_seconds > median).map(|(rank,timing)| Finding {
            rule: "duration-hotspot", severity: "info",
            location: Location { path: timing.path.as_str().replace('\\', "/"), line: timing.line, column: timing.column, start: timing.start, end: timing.end },
            evidence: format!("{} {:?}: {:.6}s; rank {} of {}; above measured median {:.6}s; measured phase total {:.6}s; scope {}; dependencies: {}", timing.name, timing.phase, timing.duration_seconds, rank + 1, self.measurements.len(), median, total, timing.scope.as_deref().unwrap_or("test"), timing.dependencies.join(", ")),
            related: Vec::new(), action: "Inspect this measured phase and its dependencies before optimizing; shared fixture invocations are counted once.",
        }).collect()
    }
}

fn run(
    project: &Project,
    profile: Option<String>,
    health_timings: bool,
    ordering: TestOrdering,
) -> Result<RunOutput> {
    let args = SubTestCommand {
        health_timings: if health_timings {
            HealthTimings::Enabled
        } else {
            HealthTimings::Disabled
        },
        ..SubTestCommand::default()
    };
    let config = ParallelTestConfig {
        num_workers: 1,
        no_cache: true,
        create_ctrlc_handler: true,
        last_failed: LastFailedSelection::All,
        failed_first: FailurePriority::Normal,
        profile,
        partition: None,
        test_ordering: ordering,
        result_retention: TestResultRetention::All,
    };
    karva_runner::run_parallel_tests(
        project,
        &config,
        &args,
        Printer::new(StatusLevel::None, FinalStatusLevel::None),
    )
}

fn execution_errors(output: &RunOutput) -> Vec<String> {
    let results: &AggregatedResults = &output.results;
    let mut errors = results
        .run_diagnostics
        .iter()
        .map(|diagnostic| diagnostic.rendered().to_owned())
        .collect::<Vec<_>>();
    errors.extend(results.test_cases.iter().filter_map(|test| {
        test.outcome()
            .diagnostic()
            .map(|diagnostic| format!("{}: {}", test.full_name(), diagnostic.rendered()))
    }));
    if output.timed_out {
        errors.push("Health runtime analysis exceeded the configured run timeout".to_owned());
    }
    errors
}
