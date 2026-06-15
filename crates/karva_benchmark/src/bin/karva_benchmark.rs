use std::fs::File;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use std::{collections::HashSet, io};

use anyhow::{Context as _, Result};
use camino::{Utf8Path, Utf8PathBuf};
use clap::Parser;
use karva_benchmark::{BENCHMARK_PROJECTS, BenchmarkProject, CLI_BENCHMARK_PROJECTS, WORKER_COUNT};
use serde::{Deserialize, Serialize};

const MATERIAL_CHANGE_PERCENT: f64 = 1.0;
const FAST_PROJECT_ITERATIONS: usize = 21;
const MEDIUM_PROJECT_ITERATIONS: usize = 15;
const SLOW_PROJECT_ITERATIONS: usize = 9;

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
    output_json: PathBuf,

    #[arg(long, value_name = "PATH")]
    output_markdown: PathBuf,
}

#[derive(Debug, Parser)]
struct MergeReportsArgs {
    #[arg(long, value_name = "PATH")]
    input_dir: PathBuf,

    #[arg(long, value_name = "PATH")]
    output_markdown: PathBuf,
}

#[derive(Debug, Serialize)]
struct Matrix {
    include: Vec<MatrixProject>,
}

#[derive(Debug, Serialize)]
struct MatrixProject {
    project: &'static str,
    iterations: usize,
}

#[derive(Debug, Deserialize, Serialize)]
struct ComparisonReport {
    baseline_label: String,
    baseline_wheel: Utf8PathBuf,
    candidate_label: String,
    candidate_wheel: Utf8PathBuf,
    projects: Vec<ProjectComparison>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ProjectComparison {
    name: String,
    iterations: usize,
    baseline: Measurement,
    candidate: Measurement,
    percent_change: f64,
}

#[derive(Debug, Deserialize, Serialize)]
struct Measurement {
    durations_secs: Vec<f64>,
    median_secs: f64,
}

#[derive(Debug, Default)]
struct ReportSummary {
    faster: usize,
    slower: usize,
    unchanged: usize,
}

struct Subject<'a> {
    label: &'a str,
    wheel: &'a Utf8Path,
    durations: &'a mut Vec<f64>,
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
        include: CLI_BENCHMARK_PROJECTS
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

    let baseline_wheel = utf8_path(args.baseline_wheel)?;
    let candidate_wheel = utf8_path(args.candidate_wheel)?;
    let output_json = utf8_path(args.output_json)?;
    let output_markdown = utf8_path(args.output_markdown)?;
    let projects = selected_projects(&args.projects)?;
    let mut comparisons = Vec::with_capacity(projects.len());

    for config in projects {
        eprintln!("Preparing benchmark project `{}`", config.name);
        let project = karva_benchmark::prepare_benchmark_project_environment(config)
            .with_context(|| format!("Failed to prepare benchmark project `{}`", config.name))?;
        let mut baseline_durations = Vec::with_capacity(args.iterations);
        let mut candidate_durations = Vec::with_capacity(args.iterations);

        for iteration in 0..args.iterations {
            let mut baseline = Subject {
                label: &args.baseline_label,
                wheel: &baseline_wheel,
                durations: &mut baseline_durations,
            };
            let mut candidate = Subject {
                label: &args.candidate_label,
                wheel: &candidate_wheel,
                durations: &mut candidate_durations,
            };

            if iteration % 2 == 0 {
                run_subject(config, project.cwd(), &mut baseline)?;
                run_subject(config, project.cwd(), &mut candidate)?;
            } else {
                run_subject(config, project.cwd(), &mut candidate)?;
                run_subject(config, project.cwd(), &mut baseline)?;
            }
        }

        let baseline = Measurement::new(baseline_durations);
        let candidate = Measurement::new(candidate_durations);
        let percent_change = percent_change(baseline.median_secs, candidate.median_secs);

        comparisons.push(ProjectComparison {
            name: config.name.to_string(),
            iterations: args.iterations,
            baseline,
            candidate,
            percent_change,
        });
    }

    let report = ComparisonReport {
        baseline_label: args.baseline_label,
        baseline_wheel,
        candidate_label: args.candidate_label,
        candidate_wheel,
        projects: comparisons,
    };

