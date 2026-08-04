//! Compares pinned Karva workloads across wheels and emits mergeable CI reports.

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "benchmark CLI intentionally prints progress and merged results"
)]

use std::io::Write as _;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::LazyLock;
#[cfg(target_os = "linux")]
use std::time::Instant;
use std::{collections::HashSet, io};

use anyhow::{Context as _, Result};
use camino::{Utf8Path, Utf8PathBuf};
use clap::Parser;
use fs_err::{self as fs, File};
use karva_benchmark::{BENCHMARK_PROJECTS, BenchmarkProject};
use karva_cache::CACHE_DIR;
use karva_cli::ExitStatus;
use karva_static::ToolEnvVars;
use regex::Regex;
use serde::{Deserialize, Serialize};
use similar::{Algorithm, TextDiff};

const MATERIAL_CHANGE_PERCENT: f64 = 1.0;
const FAST_PROJECT_ITERATIONS: usize = 21;
const MEDIUM_PROJECT_ITERATIONS: usize = 15;
const LONG_PROJECT_ITERATIONS: usize = MEDIUM_PROJECT_ITERATIONS / 2;
const EXTRA_LONG_PROJECT_ITERATIONS: usize = LONG_PROJECT_ITERATIONS.div_ceil(2);

#[derive(Debug, Parser)]
#[command(about = "Run Karva benchmark comparisons")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, clap::Subcommand)]
enum Commands {
    Compare(CompareArgs),
    ListProjects,
    MergeReports(MergeReportsArgs),
}

#[derive(Debug, Parser)]
struct CompareArgs {
    #[arg(long)]
    baseline_label: String,

    #[arg(long, value_name = "PATH")]
    baseline_wheel: PathBuf,

    #[arg(long)]
    candidate_label: String,

    #[arg(long, value_name = "PATH")]
    candidate_wheel: PathBuf,

    #[arg(long, default_value_t = 3)]
    iterations: usize,

    #[arg(long = "project", value_name = "NAME")]
    projects: Vec<String>,

    #[arg(long, value_name = "PATH")]
    output_wall_time_json: PathBuf,

    #[arg(long, value_name = "PATH")]
    output_wall_time_markdown: PathBuf,

    #[arg(long, value_name = "PATH")]
    output_memory_json: PathBuf,

    #[arg(long, value_name = "PATH")]
    output_memory_markdown: PathBuf,
}

#[derive(Debug, Parser)]
struct MergeReportsArgs {
    #[arg(long, value_name = "PATH")]
    input_dir: PathBuf,

    #[arg(long, value_name = "PATH")]
    output_markdown: PathBuf,

    #[arg(long, value_name = "PATH")]
    output_html: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    output_diagnostics_markdown: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    output_diagnostics_summary_markdown: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    output_diagnostics_html: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
/// GitHub Actions matrix emitted for pinned benchmark workloads.
struct Matrix {
    include: Vec<MatrixProject>,
}

#[derive(Debug, Serialize)]
struct MatrixProject {
    project: &'static str,
    iterations: usize,
}

#[derive(Debug, Deserialize, Serialize)]
/// Mergeable comparison report for one performance metric.
struct ComparisonReport {
    metric: BenchmarkMetric,
    baseline_label: String,
    baseline_wheel: Utf8PathBuf,
    candidate_label: String,
    candidate_wheel: Utf8PathBuf,
    projects: Vec<ProjectComparison>,
}

#[derive(Debug, Deserialize, Serialize)]
/// Baseline and candidate measurements for one pinned workload.
struct ProjectComparison {
    name: String,
    iterations: usize,
    baseline: Measurement,
    candidate: Measurement,
    percent_change: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diagnostics: Option<DiagnosticComparison>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Normalized test-summary comparison retained beside performance results.
struct DiagnosticComparison {
    baseline: Option<TestStats>,
    candidate: Option<TestStats>,
    #[serde(default)]
    workload_changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    baseline_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    candidate_output: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct TestStats {
    passed: usize,
    failed: usize,
    errors: usize,
    skipped: usize,
}

#[derive(Debug, Deserialize, Serialize)]
/// Numeric samples and their median for one benchmark subject.
struct Measurement {
    /// Raw samples in metric-native units.
    values: Vec<f64>,

    /// Median in seconds for wall time or KiB for peak memory.
    median: f64,
}

#[derive(Debug, Default)]
struct ReportSummary {
    faster: usize,
    slower: usize,
    unchanged: usize,
    changed_workloads: usize,
}

struct Subject<'a> {
    label: &'a str,
    wheel: &'a Utf8Path,
    wall_time_values: &'a mut Vec<f64>,
    memory_values: &'a mut Vec<f64>,
    diagnostic_output: &'a mut Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum BenchmarkMetric {
    WallTime,
    Memory,
}

/// Executable, environment path, and arguments for one benchmarked Karva run.
struct KarvaInvocation {
    binary: Utf8PathBuf,
    path: std::ffi::OsString,
    args: Vec<String>,
}

/// Observations captured from one benchmark process invocation.
struct RunMeasurement {
    /// Elapsed wall-clock time in seconds.
    wall_time: f64,

    /// Peak resident set size in KiB.
    peak_rss_kib: f64,

