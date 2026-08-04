//! End-to-end orchestration of one `karva test` invocation.

mod junit;
mod result_report;
mod watch;

use std::collections::HashMap;
use std::io::ErrorKind;
use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use camino::Utf8PathBuf;
use karva_cache::{
    AggregatedResults, CACHE_DIR, DisplayFlakyTests, read_random_seed, write_random_seed,
};
use karva_cli::{CovReport as CliCovReport, RandomSeed, TestCommand};
use karva_logging::{Printer, Stdout, set_colored_override, setup_tracing};
use karva_metadata::filter::FiltersetSet;
use karva_metadata::{CovReport, NoTestsMode, ProjectMetadata, ProjectOptionsOverrides};
use karva_project::Project;
use karva_project::path::absolute;
use karva_python_semantic::{TestCacheKey, current_python_version};

use crate::ExitStatus;
use crate::utils::cwd;

pub fn test(args: TestCommand) -> Result<ExitStatus> {
    let verbosity = args.verbosity().level();

    set_colored_override(args.sub_command.color);

    let _guard = setup_tracing(verbosity);

    let cwd = cwd().map_err(|_| {
        anyhow::anyhow!(
            "The current working directory contains non-Unicode characters. karva only supports Unicode paths."
        )
    })?;

    tracing::debug!(cwd = %cwd, "Working directory");

    let python_version = current_python_version();

    let config_file = args.config_file.as_ref().map(|path| absolute(path, &cwd));

    let mut project_metadata = if let Some(config_file) = &config_file {
        ProjectMetadata::from_config_file(config_file, &cwd, python_version)?
    } else {
        ProjectMetadata::discover(&cwd, python_version)?
    };

    let sub_command = args.sub_command.clone();
    let explicit_cov_reports = sub_command.cov_report.clone();
    let no_cov_on_fail = sub_command.no_cov_on_fail.unwrap_or(false);
    let watch = args.watch;
    let durations = args.durations;
    let result_output = args.result_output.clone();
    let result_format = args.result_format.unwrap_or_default();
    let last_failed = args.last_failed;
    let partition = args.partition;
    let no_cache = args.no_cache.unwrap_or(false);
    let random_seed_selection = args.random_seed();
    let num_workers = if args.no_parallel.unwrap_or(false) || args.no_capture {
        1
    } else if let Some(num_workers) = args.num_workers {
        num_workers.get()
    } else {
        karva_static::max_parallelism()
            .context("Failed to determine default worker count")?
            .get()
    };

    let profile = args.profile.clone();
    let project_options_overrides = ProjectOptionsOverrides::new(config_file, args.into_options())
        .with_profile(profile.clone());
    project_metadata
        .apply_overrides(&project_options_overrides)
        .map_err(|err| anyhow::anyhow!("{err}"))?;

    let project = Project::from_metadata(project_metadata);

    let exclusion_patterns = project
        .settings()
        .coverage()
        .exclude_lines
        .iter()
        .map(|pattern| pattern.as_str().to_owned())
        .collect::<Vec<_>>();
    karva_coverage::executable::CoverageExclusions::new(&exclusion_patterns)?;

    let printer = Printer::new(
        project.settings().terminal().status_level,
        project.settings().terminal().final_status_level,
    );

    FiltersetSet::new(&sub_command.filter_expressions).context("invalid `--filter` expression")?;

    let cache_dir = project.cwd().join(CACHE_DIR);
    let (random_seed, generated_random_seed) = match random_seed_selection {
        Some(RandomSeed::Last) => {
            if no_cache {
                anyhow::bail!("`--random-seed=last` cannot be used with `--no-cache`");
            }
            let seed = read_random_seed(&cache_dir)?
                .context("No generated random seed found; run with `--shuffle` first")?;
            (Some(seed), false)
        }
        Some(RandomSeed::Value(seed)) => (Some(seed), false),
        None => match project.settings().test().random_seed {
            Some(seed) => (Some(seed), false),
            None if project.settings().test().shuffle => {
                (Some(karva_runner::generate_random_seed()), true)
            }
            None => (None, false),
        },
    };
    if generated_random_seed
        && !no_cache
        && let Some(seed) = random_seed
    {
        write_random_seed(&cache_dir, seed)?;
    }
    if let Some(seed) = random_seed {
        let mut stdout = printer.stream_for_message().lock();
        writeln!(stdout, "Random seed: {seed}")?;
    }

    let config = karva_runner::ParallelTestConfig {
        num_workers,
        no_cache,
        create_ctrlc_handler: true,
        last_failed,
        profile,
        partition,
        random_seed,
        test_ordering: random_seed
            .filter(|_| project.settings().test().shuffle)
            .map_or(
                karva_runner::TestOrdering::RandomizeUnmeasured,
                karva_runner::TestOrdering::SeededShuffle,
            ),
    };

    if watch {
        watch::run_watch_loop(&project, &config, &sub_command, printer, durations)?;
        return Ok(ExitStatus::Success);
    }

    let start_time = Instant::now();

    let karva_runner::RunOutput {
        results: result,
        coverage_files,
        timed_out,
    } = karva_runner::run_parallel_tests(&project, &config, &sub_command, printer)?;

    print_test_output(printer, start_time, &result, durations)?;
    junit::write_junit_report(project.settings().junit(), &result, project.cwd())?;
    let coverage_data_file = persist_coverage(&project, &coverage_files)?;
    let write_result_report = |exit_status| {
        result_report::write_result_report(
            result_output.as_deref(),
            result_format,
            &result,
            project.cwd(),
            start_time.elapsed(),
            exit_status,
            random_seed,
        )
    };

    if timed_out {
        print_run_timed_out(printer)?;
        let exit_status = ExitStatus::Failure;
        write_result_report(exit_status)?;
        return Ok(exit_status);
    }

    let coverage_total = if let Some(data_file) = coverage_data_file {
        let coverage = project.settings().coverage();
        let coverage_filters =
            karva_coverage::CoverageFilters::new(&coverage.include, &coverage.omit)?
                .with_contexts(&coverage.contexts)?
                .with_path_aliases(&coverage.path_aliases)?;
        if explicit_cov_reports.is_empty()
            && coverage.report_path.is_some()
            && matches!(coverage.report, CovReport::Term | CovReport::TermMissing)
        {
            let mut stdout = printer.stream_for_message().lock();
            writeln!(
                stdout,
                "warning: `coverage.report-path` is ignored when `coverage.report` is `{}`",
                coverage.report.as_str()
            )?;
        }
        let coverage_result = (|| -> Result<Option<f64>> {
            let Some(analysis) = karva_coverage::CoverageAnalysis::load_native(
                project.cwd(),
                std::slice::from_ref(&data_file),
                &coverage_filters,
            )?
            else {
                return Ok(None);
            };
            let reports = if explicit_cov_reports.is_empty() {
                vec![configured_coverage_report(
                    coverage.report,
                    coverage.report_path.as_deref(),
                )]
            } else {
                explicit_cov_reports.clone()
            };
            if !no_cov_on_fail || result.is_success() {
                for report in &reports {
                    if let Err(error) = render_coverage_report(
                        &analysis,
                        report,
                        coverage.precision.0,
                        project.cwd(),
                    ) {
                        tracing::error!(
                            "Coverage {} report failed: {error:#}",
                            coverage_report_name(report)
                        );
                    }
                }
            }
            Ok(Some(analysis.total_percent()))
        })();
        match coverage_result {
            Ok(total) => total,
            Err(err) => {
                tracing::error!("Coverage report failed: {err:#}");
                None
            }
        }
    } else {
        None
    };

    let coverage_below_threshold = if let Some(total) = coverage_total
        && let Some(threshold) = project.settings().coverage().fail_under
        && total < threshold
    {
        let mut stdout = printer.stream_for_message().lock();
        writeln!(
            stdout,
            "\ncoverage failure: required total coverage of {threshold}% not reached, total coverage was {total:.2}%",
        )?;
        true
    } else {
        false
    };

    if no_tests_collected(&result) {
        let has_filters = !sub_command.filter_expressions.is_empty();
        let exit_status = match project.settings().test().no_tests {
            NoTestsMode::Pass => ExitStatus::Success,
            NoTestsMode::Auto if has_filters => ExitStatus::Success,
            NoTestsMode::Warn => {
                let mut stdout = printer.stream_for_message().lock();
                writeln!(stdout, "warning: no tests to run")?;
                ExitStatus::Success
            }
            NoTestsMode::Auto | NoTestsMode::Fail => {
                let mut stdout = printer.stream_for_message().lock();
                writeln!(stdout, "error: no tests to run")?;
                writeln!(stdout, "(hint: use `--no-tests` to customize)")?;
                ExitStatus::Failure
            }
        };
        write_result_report(exit_status)?;
        return Ok(exit_status);
    }

    let flaky_failed = result.has_flaky_failures();
    let exit_status = if result.is_success() && !coverage_below_threshold && !flaky_failed {
        ExitStatus::Success
    } else {
        ExitStatus::Failure
    };
    write_result_report(exit_status)?;
    Ok(exit_status)
}