    write_json(&output_json, &report)?;
    write_markdown(&output_markdown, &report)?;

    Ok(())
}

fn merge_reports(args: MergeReportsArgs) -> Result<()> {
    let input_dir = utf8_path(args.input_dir)?;
    let output_markdown = utf8_path(args.output_markdown)?;
    let report = merge_report_files(&input_dir)?;

    write_markdown(&output_markdown, &report)
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
    for entry in std::fs::read_dir(input_dir)
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
    fn new(durations_secs: Vec<f64>) -> Self {
        Self {
            median_secs: median(&durations_secs),
            durations_secs,
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
    subject: &mut Subject<'_>,
) -> Result<()> {
    karva_benchmark::install_benchmark_tools(config, project_root, subject.wheel)
        .with_context(|| format!("Failed to install `{}` benchmark wheel", subject.label))?;
    karva_benchmark::clean_project_cache(project_root)
        .with_context(|| format!("Failed to clean benchmark cache for `{}`", config.name))?;

    let duration = run_project_cli(config, project_root)
        .with_context(|| format!("Failed to run `{}` with `{}`", config.name, subject.label))?;
    subject.durations.push(duration.as_secs_f64());

    eprintln!(
        "{} / {}: {}",
        config.name,
        subject.label,
        format_seconds(duration.as_secs_f64())
    );

    Ok(())
}

fn run_project_cli(config: &BenchmarkProject, project_root: &Utf8Path) -> Result<Duration> {
    let bin_dir = venv_bin_dir(project_root);
    let binary = bin_dir.join(executable_name("karva"));
    let path = path_with_venv_first(&bin_dir)?;
    let worker_count = WORKER_COUNT.to_string();

    let mut command = Command::new(&binary);
    command.current_dir(project_root).env("PATH", path).args([
        "test",
        "--num-workers",
        &worker_count,
        "--no-cache",
        "--no-ignore",
        "--output-format",
        "concise",
        "--status-level",
        "none",
        "--final-status-level",
        "none",
    ]);

    if config.try_import_fixtures {
        command.arg("--try-import-fixtures");
    }
    command.args(config.paths);

    let start = Instant::now();
    let output = command
        .output()
        .with_context(|| format!("Failed to execute `{binary}`"))?;
    let elapsed = start.elapsed();

    anyhow::ensure!(
        output.status.success(),
        "Karva exited with status {} for `{}`\nstdout:\n{}\nstderr:\n{}",
        output.status,
        config.name,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    Ok(elapsed)
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
    std::fs::write(path, body).with_context(|| format!("Failed to write `{path}`"))
}

fn create_parent_dir(path: &Utf8Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent directory `{parent}`"))?;
    }
    Ok(())
}

fn markdown_report(report: &ComparisonReport) -> std::result::Result<String, std::fmt::Error> {
    use std::fmt::Write as _;

    let summary = ReportSummary::new(&report.projects);
    let mut body = String::from("<!-- karva-benchmark-comparison -->\n");
    writeln!(body, "### {}", verdict(&summary))?;
    writeln!(body)?;
    writeln!(
        body,
        "Baseline: `{}`. Candidate: `{}`. Each benchmark compares median CLI wall time on one GitHub Actions runner, alternating install order. Runs are configured per project. Lower is better.",
        report.baseline_label, report.candidate_label
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
    writeln!(body)?;

    if summary.slower > 0 {
        writeln!(body, "> [!WARNING]")?;
        writeln!(
            body,
            "> Benchmark regressions were detected. Review the wall-time changes before merging."
        )?;
        writeln!(body)?;
    }

    let visible_projects = report
        .projects
        .iter()
        .filter(|project| is_material_change(project.percent_change))
        .collect::<Vec<_>>();

    if visible_projects.is_empty() {
        writeln!(
            body,
            "No project changed by at least {MATERIAL_CHANGE_PERCENT:.1}%."
        )?;
        return Ok(body);
    }

    body.push_str("#### Performance Changes\n\n");
    body.push_str("|  | Mode | Benchmark | Base | Head | Change | Runs |\n");
    body.push_str("| --- | --- | --- | ---: | ---: | ---: | ---: |\n");

    for project in visible_projects {
        writeln!(
            body,
            "| {} | WallTime | `{}` | {} | {} | {} | {} |",
            trend_marker(project.percent_change),
            project.name,
            format_seconds(project.baseline.median_secs),
            format_seconds(project.candidate.median_secs),
            format_percent(project.percent_change),
            project.iterations,
        )?;
    }

    Ok(body)
}

impl ReportSummary {
    fn new(projects: &[ProjectComparison]) -> Self {
        let mut summary = Self::default();

        for project in projects {
            match trend(project.percent_change) {
                "faster" => summary.faster += 1,
                "slower" => summary.slower += 1,
                _ => summary.unchanged += 1,
            }
        }

        summary
    }
}

fn verdict(summary: &ReportSummary) -> &'static str {
    if summary.slower > 0 {
        "Merging this PR may alter performance"
    } else if summary.faster > 0 {
        "Merging this PR improves performance"
    } else {
        "Merging this PR will not alter performance"
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
        "packaging" | "pyparsing" => SLOW_PROJECT_ITERATIONS,
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

fn trend_marker(percent_change: f64) -> &'static str {
    match trend(percent_change) {
        "faster" => ":zap:",
        "slower" => ":x:",
        _ => ":white_check_mark:",
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
    if let Some(existing_path) = std::env::var_os("PATH") {
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
    use super::{
        ComparisonReport, FAST_PROJECT_ITERATIONS, MEDIUM_PROJECT_ITERATIONS, Measurement,
        ProjectComparison, SLOW_PROJECT_ITERATIONS, markdown_report, matrix_iterations, trend,
    };

    #[test]
    fn markdown_report_omits_projects_under_material_change_threshold() {
        let report = report_with_projects(vec![
            project("flat-project", 21, 1.0, 1.004),
            project("faster-project", 21, 1.0, 0.99),
            project("slower-project", 15, 1.0, 1.012),
        ]);

        let markdown = markdown_report(&report).expect("report should render");

        assert!(!markdown.contains("flat-project"));
        assert!(markdown.contains(":zap: **1** improved benchmark"));
        assert!(markdown.contains(":x: **1** regressed benchmark"));
        assert!(markdown.contains(":white_check_mark: **1** unchanged benchmark"));
        assert!(markdown.contains("> [!WARNING]"));
        assert!(
            markdown.contains(
                "| :zap: | WallTime | `faster-project` | 1.000 s | 990.0 ms | -1.0% | 21 |"
            )
        );
        assert!(
            markdown
                .contains("| :x: | WallTime | `slower-project` | 1.000 s | 1.012 s | +1.2% | 15 |")
        );
    }

    #[test]
    fn markdown_report_says_when_all_projects_are_under_material_change_threshold() {
        let report = report_with_projects(vec![project("flat-project", 21, 1.0, 1.004)]);

        let markdown = markdown_report(&report).expect("report should render");

        assert!(markdown.contains("No project changed by at least 1.0%."));
        assert!(markdown.contains("Merging this PR will not alter performance"));
        assert!(!markdown.contains("|  | Mode | Benchmark | Base | Head | Change | Runs |"));
        assert!(!markdown.contains("> [!WARNING]"));
        assert!(!markdown.contains("flat-project"));
    }

    #[test]
    fn trend_uses_material_change_threshold() {
        assert_eq!(trend(-1.0), "faster");
        assert_eq!(trend(1.0), "slower");
        assert_eq!(trend(0.9), "flat");
        assert_eq!(trend(-0.9), "flat");
    }

    #[test]
    fn matrix_iterations_are_higher_for_fast_projects() {
        assert_eq!(matrix_iterations("sniffio"), FAST_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("h11"), FAST_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("tomlkit"), MEDIUM_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("packaging"), SLOW_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("pyparsing"), SLOW_PROJECT_ITERATIONS);
    }

    fn report_with_projects(projects: Vec<ProjectComparison>) -> ComparisonReport {
        ComparisonReport {
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
        }
    }

    fn measurement(median_secs: f64) -> Measurement {
        Measurement {
            durations_secs: vec![median_secs],
            median_secs,
        }
    }
}