    /// Complete process status and streams used for correctness checks.
    output: Output,
}

/// Warmed cache directories retained to give every benchmark subject the same partition weights.
struct DurationCacheSeed {
    cache_dir: Utf8PathBuf,
    directories: HashSet<Utf8PathBuf>,
}

/// Invocation and cache state shared by every subject for one benchmark project.
struct BenchmarkContext {
    invocation: KarvaInvocation,
    duration_cache_seed: DurationCacheSeed,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Compare(args) => compare(args),
        Commands::ListProjects => list_projects(),
        Commands::MergeReports(args) => merge_reports(args),
    }
}

fn list_projects() -> Result<()> {
    let matrix = Matrix {
        include: BENCHMARK_PROJECTS
            .iter()
            .map(|project| MatrixProject {
                project: project.name,
                iterations: matrix_iterations(project.name),
            })
            .collect(),
    };

    serde_json::to_writer(io::stdout(), &matrix).context("Failed to write benchmark matrix")?;
    println!();

    Ok(())
}

fn compare(args: CompareArgs) -> Result<()> {
    anyhow::ensure!(args.iterations > 0, "iterations must be greater than zero");

    let num_workers =
        karva_static::max_parallelism().context("Failed to determine benchmark worker count")?;
    let baseline_wheel = utf8_path(args.baseline_wheel)?;
    let candidate_wheel = utf8_path(args.candidate_wheel)?;
    let output_wall_time_json = utf8_path(args.output_wall_time_json)?;
    let output_wall_time_markdown = utf8_path(args.output_wall_time_markdown)?;
    let output_memory_json = utf8_path(args.output_memory_json)?;
    let output_memory_markdown = utf8_path(args.output_memory_markdown)?;
    let projects = selected_projects(&args.projects)?;
    let mut wall_time_comparisons = Vec::with_capacity(projects.len());
    let mut memory_comparisons = Vec::with_capacity(projects.len());

    for config in projects {
        eprintln!("Preparing benchmark project `{}`", config.name);
        let project = karva_benchmark::prepare_benchmark_project_environment(config)
            .with_context(|| format!("Failed to prepare benchmark project `{}`", config.name))?;
        let benchmark = prepare_benchmark(config, project.cwd(), &baseline_wheel, num_workers)?;
        let mut baseline_wall_time_values = Vec::with_capacity(args.iterations);
        let mut baseline_memory_values = Vec::with_capacity(args.iterations);
        let mut candidate_wall_time_values = Vec::with_capacity(args.iterations);
        let mut candidate_memory_values = Vec::with_capacity(args.iterations);
        let mut baseline_diagnostic_output = None;
        let mut candidate_diagnostic_output = None;

        for iteration in 0..args.iterations {
            let mut baseline = Subject {
                label: &args.baseline_label,
                wheel: &baseline_wheel,
                wall_time_values: &mut baseline_wall_time_values,
                memory_values: &mut baseline_memory_values,
                diagnostic_output: &mut baseline_diagnostic_output,
            };
            let mut candidate = Subject {
                label: &args.candidate_label,
                wheel: &candidate_wheel,
                wall_time_values: &mut candidate_wall_time_values,
                memory_values: &mut candidate_memory_values,
                diagnostic_output: &mut candidate_diagnostic_output,
            };

            if iteration % 2 == 0 {
                run_subject(config, project.cwd(), &benchmark, &mut baseline)?;
                run_subject(config, project.cwd(), &benchmark, &mut candidate)?;
            } else {
                run_subject(config, project.cwd(), &benchmark, &mut candidate)?;
                run_subject(config, project.cwd(), &benchmark, &mut baseline)?;
            }
        }

        let baseline_wall_time = Measurement::new(baseline_wall_time_values);
        let candidate_wall_time = Measurement::new(candidate_wall_time_values);
        let wall_time_percent_change =
            percent_change(baseline_wall_time.median, candidate_wall_time.median);
        let diagnostics = diagnostic_comparison(
            baseline_diagnostic_output.as_deref(),
            candidate_diagnostic_output.as_deref(),
            &args.baseline_label,
            &args.candidate_label,
        );
        let memory_diagnostics = diagnostics
            .as_ref()
            .map(|diagnostics| DiagnosticComparison {
                baseline: diagnostics.baseline.clone(),
                candidate: diagnostics.candidate.clone(),
                workload_changed: diagnostics.workload_changed,
                diff: None,
                baseline_output: None,
                candidate_output: None,
            });

        wall_time_comparisons.push(ProjectComparison {
            name: config.name.to_string(),
            iterations: args.iterations,
            baseline: baseline_wall_time,
            candidate: candidate_wall_time,
            percent_change: wall_time_percent_change,
            diagnostics,
        });

        let baseline_memory = Measurement::new(baseline_memory_values);
        let candidate_memory = Measurement::new(candidate_memory_values);
        let memory_percent_change = percent_change(baseline_memory.median, candidate_memory.median);

        memory_comparisons.push(ProjectComparison {
            name: config.name.to_string(),
            iterations: args.iterations,
            baseline: baseline_memory,
            candidate: candidate_memory,
            percent_change: memory_percent_change,
            diagnostics: memory_diagnostics,
        });
    }

    let wall_time_report = ComparisonReport {
        metric: BenchmarkMetric::WallTime,
        baseline_label: args.baseline_label.clone(),
        baseline_wheel: baseline_wheel.clone(),
        candidate_label: args.candidate_label.clone(),
        candidate_wheel: candidate_wheel.clone(),
        projects: wall_time_comparisons,
    };
    let memory_report = ComparisonReport {
        metric: BenchmarkMetric::Memory,
        baseline_label: args.baseline_label,
        baseline_wheel,
        candidate_label: args.candidate_label,
        candidate_wheel,
        projects: memory_comparisons,
    };

    write_json(&output_wall_time_json, &wall_time_report)?;
    write_markdown(&output_wall_time_markdown, &wall_time_report)?;
    write_json(&output_memory_json, &memory_report)?;
    write_markdown(&output_memory_markdown, &memory_report)?;

    Ok(())
}

fn merge_reports(args: MergeReportsArgs) -> Result<()> {
    let input_dir = utf8_path(args.input_dir)?;
    let output_markdown = utf8_path(args.output_markdown)?;
    let report = merge_report_files(&input_dir)?;

    write_markdown(&output_markdown, &report)?;
    if let Some(path) = args.output_html {
        write_html(
            &utf8_path(path)?,
            report.metric.report_title(),
            &markdown_report(&report).context("Failed to render HTML benchmark report")?,
        )?;
    }
    if let Some(path) = args.output_diagnostics_markdown {
        write_diagnostics_markdown(&utf8_path(path)?, &report)?;
    }
    if let Some(path) = args.output_diagnostics_summary_markdown {
        write_diagnostics_summary_markdown(&utf8_path(path)?, &report)?;
    }
    if let Some(path) = args.output_diagnostics_html {
        write_html(
            &utf8_path(path)?,
            "Diagnostic comparison",
            &diagnostics_markdown_report(&report, true)
                .context("Failed to render HTML diagnostics report")?,
        )?;
    }

    Ok(())
}

fn merge_report_files(input_dir: &Utf8Path) -> Result<ComparisonReport> {
    let mut reports = read_report_files(input_dir)?;
    let first = reports
        .pop()
        .ok_or_else(|| anyhow::anyhow!("No benchmark reports found in `{input_dir}`"))?;
    let mut merged = first;
    let mut seen = HashSet::new();

    for project in &merged.projects {
        anyhow::ensure!(
            seen.insert(project.name.clone()),
            "Duplicate benchmark report for `{}`",
            project.name
        );
    }

    for report in reports {
        anyhow::ensure!(
            report.baseline_label == merged.baseline_label,
            "Benchmark reports use different baseline labels"
        );
        anyhow::ensure!(
            report.candidate_label == merged.candidate_label,
            "Benchmark reports use different candidate labels"
        );
        anyhow::ensure!(
            report.metric == merged.metric,
            "Benchmark reports use different metrics"
        );
        for project in report.projects {
            anyhow::ensure!(
                seen.insert(project.name.clone()),
                "Duplicate benchmark report for `{}`",
                project.name
            );
            merged.projects.push(project);
        }
    }

    merged.projects.sort_by_key(|project| {
        BENCHMARK_PROJECTS
            .iter()
            .position(|config| config.name == project.name)
            .unwrap_or(BENCHMARK_PROJECTS.len())
    });

    Ok(merged)
}

fn read_report_files(input_dir: &Utf8Path) -> Result<Vec<ComparisonReport>> {
    let mut reports = Vec::new();
    for entry in fs::read_dir(input_dir)
        .with_context(|| format!("Failed to read benchmark report directory `{input_dir}`"))?
    {
        let entry =
            entry.with_context(|| format!("Failed to read entry in directory `{input_dir}`"))?;
        let path = Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| {
            anyhow::anyhow!(
                "Benchmark report path is not valid UTF-8: {}",
                path.display()
            )
        })?;
        if path
            .extension()
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("json"))
        {
            continue;
        }

        let file = File::open(&path)
            .with_context(|| format!("Failed to open benchmark report `{path}`"))?;
        let report = serde_json::from_reader(file)
            .with_context(|| format!("Failed to parse benchmark report `{path}`"))?;
        reports.push(report);
    }

    Ok(reports)
}

impl Measurement {
    fn new(values: Vec<f64>) -> Self {
        Self {
            median: median(&values),
            values,
        }
    }
}

fn selected_projects(names: &[String]) -> Result<Vec<&'static BenchmarkProject>> {
    if names.is_empty() {
        return Ok(BENCHMARK_PROJECTS.iter().collect());
    }

    let mut projects = Vec::with_capacity(names.len());
    for name in names {
        let Some(project) = karva_benchmark::find_benchmark_project(name) else {
            let available = BENCHMARK_PROJECTS
                .iter()
                .map(|project| project.name)
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!("Unknown benchmark project `{name}`. Available projects: {available}");
        };
        projects.push(project);
    }

    Ok(projects)
}

