//! Baseline validation, conservative coverage selection, and isolated parallel execution.

use std::collections::BTreeSet;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, unbounded};
use karva_cli::{CovContext, SubTestCommand};
use karva_coverage::native::NativeCoverage;
use karva_diagnostic::AggregatedResults;
use karva_ipc::MutationSource;
use karva_logging::{FinalStatusLevel, Printer, StatusLevel};
use karva_metadata::{RunTimeoutSecs, TerminalOptions};
use karva_mutation::{Mutant, MutationRecord, MutationResults, Outcome};
use karva_project::Project;
use karva_runner::{ParallelTestConfig, RunOutput};

use super::files;

fn quiet() -> Printer {
    Printer::new(StatusLevel::None, FinalStatusLevel::None)
}

fn config(profile: Option<String>) -> ParallelTestConfig {
    ParallelTestConfig {
        num_workers: 1,
        no_cache: true,
        create_ctrlc_handler: false,
        last_failed: karva_runner::LastFailedSelection::All,
        failed_first: karva_runner::FailurePriority::Normal,
        profile,
        partition: None,
        test_ordering: karva_runner::TestOrdering::Stable,
        result_retention: karva_runner::TestResultRetention::All,
    }
}

/// Baseline evidence shared by all candidates in this run.
pub(super) struct Baseline {
    pub(super) elapsed_seconds: f64,
    pub(super) output: RunOutput,
    coverage: NativeCoverage,
    tests: BTreeSet<String>,
}

/// Measures full-suite work with test contexts and no historical selection/cache.
pub(super) fn baseline(
    project: &Project,
    sources: &[String],
    profile: Option<String>,
) -> Result<Baseline> {
    let mut metadata = project.metadata().clone();
    let coverage = metadata.options.coverage.get_or_insert_default();
    coverage.sources = Some(sources.to_vec());
    coverage.disabled = Some(false);
    coverage.context = None;
    coverage.branch = Some(false);
    coverage.exclude_lines = Some(Vec::new());
    coverage.omit = Some(Vec::new());
    coverage.include = Some(Vec::new());
    metadata.options.terminal = Some(TerminalOptions {
        status_level: Some(StatusLevel::None),
        final_status_level: Some(FinalStatusLevel::None),
        ..TerminalOptions::default()
    });
    let measured = Project::from_metadata(metadata);
    let args = SubTestCommand {
        cov_context: Some(CovContext::Test),
        ..SubTestCommand::default()
    };
    let mut configuration = config(profile);
    configuration.create_ctrlc_handler = true;
    let start = Instant::now();
    let output = karva_runner::run_parallel_tests(&measured, &configuration, &args, quiet())?;
    let elapsed_seconds = start.elapsed().as_secs_f64();
    let coverage = NativeCoverage::from_worker_files(
        project.cwd(),
        karva_coverage::native::CoverageMode::Line,
        &output.coverage_files,
    )?;
    let tests = output
        .results
        .test_cases
        .iter()
        .filter(|case| !case.outcome().is_skipped())
        .map(|case| case.full_name().to_owned())
        .collect();
    Ok(Baseline {
        elapsed_seconds,
        output,
        coverage,
        tests,
    })
}

/// Extracts escaped test names from contexts and rejects session/unknown coverage.
fn affected_tests(baseline: &Baseline, mutant: &Mutant) -> Option<BTreeSet<String>> {
    let file = baseline.coverage.files.get(&mutant.path)?;
    let end_line = mutant.line
        + mutant
            .original
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
    let mut tests = BTreeSet::new();
    for line in mutant.line..=end_line {
        let Ok(line) = u32::try_from(line) else {
            return None;
        };
        let Some(contexts) = file.line_contexts.get(&line) else {
            continue;
        };
        for context in contexts {
            let mut components = vec![String::new()];
            let mut escaped = false;
            for character in context.chars() {
                if escaped {
                    if let Some(component) = components.last_mut() {
                        component.push(character);
                    }
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == '|' {
                    components.push(String::new());
                } else if let Some(component) = components.last_mut() {
                    component.push(character);
                }
            }
            let [test, phase] = components.as_slice() else {
                return None;
            };
            if !matches!(phase.as_str(), "setup" | "run" | "teardown")
                || !baseline.tests.contains(test)
            {
                return None;
            }
            tests.insert(test.clone());
        }
    }
    (!tests.is_empty()).then_some(tests)
}

fn classify(output: &RunOutput) -> Outcome {
    let diagnostics = output.results.run_diagnostics.iter().chain(
        output
            .results
            .test_cases
            .iter()
            .filter_map(|case| case.outcome().diagnostic()),
    );
    let codes: Vec<_> = diagnostics
        .map(karva_diagnostic::RenderedDiagnostic::code)
        .collect();
    if output.cancelled {
        Outcome::Pending
    } else if codes.contains(&"mutation-invalid") {
        Outcome::Invalid
    } else if output.timed_out
        || codes
            .iter()
            .any(|code| code.contains("timeout") || code.contains("timed-out"))
    {
        Outcome::TimedOut
    } else if codes
        .iter()
        .any(|code| code.contains("worker-") || *code == "interrupted")
    {
        Outcome::Errored
    } else if !output.results.is_success() {
        Outcome::Killed
    } else if codes.contains(&"mutation-uncovered") {
        Outcome::Uncovered
    } else {
        Outcome::Survived
    }
}

fn diagnostics(results: &AggregatedResults) -> String {
    let mut text = String::new();
    for diagnostic in &results.run_diagnostics {
        text.push_str(diagnostic.rendered());
    }
    for case in &results.test_cases {
        if let Some(diagnostic) = case.outcome().diagnostic() {
            text.push_str(case.full_name());
            text.push_str(":\n");
            text.push_str(diagnostic.rendered());
            for related in case.outcome().related_diagnostics() {
                text.push_str(related.rendered());
            }
        }
    }
    text
}

struct Execution<'a> {
    project: &'a Project,
    baseline: &'a Baseline,
    profile: Option<String>,
    budget_seconds: f64,
    full_suite: bool,
    context: &'a str,
}