fn configured_coverage_report(report: CovReport, path: Option<&str>) -> CliCovReport {
    let path = path.map(Utf8PathBuf::from);
    match report {
        CovReport::None => CliCovReport::None,
        CovReport::Term => CliCovReport::Term,
        CovReport::TermMissing => CliCovReport::TermMissing,
        CovReport::Xml => CliCovReport::Xml { path },
        CovReport::Json => CliCovReport::Json { path },
        CovReport::Html => CliCovReport::Html { path },
        CovReport::Lcov => CliCovReport::Lcov { path },
    }
}

fn render_coverage_report(
    analysis: &karva_coverage::CoverageAnalysis,
    report: &CliCovReport,
    precision: usize,
    project_root: &camino::Utf8Path,
) -> Result<()> {
    match report {
        CliCovReport::None => {}
        CliCovReport::Term => {
            analysis.report_with_precision(false, precision)?;
        }
        CliCovReport::TermMissing => {
            analysis.report_with_precision(true, precision)?;
        }
        CliCovReport::Xml { path } => {
            let output = coverage_report_path(
                path.as_ref().map(|path| path.as_str()),
                "coverage.xml",
                project_root,
            );
            analysis.write_cobertura_xml(&output)?;
        }
        CliCovReport::Json { path } => {
            let output = coverage_report_path(
                path.as_ref().map(|path| path.as_str()),
                "coverage.json",
                project_root,
            );
            analysis.write_json(&output)?;
        }
        CliCovReport::Html { path } => {
            let output = coverage_report_path(
                path.as_ref().map(|path| path.as_str()),
                "htmlcov",
                project_root,
            );
            analysis.write_html(&output)?;
        }
        CliCovReport::Lcov { path } => {
            let output = coverage_report_path(
                path.as_ref().map(|path| path.as_str()),
                "coverage.lcov",
                project_root,
            );
            analysis.write_lcov(&output)?;
        }
    }
    Ok(())
}