fn run_subject(
    config: &BenchmarkProject,
    project_root: &Utf8Path,
    benchmark: &BenchmarkContext,
    subject: &mut Subject<'_>,
) -> Result<()> {
    karva_benchmark::install_benchmark_tools(config, project_root, subject.wheel)
        .with_context(|| format!("Failed to install `{}` benchmark wheel", subject.label))?;
    benchmark.duration_cache_seed.restore()?;

    let measurement = run_project_cli(config, project_root, &benchmark.invocation)
        .with_context(|| format!("Failed to run `{}` with `{}`", config.name, subject.label))?;
    subject.wall_time_values.push(measurement.wall_time);
    subject.memory_values.push(measurement.peak_rss_kib);
    if subject.diagnostic_output.is_none() {
        *subject.diagnostic_output = Some(normalize_diagnostic_output(&measurement.output));
    }

    eprintln!(
        "{} / {} / {}: {} / {}: {}",
        config.name,
        subject.label,
        BenchmarkMetric::WallTime.mode_label(),
        BenchmarkMetric::WallTime.format_value(measurement.wall_time),
        BenchmarkMetric::Memory.mode_label(),
        BenchmarkMetric::Memory.format_value(measurement.peak_rss_kib),
    );

    Ok(())
}

fn prepare_benchmark(
    config: &BenchmarkProject,
    project_root: &Utf8Path,
    baseline_wheel: &Utf8Path,
    num_workers: NonZeroUsize,
) -> Result<BenchmarkContext> {
    let invocation = karva_invocation(config, project_root, num_workers)?;
    karva_benchmark::install_benchmark_tools(config, project_root, baseline_wheel)
        .context("Failed to install baseline benchmark wheel")?;
    karva_benchmark::clean_project_cache(project_root)
        .with_context(|| format!("Failed to clean benchmark cache for `{}`", config.name))?;
    warm_project_cache(config, project_root, &invocation)
        .with_context(|| format!("Failed to warm benchmark cache for `{}`", config.name))?;
    let duration_cache_seed = DurationCacheSeed::capture(project_root)?;
    Ok(BenchmarkContext {
        invocation,
        duration_cache_seed,
    })
}

impl DurationCacheSeed {
    fn capture(project_root: &Utf8Path) -> Result<Self> {
        let cache_dir = project_root.join(CACHE_DIR);
        let directories = cache_directories(&cache_dir)?;
        anyhow::ensure!(
            !directories.is_empty(),
            "Warming benchmark cache created no run directories in `{cache_dir}`"
        );
        Ok(Self {
            cache_dir,
            directories,
        })
    }

    fn restore(&self) -> Result<()> {
        for directory in cache_directories(&self.cache_dir)? {
            if !self.directories.contains(&directory) {
                fs::remove_dir_all(&directory).with_context(|| {
                    format!("Failed to remove benchmark measurement cache `{directory}`")
                })?;
            }
        }
        Ok(())
    }
}

fn cache_directories(cache_dir: &Utf8Path) -> Result<HashSet<Utf8PathBuf>> {
    let mut directories = HashSet::new();
    for entry in fs::read_dir(cache_dir)
        .with_context(|| format!("Failed to read benchmark cache `{cache_dir}`"))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let directory = Utf8PathBuf::try_from(entry.path())
            .context("Benchmark cache directory path is not valid UTF-8")?;
        directories.insert(directory);
    }
    Ok(directories)
}

fn run_project_cli(
    config: &BenchmarkProject,
    project_root: &Utf8Path,
    invocation: &KarvaInvocation,
) -> Result<RunMeasurement> {
    #[cfg(target_os = "linux")]
    {
        let report_path = memory_report_path(project_root, config.name);

        let start = Instant::now();
        let output = Command::new("/usr/bin/time")
            .current_dir(project_root)
            .env(ToolEnvVars::PATH, &invocation.path)
            .args(["--quiet", "-f", "%M", "-o", report_path.as_str()])
            .arg(invocation.binary.as_str())
            .args(&invocation.args)
            .output()
            .context("Failed to execute `/usr/bin/time` for memory benchmark")?;
        let elapsed = start.elapsed();

        ensure_karva_completed(&output, config)?;

        let peak_rss_kib = read_peak_rss_kib(&report_path)?;
        if let Err(err) = fs::remove_file(&report_path) {
            eprintln!("failed to remove memory benchmark report `{report_path}`: {err}");
        }

        Ok(RunMeasurement {
            wall_time: elapsed.as_secs_f64(),
            peak_rss_kib,
            output,
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = config;
        let _ = project_root;
        let _ = invocation;
        anyhow::bail!("Memory benchmarks require Linux and GNU `/usr/bin/time`")
    }
}

fn warm_project_cache(
    config: &BenchmarkProject,
    project_root: &Utf8Path,
    invocation: &KarvaInvocation,
) -> Result<()> {
    let output = run_invocation(invocation, project_root)?;
    ensure_karva_completed(&output, config)
}

fn run_invocation(invocation: &KarvaInvocation, project_root: &Utf8Path) -> Result<Output> {
    invocation.command(project_root).output().with_context(|| {
        format!(
            "Failed to execute `{}`",
            invocation.binary.as_std_path().display()
        )
    })
}

fn karva_invocation(
    config: &BenchmarkProject,
    project_root: &Utf8Path,
    num_workers: NonZeroUsize,
) -> Result<KarvaInvocation> {
    let bin_dir = venv_bin_dir(project_root);
    let binary = bin_dir.join(executable_name("karva"));
    let path = path_with_venv_first(&bin_dir)?;

    let mut args = vec![
        "test".to_string(),
        "--num-workers".to_string(),
        num_workers.to_string(),
        "--no-ignore".to_string(),
        "--output-format".to_string(),
        "concise".to_string(),
    ];

    if config.try_import_fixtures {
        args.push("--try-import-fixtures".to_string());
    }
    args.extend(config.paths.iter().map(ToString::to_string));

    Ok(KarvaInvocation { binary, path, args })
}

impl KarvaInvocation {
    fn command(&self, project_root: &Utf8Path) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .current_dir(project_root)
            .env(ToolEnvVars::PATH, &self.path)
            .args(&self.args);
        command
    }
}

fn ensure_karva_completed(output: &Output, config: &BenchmarkProject) -> Result<()> {
    anyhow::ensure!(
        is_benchmarkable_exit(output.status.code()),
        "Karva exited with status {} for `{}`\nstdout:\n{}\nstderr:\n{}",
        output.status,
        config.name,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    Ok(())
}

fn is_benchmarkable_exit(code: Option<i32>) -> bool {
    matches!(
        code,
        Some(code)
            if code == ExitStatus::Success.to_i32() || code == ExitStatus::Failure.to_i32()
    )
}

fn normalize_diagnostic_output(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        normalize_diagnostic_text(&String::from_utf8_lossy(&output.stdout)),
        normalize_diagnostic_text(&String::from_utf8_lossy(&output.stderr))
    )
}

fn normalize_diagnostic_text(text: &str) -> String {
    static DURATION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\[\s*\d+\.\d{3}s\]").expect("duration regex should be valid")
    });
    static ADDRESS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"0x[0-9a-fA-F]+").expect("address regex should be valid"));

    let text = DURATION.replace_all(text, "[TIME]");
    ADDRESS.replace_all(&text, "0xADDR").into_owned()
}