impl Execution<'_> {
    fn candidate(&self, mutant: Mutant, shutdown: &Receiver<()>) -> MutationRecord {
        let start = Instant::now();
        let mut record = MutationRecord {
            mutant,
            context_fingerprint: self.context.to_owned(),
            outcome: Outcome::Pending,
            selected_tests: Vec::new(),
            duration_seconds: 0.0,
            reason: String::new(),
            diagnostic: None,
        };
        if let Err(error) = self.run(&mut record, shutdown) {
            record.outcome = Outcome::Errored;
            record.diagnostic = Some(format!("mutant {}: {error:#}", record.mutant.id));
        }
        record.duration_seconds = start.elapsed().as_secs_f64();
        record
    }

    fn run(&self, record: &mut MutationRecord, shutdown: &Receiver<()>) -> Result<()> {
        let path = files::canonical_source(
            self.project.cwd(),
            &self.project.cwd().join(&record.mutant.path),
        )?;
        let source = std::fs::read_to_string(&path)?;
        let mutation = MutationSource {
            id: record.mutant.id.clone(),
            path,
            original_fingerprint: record.mutant.source_fingerprint.clone(),
            source: record.mutant.apply(&source)?,
        };
        let mut metadata = self.project.metadata().clone();
        metadata.options.coverage.get_or_insert_default().sources = Some(Vec::new());
        metadata.options.junit = None;
        metadata.options.terminal = Some(TerminalOptions {
            status_level: Some(StatusLevel::None),
            final_status_level: Some(FinalStatusLevel::None),
            ..TerminalOptions::default()
        });
        metadata.options.test.get_or_insert_default().run_timeout =
            Some(RunTimeoutSecs(self.budget_seconds));
        let project = Project::from_metadata(metadata);
        let configuration = config(self.profile.clone());
        let mut args = SubTestCommand::default();
        let mut requested = self.baseline.tests.clone();
        let affected = (!self.full_suite)
            .then(|| affected_tests(self.baseline, &record.mutant))
            .flatten();
        if let Some(tests) = affected.filter(|tests| tests.len() < self.baseline.tests.len()) {
            args.filter_expressions = tests
                .iter()
                .map(|test| serde_json::to_string(test).map(|quoted| format!("test(={quoted})")))
                .collect::<std::result::Result<_, _>>()?;
            requested = tests;
            record.reason = "affected tests first; full-suite safety check before survival".into();
        } else {
            record.reason = "full suite: requested or source/test mapping incomplete".into();
        }
        let start = Instant::now();
        let mut output = karva_runner::run_mutation_tests(
            &project,
            &configuration,
            &args,
            quiet(),
            mutation.clone(),
            shutdown,
        )?;
        let mut tests: BTreeSet<_> = output
            .results
            .test_cases
            .iter()
            .filter(|case| !case.outcome().is_skipped())
            .map(|case| case.full_name().to_owned())
            .collect();
        tests.extend(output.incomplete_tests.iter().cloned());
        let mut outcome = classify(&output);
        if matches!(outcome, Outcome::Survived | Outcome::Uncovered)
            && !args.filter_expressions.is_empty()
        {
            args.filter_expressions.clear();
            requested.clone_from(&self.baseline.tests);
            // Both stages share one per-mutant budget, rather than silently
            // granting a second full budget after the affected stage.
            let remaining = self.budget_seconds - start.elapsed().as_secs_f64();
            if remaining <= 0.0 {
                outcome = Outcome::TimedOut;
            } else {
                let mut metadata = project.metadata().clone();
                metadata.options.test.get_or_insert_default().run_timeout =
                    Some(RunTimeoutSecs(remaining));
                output = karva_runner::run_mutation_tests(
                    &Project::from_metadata(metadata),
                    &configuration,
                    &args,
                    quiet(),
                    mutation,
                    shutdown,
                )?;
                tests.extend(
                    output
                        .results
                        .test_cases
                        .iter()
                        .filter(|case| !case.outcome().is_skipped())
                        .map(|case| case.full_name().to_owned()),
                );
                tests.extend(output.incomplete_tests.iter().cloned());
                outcome = classify(&output);
            }
        }
        record.outcome = outcome;
        record.selected_tests = tests.into_iter().collect();
        if outcome == Outcome::TimedOut {
            record.diagnostic = Some(format!(
                "mutant {}: mutation-timeout budget exceeded; limit {:.6}s, requested {} tests ({}), measured baseline {:.6}s; test evidence: {}",
                record.mutant.id,
                self.budget_seconds,
                requested.len(),
                requested.iter().cloned().collect::<Vec<_>>().join(", "),
                self.baseline.elapsed_seconds,
                diagnostics(&output.results)
            ));
        } else if outcome == Outcome::Errored {
            record.diagnostic = Some(format!(
                "mutant {}: worker execution failed; requested {} tests ({}), measured baseline {:.6}s, budget {:.6}s; evidence: {}",
                record.mutant.id,
                requested.len(),
                requested.iter().cloned().collect::<Vec<_>>().join(", "),
                self.baseline.elapsed_seconds,
                self.budget_seconds,
                diagnostics(&output.results)
            ));
        } else if matches!(outcome, Outcome::Killed | Outcome::Invalid) {
            record.diagnostic = Some(diagnostics(&output.results));
        } else if outcome == Outcome::Uncovered {
            record.reason =
                "uncovered: source was not imported; full suite safety check passed".into();
        }
        Ok(())
    }
}

