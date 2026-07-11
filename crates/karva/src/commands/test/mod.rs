mod watch;

use std::collections::HashMap;
use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use camino::Utf8PathBuf;
use karva_cache::{AggregatedResults, DisplayFlakyTests};
use karva_cli::TestCommand;
use karva_logging::{Printer, Stdout, set_colored_override, setup_tracing};
use karva_metadata::filter::FiltersetSet;
use karva_metadata::{CovReport, NoTestsMode, ProjectMetadata, ProjectOptionsOverrides};
use karva_project::Project;
use karva_project::path::absolute;
use karva_python_semantic::current_python_version;

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
    let watch = args.watch;
    let durations = args.durations;
    let last_failed = args.last_failed;
    let partition = args.partition;
    let no_cache = args.no_cache.unwrap_or(false);
    let num_workers = if args.no_parallel.unwrap_or(false) || args.no_capture {
        1
    } else if let Some(num_workers) = args.num_workers {
        num_workers
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

    let printer = Printer::new(
        project.settings().terminal().status_level,
        project.settings().terminal().final_status_level,
    );

    FiltersetSet::new(&sub_command.filter_expressions).context("invalid `--filter` expression")?;

    let config = karva_runner::ParallelTestConfig {
        num_workers,
        no_cache,
        create_ctrlc_handler: true,
        last_failed,
        profile,
        partition,
        test_ordering: karva_runner::TestOrdering::ShuffleUnknownDurations,
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

    if timed_out {
        print_run_timed_out(printer)?;
        return Ok(ExitStatus::Failure);
    }

    let coverage_total = if coverage_files.is_empty() {
        None
    } else {
        let coverage = project.settings().coverage();
        let coverage_filters =
            karva_coverage::CoverageFilters::new(&coverage.include, &coverage.omit)?;
        if coverage.report_path.is_some()
            && matches!(coverage.report, CovReport::Term | CovReport::TermMissing)
        {
            let mut stdout = printer.stream_for_message().lock();
            writeln!(
                stdout,
                "warning: `coverage.report-path` is ignored when `coverage.report` is `{}`",
                coverage.report.as_str()
            )?;
        }
        let coverage_data_path = project.cwd().join(".coverage");
        if let Err(err) = karva_coverage::write_coveragepy_sqlite(
            project.cwd(),
            &coverage_files,
            &coverage_data_path,
            &coverage_filters,
        ) {
            tracing::error!("Coverage data file failed: {err:#}");
        }
        let coverage_result = match coverage.report {
            CovReport::Term => karva_coverage::combine_and_report(
                project.cwd(),
                &coverage_files,
                false,
                &coverage_filters,
            ),
            CovReport::TermMissing => karva_coverage::combine_and_report(
                project.cwd(),
                &coverage_files,
                true,
                &coverage_filters,
            ),
            CovReport::Xml => {
                let output = coverage_report_path(
                    coverage.report_path.as_deref(),
                    "coverage.xml",
                    project.cwd(),
                );
                karva_coverage::write_cobertura_xml(
                    project.cwd(),
                    &coverage_files,
                    &output,
                    &coverage_filters,
                )
            }
            CovReport::Json => {
                let output = coverage_report_path(
                    coverage.report_path.as_deref(),
                    "coverage.json",
                    project.cwd(),
                );
                karva_coverage::write_json_report(
                    project.cwd(),
                    &coverage_files,
                    &output,
                    &coverage_filters,
                )
            }
            CovReport::Html => {
                let output =
                    coverage_report_path(coverage.report_path.as_deref(), "htmlcov", project.cwd());
                karva_coverage::write_html_report(
                    project.cwd(),
                    &coverage_files,
                    &output,
                    &coverage_filters,
                )
            }
        };
        match coverage_result {
            Ok(total) => total,
            Err(err) => {
                tracing::error!("Coverage report failed: {err:#}");
                None
            }
        }
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
        match project.settings().test().no_tests {
            NoTestsMode::Pass => return Ok(ExitStatus::Success),
            NoTestsMode::Auto if has_filters => return Ok(ExitStatus::Success),
            NoTestsMode::Warn => {
                let mut stdout = printer.stream_for_message().lock();
                writeln!(stdout, "warning: no tests to run")?;
                return Ok(ExitStatus::Success);
            }
            NoTestsMode::Auto | NoTestsMode::Fail => {
                let mut stdout = printer.stream_for_message().lock();
                writeln!(stdout, "error: no tests to run")?;
                writeln!(stdout, "(hint: use `--no-tests` to customize)")?;
                return Ok(ExitStatus::Failure);
            }
        }
    }

    if result.stats.is_success() && result.diagnostics.is_empty() && !coverage_below_threshold {
        Ok(ExitStatus::Success)
    } else {
        Ok(ExitStatus::Failure)
    }
}

fn coverage_report_path(
    configured: Option<&str>,
    default: &str,
    project_root: &camino::Utf8Path,
) -> Utf8PathBuf {
    absolute(configured.unwrap_or(default), project_root)
}

fn no_tests_collected(result: &AggregatedResults) -> bool {
    result.stats.total() == 0 && result.diagnostics.is_empty()
}

/// Print the message shown when a run is stopped by `--run-timeout`.
///
/// Shared by the one-shot and watch-mode paths.
pub(super) fn print_run_timed_out(printer: Printer) -> std::io::Result<()> {
    let mut stdout = printer.stream_for_message().lock();
    writeln!(stdout, "\nerror: run timed out before all tests completed")
}

/// Print test output: diagnostics, durations, and result summary.
pub fn print_test_output(
    printer: Printer,
    start_time: Instant,
    result: &AggregatedResults,
    durations: Option<usize>,
) -> Result<()> {
    let mut details = printer.stream_for_details().lock();

    let has_diagnostics = !result.diagnostics.is_empty();
    let has_captured_output = has_failed_captured_output(result);
    let has_preceding_test_lines = result.stats.total() > 0;

    write_diagnostics_block(&mut details, result, has_preceding_test_lines)?;
    write_captured_output_block(
        &mut details,
        result,
        has_preceding_test_lines && !has_diagnostics,
    )?;

    write_durations_block(
        &mut details,
        &result.durations,
        durations,
        has_preceding_test_lines && !has_diagnostics && !has_captured_output,
    )?;

    drop(details);

    let mut summary = printer
        .stream_for_summary(result.stats.is_success(), result.stats.flaky() > 0)
        .lock();

    write!(summary, "{}", result.stats.display(start_time))?;
    write!(summary, "{}", DisplayFlakyTests::new(&result.flaky_tests))?;

    Ok(())
}

fn has_failed_captured_output(result: &AggregatedResults) -> bool {
    result
        .captured_outputs
        .iter()
        .any(|output| output.outcome().is_failed() && !output.is_empty())
}

fn write_diagnostics_block(
    stdout: &mut Stdout,
    result: &AggregatedResults,
    needs_leading_blank: bool,
) -> Result<()> {
    if result.diagnostics.is_empty() {
        return Ok(());
    }

    if needs_leading_blank && stdout.is_enabled() {
        writeln!(stdout)?;
    }
    writeln!(stdout, "diagnostics:")?;
    writeln!(stdout)?;
    write!(stdout, "{}", result.diagnostics)?;

    Ok(())
}

fn write_captured_output_block(
    stdout: &mut Stdout,
    result: &AggregatedResults,
    needs_leading_blank: bool,
) -> Result<()> {
    let mut failed_outputs: Vec<_> = result
        .captured_outputs
        .iter()
        .filter(|output| output.outcome().is_failed() && !output.is_empty())
        .collect();
    if failed_outputs.is_empty() {
        return Ok(());
    }

    failed_outputs.sort_by_key(|output| output.test_name());

    if needs_leading_blank && stdout.is_enabled() {
        writeln!(stdout)?;
    }

    for output in failed_outputs {
        write_captured_stream(stdout, "stdout", output.test_name(), output.stdout())?;
        write_captured_stream(stdout, "stderr", output.test_name(), output.stderr())?;
    }
    writeln!(stdout)?;

    Ok(())
}

fn write_captured_stream(
    stdout: &mut Stdout,
    stream_name: &str,
    test_name: &str,
    content: &str,
) -> Result<()> {
    if content.is_empty() {
        return Ok(());
    }

    writeln!(stdout, "captured {stream_name} for {test_name}:")?;
    write!(stdout, "{content}")?;
    if !content.ends_with('\n') {
        writeln!(stdout)?;
    }

    Ok(())
}

fn write_durations_block(
    stdout: &mut Stdout,
    test_durations: &HashMap<String, Duration>,
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

    let mut sorted: Vec<_> = test_durations.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    let count = n.min(sorted.len());

    writeln!(stdout, "{count} slowest tests:")?;
    for (name, duration) in sorted.into_iter().take(n) {
        writeln!(
            stdout,
            "  {} ({})",
            name,
            karva_logging::time::format_duration(*duration)
        )?;
    }
    // Trailing blank so the summary divider doesn't bump up against the
    // last duration line.
    writeln!(stdout)?;
    Ok(())
}