fn diagnostic_comparison(
    baseline: Option<&str>,
    candidate: Option<&str>,
    baseline_label: &str,
    candidate_label: &str,
) -> Option<DiagnosticComparison> {
    let (Some(baseline), Some(candidate)) = (baseline, candidate) else {
        return None;
    };

    let sorted_baseline = sort_diagnostic_lines(baseline);
    let sorted_candidate = sort_diagnostic_lines(candidate);
    let diff = (sorted_baseline != sorted_candidate).then(|| {
        TextDiff::configure()
            .algorithm(Algorithm::Patience)
            .diff_lines(&sorted_baseline, &sorted_candidate)
            .unified_diff()
            .context_radius(3)
            .header(baseline_label, candidate_label)
            .to_string()
    });
    let baseline_stats = parse_test_stats(baseline);
    let candidate_stats = parse_test_stats(candidate);
    let workload_changed =
        baseline_stats != candidate_stats || test_workload(baseline) != test_workload(candidate);

    Some(DiagnosticComparison {
        baseline: baseline_stats,
        candidate: candidate_stats,
        workload_changed,
        diff,
        baseline_output: Some(baseline.to_string()),
        candidate_output: Some(candidate.to_string()),
    })
}

/// Sorts nonblank concise output lines because parallel workers emit them in completion order.
fn sort_diagnostic_lines(output: &str) -> String {
    let mut lines = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    lines.sort_unstable();
    let mut sorted = lines.join("\n");
    sorted.push('\n');
    sorted
}

fn test_workload(output: &str) -> Vec<&str> {
    static TEST_RESULT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^\s+(?:PASS|FAIL|ERROR|SKIP|FLAKY \d+/\d+) \[TIME\] (?P<test>.+)$")
            .expect("test result regex should be valid")
    });

    let mut tests = TEST_RESULT
        .captures_iter(output)
        .filter_map(|captures| captures.name("test").map(|value| value.as_str()))
        .collect::<Vec<_>>();
    tests.sort_unstable();
    tests
}

fn parse_test_stats(output: &str) -> Option<TestStats> {
    static SUMMARY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?m)^\s*Summary \[TIME\] \d+ tests? run: (?P<passed>\d+) passed(?: \(\d+ flaky\))?(?:, (?P<failed>\d+) failed)?(?:, (?P<errors>\d+) errors?)?, (?P<skipped>\d+) skipped",
        )
        .expect("summary regex should be valid")
    });

    let captures = SUMMARY.captures(output)?;
    Some(TestStats {
        passed: captures.name("passed")?.as_str().parse().ok()?,
        failed: captures
            .name("failed")
            .map_or(Some(0), |value| value.as_str().parse().ok())?,
        errors: captures
            .name("errors")
            .map_or(Some(0), |value| value.as_str().parse().ok())?,
        skipped: captures.name("skipped")?.as_str().parse().ok()?,
    })
}

#[cfg(target_os = "linux")]
fn memory_report_path(project_root: &Utf8Path, project_name: &str) -> Utf8PathBuf {
    project_root.join(format!(
        ".karva-benchmark-memory-{project_name}-{}.txt",
        std::process::id()
    ))
}

#[cfg(target_os = "linux")]
fn read_peak_rss_kib(path: &Utf8Path) -> Result<f64> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read memory benchmark report `{path}`"))?;
    raw.trim()
        .parse::<f64>()
        .with_context(|| format!("Failed to parse peak RSS from `{path}`: {raw:?}"))
}

fn write_json(path: &Utf8Path, report: &ComparisonReport) -> Result<()> {
    create_parent_dir(path)?;
    let mut file = File::create(path).with_context(|| format!("Failed to create `{path}`"))?;
    serde_json::to_writer_pretty(&mut file, report)
        .with_context(|| format!("Failed to write `{path}`"))?;
    writeln!(file).with_context(|| format!("Failed to finish writing `{path}`"))?;
    Ok(())
}

fn write_markdown(path: &Utf8Path, report: &ComparisonReport) -> Result<()> {
    create_parent_dir(path)?;
    let body = markdown_report(report).context("Failed to render markdown benchmark report")?;
    fs::write(path, body).with_context(|| format!("Failed to write `{path}`"))
}

fn write_diagnostics_markdown(path: &Utf8Path, report: &ComparisonReport) -> Result<()> {
    create_parent_dir(path)?;
    let body = diagnostics_markdown_report(report, true)
        .context("Failed to render markdown diagnostics report")?;
    fs::write(path, body).with_context(|| format!("Failed to write `{path}`"))
}

fn write_diagnostics_summary_markdown(path: &Utf8Path, report: &ComparisonReport) -> Result<()> {
    create_parent_dir(path)?;
    let body = diagnostics_markdown_report(report, false)
        .context("Failed to render markdown diagnostics summary")?;
    fs::write(path, body).with_context(|| format!("Failed to write `{path}`"))
}

fn write_html(path: &Utf8Path, title: &str, report: &str) -> Result<()> {
    create_parent_dir(path)?;
    let body = standalone_html(title, report).context("Failed to render HTML report")?;
    fs::write(path, body).with_context(|| format!("Failed to write `{path}`"))
}

fn standalone_html(title: &str, report: &str) -> Result<String> {
    let mut options = markdown::Options::gfm();
    options.compile.allow_dangerous_html = true;
    let report = markdown::to_html_with_options(report, &options)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let title = escape_html(title);
    Ok(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>{title}</title><style>\
         :root{{color-scheme:light dark;font-family:ui-sans-serif,system-ui,sans-serif}}\
         body{{margin:0 auto;max-width:120rem;padding:2rem;line-height:1.45}}\
         table{{border-collapse:collapse;display:block;overflow:auto;width:max-content;max-width:100%}}\
         th,td{{border:1px solid color-mix(in srgb,currentColor 20%,transparent);padding:.35rem .6rem;text-align:left}}\
         th{{background:color-mix(in srgb,currentColor 8%,transparent)}}\
         code,pre{{font-family:ui-monospace,SFMono-Regular,Consolas,monospace}}\
         pre{{background:color-mix(in srgb,currentColor 6%,transparent);border:1px solid color-mix(in srgb,currentColor 18%,transparent);border-radius:.5rem;overflow:auto;padding:1rem}}\
         details{{margin-block:1rem}}summary{{cursor:pointer;font-weight:600}}\
         blockquote{{border-left:.25rem solid currentColor;margin-left:0;padding-left:1rem}}\
         </style></head><body><h1>{title}</h1><main>{report}</main></body></html>"
    ))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn create_parent_dir(path: &Utf8Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent directory `{parent}`"))?;
    }
    Ok(())
}