/// Scheduling and output policy for one set of candidate workers.
pub(super) struct RunSettings {
    pub(super) workers: usize,
    pub(super) budget_seconds: f64,
    pub(super) profile: Option<String>,
    pub(super) full_suite: bool,
    pub(super) json: bool,
}

/// Runs bounded parallel candidate streams and persists each completed result.
///
/// Cancellation is broadcast to every active controller. Unstarted/interrupted
/// candidates remain pending, so an interrupted invocation resumes valid work.
pub(super) fn run(
    project: &Project,
    baseline: &Baseline,
    results: &mut MutationResults,
    pending: Vec<Mutant>,
    settings: RunSettings,
) -> Result<bool> {
    let RunSettings {
        workers,
        budget_seconds,
        profile,
        full_suite,
        json,
    } = settings;
    let path = project.cwd().join(".karva/mutation/results.json");
    let shutdown = karva_runner::shutdown_receiver();
    let cancelled = AtomicBool::new(false);
    let context = results.context_fingerprint.clone();
    let total = pending.len();
    let execution = Execution {
        project,
        baseline,
        profile,
        budget_seconds,
        full_suite,
        context: &context,
    };
    let (jobs_tx, jobs_rx) = unbounded();
    for mutant in pending {
        jobs_tx.send(mutant)?;
    }
    drop(jobs_tx);
    std::thread::scope(|scope| -> Result<bool> {
        let (completed_tx, completed_rx) = unbounded();
        let mut cancellation = Vec::new();
        for index in 0..workers.min(jobs_rx.len()) {
            let (cancel_tx, cancel_rx) = unbounded();
            cancellation.push(cancel_tx);
            let jobs = jobs_rx.clone();
            let completed = completed_tx.clone();
            let execution = &execution;
            let cancelled = &cancelled;
            let worker = std::thread::Builder::new().spawn_scoped(scope, move || {
                while let Ok(mutant) = jobs.recv() {
                    if cancelled.load(Ordering::Relaxed) {
                        break;
                    }
                    let record = execution.candidate(mutant, &cancel_rx);
                    if completed.send(record).is_err() {
                        break;
                    }
                }
            });
            if let Err(error) = worker {
                cancelled.store(true, Ordering::Relaxed);
                for sender in &cancellation {
                    let _ = sender.send(());
                }
                return Err(error).with_context(|| format!("cannot start mutation worker {index}; requested {workers} workers for {total} mutants"));
            }
        }
        drop(completed_tx);
        let mut completed_count = 0;
        loop {
            crossbeam_channel::select! {
                recv(shutdown) -> _ => {
                    cancelled.store(true, Ordering::Relaxed);
                    for sender in &cancellation { let _ = sender.send(()); }
                }
                recv(completed_rx) -> completed => {
                    let Ok(record) = completed else { break };
                    completed_count += 1;
                    if !json {
                        writeln!(std::io::stderr(), "Completed mutation {completed_count}/{total}")?;
                    }
                    results.records.insert(record.mutant.id.clone(), record);
                    if let Err(error) = results.write(&path) {
                        cancelled.store(true, Ordering::Relaxed);
                        for sender in &cancellation { let _ = sender.send(()); }
                        return Err(error).context("cannot preserve completed mutation results");
                    }
                }
            }
        }
        Ok(cancelled.load(Ordering::Relaxed))
    })
}