fn coverage_report_name(report: &CliCovReport) -> &'static str {
    match report {
        CliCovReport::None => "none",
        CliCovReport::Term => "term",
        CliCovReport::TermMissing => "term-missing",
        CliCovReport::Xml { .. } => "xml",
        CliCovReport::Json { .. } => "json",
        CliCovReport::Html { .. } => "html",
        CliCovReport::Lcov { .. } => "lcov",
    }
}

fn persist_coverage(
    project: &Project,
    worker_files: &[Utf8PathBuf],
) -> Result<Option<Utf8PathBuf>> {
    if worker_files.is_empty() {
        return Ok(None);
    }

    let settings = project.settings().coverage();
    let mode = if settings.branch {
        karva_coverage::native::CoverageMode::Branch
    } else {
        karva_coverage::native::CoverageMode::Line
    };
    let current = karva_coverage::native::NativeCoverage::from_worker_files(
        project.cwd(),
        mode,
        worker_files,
    )?;
    let data_file = absolute(&settings.data_file, project.cwd());
    let artifact = if settings.append {
        match std::fs::metadata(&data_file) {
            Ok(_) => {
                let mut previous = karva_coverage::native::NativeCoverage::read(&data_file)?;
                previous.merge(current).with_context(|| {
                    format!("failed to append native coverage data `{data_file}`")
                })?;
                previous
            }
            Err(error) if error.kind() == ErrorKind::NotFound => current,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect coverage data `{data_file}`"));
            }
        }
    } else {
        current
    };
    artifact.write(&data_file)?;
    Ok(Some(data_file))
}

fn coverage_report_path(
    configured: Option<&str>,
    default: &str,
    project_root: &camino::Utf8Path,
) -> Utf8PathBuf {
    absolute(configured.unwrap_or(default), project_root)
}

fn no_tests_collected(result: &AggregatedResults) -> bool {
    result.stats.total() == 0 && !result.has_run_errors()
}

/// Print the message shown when a run is stopped by `--run-timeout`.
///
/// Shared by the one-shot and watch-mode paths.
fn print_run_timed_out(printer: Printer) -> std::io::Result<()> {
    let mut stdout = printer.stream_for_message().lock();
    writeln!(stdout, "\nerror: run timed out before all tests completed")
}