fn markdown_report(report: &ComparisonReport) -> std::result::Result<String, std::fmt::Error> {
    use std::fmt::Write as _;

    let summary = ReportSummary::new(&report.projects);
    let mut body = String::from(report.metric.marker());
    body.push('\n');
    let (marker, verdict) = verdict(report.metric, &summary);
    writeln!(body, "### {marker} {verdict}")?;

    if summary.slower > 0 || summary.changed_workloads > 0 {
        writeln!(body)?;
        writeln!(
            body,
            "Baseline: `{}`. Candidate: `{}`. {}",
            report.baseline_label,
            report.candidate_label,
            report.metric.report_context()
        )?;
        writeln!(body)?;
        write_summary_line(&mut body, ":zap:", summary.faster, "improved benchmark")?;
        write_summary_line(&mut body, ":x:", summary.slower, "regressed benchmark")?;
        write_summary_line(
            &mut body,
            ":white_check_mark:",
            summary.unchanged,
            "unchanged benchmark",
        )?;
        if summary.changed_workloads > 0 {
            write_summary_line(
                &mut body,
                ":warning:",
                summary.changed_workloads,
                "changed workload",
            )?;
        }
        if summary.slower > 0 {
            writeln!(body)?;
            writeln!(body, "> [!WARNING]")?;
            writeln!(
                body,
                "> Benchmark regressions were detected. Review the {} changes before merging.",
                report.metric.warning_label()
            )?;
            writeln!(body)?;

            body.push_str("#### Performance Changes\n\n");
            write_project_table(
                &mut body,
                report.metric,
                report.projects.iter().filter(|project| {
                    !project.workload_changed() && is_material_change(project.percent_change)
                }),
            )?;
        }
        if summary.changed_workloads > 0 {
            writeln!(body)?;
            writeln!(body, "> [!NOTE]")?;
            writeln!(
                body,
                "> Changed workloads execute different test sets. Their raw totals are shown but are not classified as performance changes."
            )?;
            writeln!(body)?;
            body.push_str("#### Workload Changes\n\n");
            write_project_table(
                &mut body,
                report.metric,
                report
                    .projects
                    .iter()
                    .filter(|project| project.workload_changed()),
            )?;
        }
    }

    writeln!(body)?;
    writeln!(body, "<details>")?;
    writeln!(body, "<summary>All benchmark scores</summary>")?;
    writeln!(body)?;
    write_project_table(&mut body, report.metric, &report.projects)?;
    writeln!(body)?;
    writeln!(body, "</details>")?;

    Ok(body)
}

fn diagnostics_markdown_report(
    report: &ComparisonReport,
    include_full_output: bool,
) -> std::result::Result<String, std::fmt::Error> {
    use std::fmt::Write as _;

    let changed = report
        .projects
        .iter()
        .filter(|project| {
            project
                .diagnostics
                .as_ref()
                .is_some_and(|diagnostics| diagnostics.diff.is_some())
        })
        .count();
    let mut body = String::from("<!-- karva-diagnostic-comparison -->\n");
    if changed == 0 {
        body.push_str("### :white_check_mark: No diagnostic changes\n");
    } else {
        let suffix = if changed == 1 { "" } else { "s" };
        writeln!(
            body,
            "### :warning: Diagnostic changes in {changed} project{suffix}"
        )?;
    }

    body.push_str("\n<details>\n<summary>Test result comparison</summary>\n\n");
    body.push_str("| Project | Previous | New |\n");
    body.push_str("| --- | --- | --- |\n");
    for project in &report.projects {
        let diagnostics = project.diagnostics.as_ref();
        let baseline = diagnostics.and_then(|diagnostics| diagnostics.baseline.as_ref());
        let candidate = diagnostics.and_then(|diagnostics| diagnostics.candidate.as_ref());
        writeln!(
            body,
            "| `{}` | {} | {} |",
            project.name,
            test_result(baseline),
            test_result(candidate),
        )?;
    }
    body.push_str("\n</details>\n");

    for project in &report.projects {
        let Some(diagnostics) = &project.diagnostics else {
            continue;
        };
        let Some(diff) = diagnostics.diff.as_deref() else {
            if include_full_output {
                write_full_diagnostic_output(
                    &mut body,
                    project,
                    diagnostics,
                    &report.baseline_label,
                    &report.candidate_label,
                )?;
            }
            continue;
        };
        writeln!(
            body,
            "\n<details>\n<summary><code>{}</code> concise output diff</summary>\n",
            project.name
        )?;
        body.push_str("Durations, memory addresses, and line order are normalized.\n\n");
        write_code_block(&mut body, "diff", diff)?;
        writeln!(body)?;
        body.push_str("</details>\n");

        if include_full_output {
            write_full_diagnostic_output(
                &mut body,
                project,
                diagnostics,
                &report.baseline_label,
                &report.candidate_label,
            )?;
        }
    }

    Ok(body)
}

fn write_full_diagnostic_output(
    body: &mut String,
    project: &ProjectComparison,
    diagnostics: &DiagnosticComparison,
    baseline_label: &str,
    candidate_label: &str,
) -> std::result::Result<(), std::fmt::Error> {
    use std::fmt::Write as _;

    writeln!(
        body,
        "\n<details>\n<summary><code>{}</code> full diagnostic output</summary>\n",
        project.name
    )?;
    body.push_str("Durations and memory addresses are normalized.\n\n");
    writeln!(body, "#### Baseline: `{baseline_label}`\n")?;
    write_optional_code_block(body, diagnostics.baseline_output.as_deref())?;
    writeln!(body, "\n#### Candidate: `{candidate_label}`\n")?;
    write_optional_code_block(body, diagnostics.candidate_output.as_deref())?;
    body.push_str("\n</details>\n");
    Ok(())
}

fn write_optional_code_block(
    body: &mut String,
    output: Option<&str>,
) -> std::result::Result<(), std::fmt::Error> {
    if let Some(output) = output {
        write_code_block(body, "text", output)
    } else {
        body.push_str("Diagnostic output unavailable.\n");
        Ok(())
    }
}

fn write_code_block(
    body: &mut String,
    language: &str,
    contents: &str,
) -> std::result::Result<(), std::fmt::Error> {
    use std::fmt::Write as _;

    let (longest, trailing) = contents.bytes().fold((0_usize, 0_usize), |state, byte| {
        if byte == b'`' {
            (state.0, state.1 + 1)
        } else {
            (state.0.max(state.1), 0)
        }
    });
    let fence = "`".repeat(longest.max(trailing).saturating_add(1).max(3));
    writeln!(body, "{fence}{language}")?;
    body.push_str(contents);
    if !contents.ends_with('\n') {
        body.push('\n');
    }
    writeln!(body, "{fence}")
}

fn test_result(stats: Option<&TestStats>) -> String {
    stats.map_or_else(
        || "—".to_string(),
        |stats| {
            let icon = if stats.failed == 0 && stats.errors == 0 {
                ":white_check_mark:"
            } else {
                ":x:"
            };
            format!(
                "{icon} {} pass · {} fail · {} error · {} skip",
                stats.passed, stats.failed, stats.errors, stats.skipped
            )
        },
    )
}

fn write_project_table<'a>(
    body: &mut String,
    metric: BenchmarkMetric,
    projects: impl IntoIterator<Item = &'a ProjectComparison>,
) -> std::result::Result<(), std::fmt::Error> {
    use std::fmt::Write as _;

    body.push_str("|  | Mode | Benchmark | Base | Head | Change | Runs |\n");
    body.push_str("| --- | --- | --- | ---: | ---: | ---: | ---: |\n");

    for project in projects {
        writeln!(
            body,
            "| {} | {} | `{}` | {} | {} | {} | {} |",
            project.trend_marker(),
            metric.mode_label(),
            project.name,
            metric.format_value(project.baseline.median),
            metric.format_value(project.candidate.median),
            format_percent(project.percent_change),
            project.iterations,
        )?;
    }

    Ok(())
}

impl ReportSummary {
    fn new(projects: &[ProjectComparison]) -> Self {
        let mut summary = Self::default();

        for project in projects {
            if project.workload_changed() {
                summary.changed_workloads += 1;
                continue;
            }
            match trend(project.percent_change) {
                "faster" => summary.faster += 1,
                "slower" => summary.slower += 1,
                _ => summary.unchanged += 1,
            }
        }

        summary
    }
}

fn verdict(metric: BenchmarkMetric, summary: &ReportSummary) -> (&'static str, &'static str) {
    if summary.slower > 0 {
        (":x:", metric.regression_verdict())
    } else if summary.changed_workloads > 0 {
        (":warning:", "Benchmark includes changed test workloads")
    } else if summary.faster > 0 {
        (":zap:", metric.improvement_verdict())
    } else {
        (":white_check_mark:", metric.unchanged_verdict())
    }
}

fn write_summary_line(
    body: &mut String,
    marker: &str,
    count: usize,
    singular_label: &str,
) -> std::result::Result<(), std::fmt::Error> {
    use std::fmt::Write as _;

    let suffix = if count == 1 { "" } else { "s" };
    writeln!(body, "{marker} **{count}** {singular_label}{suffix}")
}

fn matrix_iterations(project_name: &str) -> usize {
    match project_name {
        "requests" => EXTRA_LONG_PROJECT_ITERATIONS,
        "fastapi" | "httpx" | "werkzeug" => LONG_PROJECT_ITERATIONS,
        "tomlkit" => MEDIUM_PROJECT_ITERATIONS,
        _ => FAST_PROJECT_ITERATIONS,
    }
}

fn trend(percent_change: f64) -> &'static str {
    if percent_change <= -MATERIAL_CHANGE_PERCENT {
        "faster"
    } else if percent_change >= MATERIAL_CHANGE_PERCENT {
        "slower"
    } else {
        "flat"
    }
}

fn is_material_change(percent_change: f64) -> bool {
    percent_change.abs() >= MATERIAL_CHANGE_PERCENT
}

impl ProjectComparison {
    fn workload_changed(&self) -> bool {
        let Some(diagnostics) = &self.diagnostics else {
            return false;
        };
        diagnostics.workload_changed || diagnostics.baseline != diagnostics.candidate
    }

    fn trend_marker(&self) -> &'static str {
        if self.workload_changed() {
            ":warning:"
        } else {
            match trend(self.percent_change) {
                "faster" => ":zap:",
                "slower" => ":x:",
                _ => ":white_check_mark:",
            }
        }
    }
}

impl BenchmarkMetric {
    fn report_title(self) -> &'static str {
        match self {
            Self::WallTime => "Wall-time benchmark comparison",
            Self::Memory => "Memory benchmark comparison",
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::WallTime => "<!-- karva-benchmark-comparison -->",
            Self::Memory => "<!-- karva-memory-benchmark-comparison -->",
        }
    }

    fn mode_label(self) -> &'static str {
        match self {
            Self::WallTime => "WallTime",
            Self::Memory => "Memory",
        }
    }

    fn warning_label(self) -> &'static str {
        match self {
            Self::WallTime => "wall-time",
            Self::Memory => "peak-memory",
        }
    }

    fn report_context(self) -> &'static str {
        match self {
            Self::WallTime => {
                "Each benchmark compares median CLI wall time from optimized wheels on one GitHub Actions runner, alternating install order. Runs use available CPU parallelism, share one baseline-warmed duration cache so both revisions use the same test partition, and include default per-test status output. Raw totals are directly comparable only when both revisions execute the same tests. Lower is better."
            }
            Self::Memory => {
                "Each benchmark compares median peak RSS from optimized wheels on one GitHub Actions runner, alternating install order. Runs use available CPU parallelism, share one baseline-warmed duration cache so both revisions use the same test partition, and are configured per project. Raw totals are directly comparable only when both revisions execute the same tests. Lower is better."
            }
        }
    }

    fn regression_verdict(self) -> &'static str {
        match self {
            Self::WallTime => "Merging this PR may alter performance",
            Self::Memory => "Merging this PR may increase memory usage",
        }
    }

    fn improvement_verdict(self) -> &'static str {
        match self {
            Self::WallTime => "Merging this PR improves performance",
            Self::Memory => "Merging this PR reduces memory usage",
        }
    }

    fn unchanged_verdict(self) -> &'static str {
        match self {
            Self::WallTime => "Merging this PR will not alter performance",
            Self::Memory => "Merging this PR will not alter memory usage",
        }
    }

    fn format_value(self, value: f64) -> String {
        match self {
            Self::WallTime => format_seconds(value),
            Self::Memory => format_peak_rss_kib(value),
        }
    }
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let midpoint = sorted.len() / 2;

    if sorted.len().is_multiple_of(2) {
        f64::midpoint(sorted[midpoint - 1], sorted[midpoint])
    } else {
        sorted[midpoint]
    }
}

fn percent_change(baseline: f64, candidate: f64) -> f64 {
    ((candidate - baseline) / baseline) * 100.0
}

fn format_seconds(seconds: f64) -> String {
    if seconds < 1.0 {
        format!("{:.1} ms", seconds * 1000.0)
    } else {
        format!("{seconds:.3} s")
    }
}

fn format_peak_rss_kib(peak_rss_kib: f64) -> String {
    if peak_rss_kib < 1024.0 {
        format!("{peak_rss_kib:.0} KiB")
    } else {
        format!("{:.1} MiB", peak_rss_kib / 1024.0)
    }
}

fn format_percent(percent: f64) -> String {
    if percent.is_sign_positive() {
        format!("+{percent:.1}%")
    } else {
        format!("{percent:.1}%")
    }
}

fn venv_bin_dir(project_root: &Utf8Path) -> Utf8PathBuf {
    if cfg!(target_os = "windows") {
        project_root.join(".venv").join("Scripts")
    } else {
        project_root.join(".venv").join("bin")
    }
}