/// Print test output: failures, run diagnostics, durations, and result summary.
pub fn print_test_output(
    printer: Printer,
    start_time: Instant,
    result: &AggregatedResults,
    durations: Option<usize>,
) -> Result<()> {
    let mut details = printer.stream_for_details().lock();

    let has_test_diagnostics = !result.stats.is_success();
    let has_run_diagnostics = !result.run_diagnostics.is_empty();
    let has_diagnostics = has_test_diagnostics || has_run_diagnostics;
    let has_preceding_test_lines = result.stats.total() > 0;

    if has_test_diagnostics {
        write_test_failures_block(&mut details, result, has_preceding_test_lines)?;
    }
    write_run_diagnostics_block(
        &mut details,
        result,
        has_preceding_test_lines && !has_test_diagnostics,
    )?;

    write_durations_block(
        &mut details,
        &result.durations,
        durations,
        has_preceding_test_lines && !has_diagnostics,
    )?;

    drop(details);

    let flaky_failed = result.has_flaky_failures();
    let success = result.is_success() && !flaky_failed;
    let mut summary = printer
        .stream_for_summary(success, result.stats.flaky() > 0)
        .lock();

    write!(summary, "{}", result.stats.display(start_time, success))?;
    write!(summary, "{}", DisplayFlakyTests::new(&result.flaky_tests))?;
    if flaky_failed {
        writeln!(summary, "flaky failure: flaky tests caused the run to fail")?;
    }

    Ok(())
}

fn write_test_failures_block(
    stdout: &mut Stdout,
    result: &AggregatedResults,
    needs_leading_blank: bool,
) -> Result<()> {
    let failed_tests = result
        .test_cases
        .iter()
        .filter(|case| case.outcome().diagnostic().is_some())
        .collect::<Vec<_>>();
    if failed_tests.is_empty() {
        return Ok(());
    }

    if needs_leading_blank && stdout.is_enabled() {
        writeln!(stdout)?;
    }
    writeln!(stdout, "failures:")?;
    writeln!(stdout)?;
    let mut emitted = vec![false; failed_tests.len()];
    for (index, case) in failed_tests.iter().enumerate() {
        if emitted[index] {
            continue;
        }
        emitted[index] = true;
        let Some(diagnostic) = case.outcome().diagnostic() else {
            continue;
        };
        write_test_failure_header(stdout, case)?;
        if case.captured_output().is_none() {
            for (other_index, other) in failed_tests.iter().enumerate().skip(index + 1) {
                if other.captured_output().is_none()
                    && other.outcome().diagnostic() == Some(diagnostic)
                    && other.outcome().related_diagnostics() == case.outcome().related_diagnostics()
                    && other.random_seeds() == case.random_seeds()
                {
                    emitted[other_index] = true;
                    write_test_failure_header(stdout, other)?;
                }
            }
        }
        writeln!(stdout)?;
        write_rendered_diagnostic(stdout, diagnostic.rendered_for_terminal())?;
        for diagnostic in case.outcome().related_diagnostics() {
            write_rendered_diagnostic(stdout, diagnostic.rendered_for_terminal())?;
        }
        if let Some(seeds) = case.random_seeds() {
            writeln!(stdout, "Random seed: {}", seeds.base())?;
            writeln!(
                stdout,
                "Phase seeds: setup={}, call={}, teardown={}",
                seeds.setup(),
                seeds.call(),
                seeds.teardown()
            )?;
            writeln!(stdout)?;
        }
        if let Some(output) = case.captured_output() {
            write_captured_stream(stdout, "stdout", output.stdout())?;
            write_captured_stream(stdout, "stderr", output.stderr())?;
            writeln!(stdout)?;
        }
    }

    Ok(())
}

fn write_test_failure_header(
    stdout: &mut Stdout,
    case: &karva_diagnostic::TestCaseResult,
) -> Result<()> {
    write!(stdout, "{}", case.full_name())?;
    let fixture_failures = case.outcome().fixture_failures();
    if !fixture_failures.is_empty() {
        write!(stdout, " (")?;
        for (index, failure) in fixture_failures.iter().enumerate() {
            if index > 0 {
                write!(stdout, "; ")?;
            }
            write!(stdout, "{}", failure.description())?;
        }
        write!(stdout, ")")?;
    }
    writeln!(stdout, ":")?;
    Ok(())
}