fn executable_name(name: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn path_with_venv_first(bin_dir: &Utf8Path) -> Result<std::ffi::OsString> {
    let mut paths = vec![PathBuf::from(bin_dir.as_str())];
    if let Some(existing_path) = std::env::var_os(ToolEnvVars::PATH) {
        paths.extend(std::env::split_paths(&existing_path));
    }
    std::env::join_paths(paths).context("Failed to construct PATH for benchmark command")
}

fn utf8_path(path: PathBuf) -> Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("Path is not valid UTF-8: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use camino::Utf8Path;
    use fs_err as fs;
    use tempfile::tempdir;

    use super::{
        BenchmarkMetric, CACHE_DIR, ComparisonReport, DiagnosticComparison, DurationCacheSeed,
        EXTRA_LONG_PROJECT_ITERATIONS, FAST_PROJECT_ITERATIONS, LONG_PROJECT_ITERATIONS,
        MEDIUM_PROJECT_ITERATIONS, Measurement, ProjectComparison, TestStats,
        diagnostic_comparison, diagnostics_markdown_report, is_benchmarkable_exit,
        karva_invocation, markdown_report, matrix_iterations, normalize_diagnostic_text,
        standalone_html, trend, write_code_block,
    };

    #[test]
    fn duration_cache_seed_removes_measurement_runs() {
        let temp_dir = tempdir().expect("temporary directory should be created");
        let project_root = Utf8Path::from_path(temp_dir.path())
            .expect("temporary directory path should be valid UTF-8");
        let cache_dir = project_root.join(CACHE_DIR);
        let seed_run = cache_dir.join("run-seed");
        let measurement_run = cache_dir.join("run-measurement");
        fs::create_dir_all(&seed_run).expect("seed cache should be created");
        let seed = DurationCacheSeed::capture(project_root).expect("seed should be captured");
        fs::create_dir_all(&measurement_run).expect("measurement cache should be created");

        seed.restore().expect("seed should be restored");

        assert!(seed_run.is_dir());
        assert!(!measurement_run.exists());
    }

    #[test]
    fn standalone_html_is_complete_and_escapes_report_content() {
        let html = standalone_html(
            "Report <title>",
            "## Results\n\n| Name |\n| --- |\n| value |\n\ndiff & <script>alert(1)</script>",
        )
        .expect("report should render");

        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("Report &lt;title&gt;"));
        assert!(html.contains("<h2>Results</h2>"));
        assert!(html.contains("<table>"));
        assert!(html.contains("diff &amp; &lt;script>"));
        assert!(!html.contains("<script>"));
        assert!(html.ends_with("</body></html>"));
    }

    #[test]
    fn markdown_report_renders_regressions() {
        let report = report_with_projects(vec![
            project("flat-project", 21, 1.0, 1.004),
            project("faster-project", 21, 1.0, 0.99),
            project("slower-project", 15, 1.0, 1.012),
        ]);

        let markdown = markdown_report(&report).expect("report should render");

        insta::assert_snapshot!(markdown, @"
        <!-- karva-benchmark-comparison -->
        ### :x: Merging this PR may alter performance

        Baseline: `main`. Candidate: `PR`. Each benchmark compares median CLI wall time from optimized wheels on one GitHub Actions runner, alternating install order. Runs use available CPU parallelism, share one baseline-warmed duration cache so both revisions use the same test partition, and include default per-test status output. Raw totals are directly comparable only when both revisions execute the same tests. Lower is better.

        :zap: **1** improved benchmark
        :x: **1** regressed benchmark
        :white_check_mark: **1** unchanged benchmark

        > [!WARNING]
        > Benchmark regressions were detected. Review the wall-time changes before merging.

        #### Performance Changes

        |  | Mode | Benchmark | Base | Head | Change | Runs |
        | --- | --- | --- | ---: | ---: | ---: | ---: |
        | :zap: | WallTime | `faster-project` | 1.000 s | 990.0 ms | -1.0% | 21 |
        | :x: | WallTime | `slower-project` | 1.000 s | 1.012 s | +1.2% | 15 |

        <details>
        <summary>All benchmark scores</summary>

        |  | Mode | Benchmark | Base | Head | Change | Runs |
        | --- | --- | --- | ---: | ---: | ---: | ---: |
        | :white_check_mark: | WallTime | `flat-project` | 1.000 s | 1.004 s | +0.4% | 21 |
        | :zap: | WallTime | `faster-project` | 1.000 s | 990.0 ms | -1.0% | 21 |
        | :x: | WallTime | `slower-project` | 1.000 s | 1.012 s | +1.2% | 15 |

        </details>
        ");
    }

    #[test]
    fn markdown_report_renders_unchanged_results() {
        let report = report_with_projects(vec![project("flat-project", 21, 1.0, 1.004)]);

        let markdown = markdown_report(&report).expect("report should render");

        insta::assert_snapshot!(markdown, @"
        <!-- karva-benchmark-comparison -->
        ### :white_check_mark: Merging this PR will not alter performance

        <details>
        <summary>All benchmark scores</summary>

        |  | Mode | Benchmark | Base | Head | Change | Runs |
        | --- | --- | --- | ---: | ---: | ---: | ---: |
        | :white_check_mark: | WallTime | `flat-project` | 1.000 s | 1.004 s | +0.4% | 21 |

        </details>
        ");
    }

    #[test]
    fn markdown_report_does_not_classify_changed_workload_as_regression() {
        let mut comparison = project("expanded-project", 7, 1.0, 1.8);
        comparison.diagnostics = Some(DiagnosticComparison {
            baseline: Some(TestStats {
                passed: 10,
                failed: 0,
                errors: 5,
                skipped: 0,
            }),
            candidate: Some(TestStats {
                passed: 20,
                failed: 0,
                errors: 0,
                skipped: 0,
            }),
            workload_changed: true,
            diff: None,
            baseline_output: None,
            candidate_output: None,
        });

        let markdown =
            markdown_report(&report_with_projects(vec![comparison])).expect("report should render");

        insta::assert_snapshot!(markdown, @"
        <!-- karva-benchmark-comparison -->
        ### :warning: Benchmark includes changed test workloads

        Baseline: `main`. Candidate: `PR`. Each benchmark compares median CLI wall time from optimized wheels on one GitHub Actions runner, alternating install order. Runs use available CPU parallelism, share one baseline-warmed duration cache so both revisions use the same test partition, and include default per-test status output. Raw totals are directly comparable only when both revisions execute the same tests. Lower is better.

        :zap: **0** improved benchmarks
        :x: **0** regressed benchmarks
        :white_check_mark: **0** unchanged benchmarks
        :warning: **1** changed workload

        > [!NOTE]
        > Changed workloads execute different test sets. Their raw totals are shown but are not classified as performance changes.

        #### Workload Changes

        |  | Mode | Benchmark | Base | Head | Change | Runs |
        | --- | --- | --- | ---: | ---: | ---: | ---: |
        | :warning: | WallTime | `expanded-project` | 1.000 s | 1.800 s | +80.0% | 7 |

        <details>
        <summary>All benchmark scores</summary>

        |  | Mode | Benchmark | Base | Head | Change | Runs |
        | --- | --- | --- | ---: | ---: | ---: | ---: |
        | :warning: | WallTime | `expanded-project` | 1.000 s | 1.800 s | +80.0% | 7 |

        </details>
        ");
    }

    #[test]
    fn markdown_report_renders_memory_improvements() {
        let report = report_with_metric(
            BenchmarkMetric::Memory,
            vec![project("memory-project", 21, 100_000.0, 90_000.0)],
        );

        let markdown = markdown_report(&report).expect("report should render");

        insta::assert_snapshot!(markdown, @"
        <!-- karva-memory-benchmark-comparison -->
        ### :zap: Merging this PR reduces memory usage

        <details>
        <summary>All benchmark scores</summary>

        |  | Mode | Benchmark | Base | Head | Change | Runs |
        | --- | --- | --- | ---: | ---: | ---: | ---: |
        | :zap: | Memory | `memory-project` | 97.7 MiB | 87.9 MiB | -10.0% | 21 |

        </details>
        ");
    }

    #[test]
    fn diagnostics_report_renders_changes() {
        let baseline = normalize_diagnostic_text(
            "    PASS [   0.001s] test_hooks(hook=<function hook at 0x123abc>)\n\
             Summary [   0.100s] 3 tests run: 2 passed, 1 skipped\n",
        );
        let candidate = normalize_diagnostic_text(
            "    PASS [   0.900s] test_hooks(hook=<function hook at 0x456def>)\n\
             new diagnostic\n\
             Summary [   1.000s] 3 tests run: 1 passed, 1 error, 1 skipped\n",
        );
        insta::assert_snapshot!(baseline, @"
            PASS [TIME] test_hooks(hook=<function hook at 0xADDR>)
        Summary [TIME] 3 tests run: 2 passed, 1 skipped
        ");
        let mut comparison = project("requests", 21, 1.0, 1.0);
        comparison.diagnostics =
            diagnostic_comparison(Some(&baseline), Some(&candidate), "main", "PR");

        let report = report_with_projects(vec![comparison]);
        let markdown = diagnostics_markdown_report(&report, true).expect("report should render");
        let summary = diagnostics_markdown_report(&report, false).expect("summary should render");

        assert!(summary.contains("concise output diff"));
        assert!(!summary.contains("full diagnostic output"));

        insta::assert_snapshot!(markdown, @"
        <!-- karva-diagnostic-comparison -->
        ### :warning: Diagnostic changes in 1 project

        <details>
        <summary>Test result comparison</summary>

        | Project | Previous | New |
        | --- | --- | --- |
        | `requests` | :white_check_mark: 2 pass · 0 fail · 0 error · 1 skip | :x: 1 pass · 0 fail · 1 error · 1 skip |

        </details>

        <details>
        <summary><code>requests</code> concise output diff</summary>

        Durations, memory addresses, and line order are normalized.

        ```diff
        --- main
        +++ PR
        @@ -1,2 +1,3 @@
             PASS [TIME] test_hooks(hook=<function hook at 0xADDR>)
        -Summary [TIME] 3 tests run: 2 passed, 1 skipped
        +Summary [TIME] 3 tests run: 1 passed, 1 error, 1 skipped
        +new diagnostic
        ```

        </details>

        <details>
        <summary><code>requests</code> full diagnostic output</summary>

        Durations and memory addresses are normalized.

        #### Baseline: `main`

        ```text
            PASS [TIME] test_hooks(hook=<function hook at 0xADDR>)
        Summary [TIME] 3 tests run: 2 passed, 1 skipped
        ```

        #### Candidate: `PR`

        ```text
            PASS [TIME] test_hooks(hook=<function hook at 0xADDR>)
        new diagnostic
        Summary [TIME] 3 tests run: 1 passed, 1 error, 1 skipped
        ```

        </details>
        ");
    }

    #[test]
    fn diagnostic_comparison_detects_equal_sized_workload_changes() {
        let baseline =
            "    PASS [TIME] test_first\nSummary [TIME] 1 test run: 1 passed, 0 skipped\n";
        let candidate =
            "    PASS [TIME] test_second\nSummary [TIME] 1 test run: 1 passed, 0 skipped\n";

        let comparison = diagnostic_comparison(Some(baseline), Some(candidate), "main", "PR")
            .expect("diagnostics should be available");

        assert_eq!(comparison.baseline, comparison.candidate);
        assert!(comparison.workload_changed);
    }

    #[test]
    fn diagnostic_comparison_detects_equal_sized_skipped_workload_changes() {
        let baseline = "    SKIP [TIME] test_first: unavailable\nSummary [TIME] 1 test run: 0 passed, 1 skipped\n";
        let candidate = "    SKIP [TIME] test_second: unavailable\nSummary [TIME] 1 test run: 0 passed, 1 skipped\n";

        let comparison = diagnostic_comparison(Some(baseline), Some(candidate), "main", "PR")
            .expect("diagnostics should be available");

        assert_eq!(comparison.baseline, comparison.candidate);
        assert!(comparison.workload_changed);
    }

    #[test]
    fn diagnostic_comparison_ignores_parallel_line_order() {
        let baseline = "    PASS [TIME] test_second\n    PASS [TIME] test_first\nSummary [TIME] 2 tests run: 2 passed, 0 skipped\n";
        let candidate = "    PASS [TIME] test_first\n    PASS [TIME] test_second\nSummary [TIME] 2 tests run: 2 passed, 0 skipped\n";

        let comparison = diagnostic_comparison(Some(baseline), Some(candidate), "main", "PR")
            .expect("diagnostics should be available");

        assert!(!comparison.workload_changed);
        assert!(comparison.diff.is_none());
    }

    #[test]
    fn diagnostic_code_blocks_expand_around_embedded_fences() {
        let mut markdown = String::new();

        write_code_block(&mut markdown, "text", "before\n```\nafter\n")
            .expect("code block should render");

        assert_eq!(markdown, "````text\nbefore\n```\nafter\n````\n");
    }

    #[test]
    fn diagnostics_report_omits_unchanged_diffs() {
        let output = normalize_diagnostic_text(
            "    PASS [   0.001s] test_pass\n\
             Summary [   0.100s] 1 test run: 1 passed, 0 skipped\n",
        );
        let mut comparison = project("requests", 21, 1.0, 1.0);
        comparison.diagnostics = diagnostic_comparison(Some(&output), Some(&output), "main", "PR");

        let report = report_with_projects(vec![comparison]);
        let markdown = diagnostics_markdown_report(&report, true).expect("report should render");
        let summary = diagnostics_markdown_report(&report, false).expect("summary should render");

        assert!(!summary.contains("full diagnostic output"));

        insta::assert_snapshot!(markdown, @"
        <!-- karva-diagnostic-comparison -->
        ### :white_check_mark: No diagnostic changes

        <details>
        <summary>Test result comparison</summary>

        | Project | Previous | New |
        | --- | --- | --- |
        | `requests` | :white_check_mark: 1 pass · 0 fail · 0 error · 0 skip | :white_check_mark: 1 pass · 0 fail · 0 error · 0 skip |

        </details>

        <details>
        <summary><code>requests</code> full diagnostic output</summary>

        Durations and memory addresses are normalized.

        #### Baseline: `main`

        ```text
            PASS [TIME] test_pass
        Summary [TIME] 1 test run: 1 passed, 0 skipped
        ```

        #### Candidate: `PR`

        ```text
            PASS [TIME] test_pass
        Summary [TIME] 1 test run: 1 passed, 0 skipped
        ```

        </details>
        ");
    }

    #[test]
    fn trend_uses_material_change_threshold() {
        assert_eq!(trend(-1.0), "faster");
        assert_eq!(trend(1.0), "slower");
        assert_eq!(trend(0.9), "flat");
        assert_eq!(trend(-0.9), "flat");
    }

    #[test]
    fn matrix_iterations_match_project_runtime() {
        assert_eq!(matrix_iterations("sniffio"), FAST_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("h11"), FAST_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("fastapi"), LONG_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("httpx"), LONG_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("requests"), EXTRA_LONG_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("werkzeug"), LONG_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("tomlkit"), MEDIUM_PROJECT_ITERATIONS);
    }

    #[test]
    fn test_failures_are_benchmarkable() {
        assert!(is_benchmarkable_exit(Some(
            karva_cli::ExitStatus::Success.to_i32()
        )));
        assert!(is_benchmarkable_exit(Some(
            karva_cli::ExitStatus::Failure.to_i32()
        )));
        assert!(!is_benchmarkable_exit(Some(
            karva_cli::ExitStatus::Error.to_i32()
        )));
        assert!(!is_benchmarkable_exit(None));
    }

    #[test]
    fn cli_benchmark_invocation_uses_normal_cached_status_output() {
        let num_workers = NonZeroUsize::MIN;
        let invocation = karva_invocation(
            &karva_benchmark::SYNTHETIC_PROJECT,
            Utf8Path::new("/tmp/project"),
            num_workers,
        )
        .expect("invocation should build");
        let worker_args = ["--num-workers".to_string(), num_workers.to_string()];

        assert!(
            invocation
                .args
                .windows(worker_args.len())
                .any(|args| args == worker_args)
        );
        assert!(
            !invocation
                .args
                .iter()
                .any(|arg| arg == "--status-level" || arg == "--final-status-level")
        );
        assert!(!invocation.args.iter().any(|arg| arg == "--no-cache"));
    }

    fn report_with_projects(projects: Vec<ProjectComparison>) -> ComparisonReport {
        report_with_metric(BenchmarkMetric::WallTime, projects)
    }

    fn report_with_metric(
        metric: BenchmarkMetric,
        projects: Vec<ProjectComparison>,
    ) -> ComparisonReport {
        ComparisonReport {
            metric,
            baseline_label: "main".to_string(),
            baseline_wheel: "baseline.whl".into(),
            candidate_label: "PR".to_string(),
            candidate_wheel: "candidate.whl".into(),
            projects,
        }
    }

    fn project(name: &str, iterations: usize, baseline: f64, candidate: f64) -> ProjectComparison {
        ProjectComparison {
            name: name.to_string(),
            iterations,
            baseline: measurement(baseline),
            candidate: measurement(candidate),
            percent_change: super::percent_change(baseline, candidate),
            diagnostics: None,
        }
    }

    fn measurement(median: f64) -> Measurement {
        Measurement {
            values: vec![median],
            median,
        }
    }
}