fn write_run_diagnostics_block(
    stdout: &mut Stdout,
    result: &AggregatedResults,
    needs_leading_blank: bool,
) -> Result<()> {
    if result.run_diagnostics.is_empty() {
        return Ok(());
    }

    if needs_leading_blank && stdout.is_enabled() {
        writeln!(stdout)?;
    }
    writeln!(stdout, "diagnostics:")?;
    writeln!(stdout)?;
    for diagnostic in &result.run_diagnostics {
        write_rendered_diagnostic(stdout, diagnostic.rendered_for_terminal())?;
    }

    Ok(())
}

fn write_rendered_diagnostic(stdout: &mut Stdout, rendered: &str) -> Result<()> {
    write!(stdout, "{rendered}")?;
    if !rendered.ends_with('\n') {
        writeln!(stdout)?;
    }
    if !rendered.ends_with("\n\n") {
        writeln!(stdout)?;
    }
    Ok(())
}

fn write_captured_stream(stdout: &mut Stdout, stream_name: &str, content: &str) -> Result<()> {
    if content.is_empty() {
        return Ok(());
    }

    writeln!(stdout, "captured {stream_name}:")?;
    write!(stdout, "{content}")?;
    if !content.ends_with('\n') {
        writeln!(stdout)?;
    }

    Ok(())
}

fn write_durations_block(
    stdout: &mut Stdout,
    test_durations: &HashMap<TestCacheKey, Duration>,
    durations: Option<usize>,
    needs_leading_blank: bool,
) -> Result<()> {
    let Some(n) = durations else {
        return Ok(());
    };
    if n == 0 || test_durations.is_empty() {
        return Ok(());
    }

    if needs_leading_blank && stdout.is_enabled() {
        writeln!(stdout)?;
    }

    let sorted = sorted_test_durations(test_durations);
    let count = n.min(sorted.len());

    writeln!(stdout, "{count} slowest tests:")?;
    for (name, duration) in sorted.into_iter().take(n) {
        writeln!(
            stdout,
            "  {} ({})",
            name,
            karva_logging::time::format_duration(duration)
        )?;
    }
    // Trailing blank so the summary divider doesn't bump up against the
    // last duration line.
    writeln!(stdout)?;
    Ok(())
}

fn sorted_test_durations(
    test_durations: &HashMap<TestCacheKey, Duration>,
) -> Vec<(String, Duration)> {
    let mut function_durations = HashMap::new();
    for (name, duration) in test_durations {
        function_durations
            .entry(name.test_function_name())
            .and_modify(|total| *total += *duration)
            .or_insert(*duration);
    }

    let mut sorted: Vec<_> = function_durations
        .into_iter()
        .map(|(name, duration)| (name.to_string(), duration))
        .collect();
    sorted.sort_by(|(a_name, a_duration), (b_name, b_duration)| {
        b_duration.cmp(a_duration).then_with(|| a_name.cmp(b_name))
    });
    sorted
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use karva_python_semantic::{ModulePath, QualifiedFunctionName, TestCacheKey};

    use super::sorted_test_durations;

    #[test]
    fn sorted_test_durations_breaks_ties_by_name() {
        let durations = HashMap::from([
            (
                TestCacheKey::function_name("test_b"),
                Duration::from_millis(10),
            ),
            (
                TestCacheKey::function_name("test_slow"),
                Duration::from_millis(20),
            ),
            (
                TestCacheKey::function_name("test_a"),
                Duration::from_millis(10),
            ),
        ]);

        let names: Vec<_> = sorted_test_durations(&durations)
            .into_iter()
            .map(|(name, _)| name)
            .collect();

        assert_eq!(names, ["test_slow", "test_a", "test_b"]);
    }

    #[test]
    fn sorted_test_durations_aggregates_parameter_cases() {
        let function = QualifiedFunctionName::new(
            "test_example".to_string(),
            ModulePath::new_with_name("test.py", "tests.test".to_string()),
        );
        let durations = HashMap::from([
            (
                TestCacheKey::parameter_case(&function, 0),
                Duration::from_millis(10),
            ),
            (
                TestCacheKey::parameter_case(&function, 1),
                Duration::from_millis(20),
            ),
        ]);

        assert_eq!(
            sorted_test_durations(&durations),
            [(
                "tests.test::test_example".to_string(),
                Duration::from_millis(30)
            )]
        );
    }
}
