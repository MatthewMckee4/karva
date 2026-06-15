use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use karva_diagnostic::TestResultStats;
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(about = "Run pinned Karva wall-time benchmarks and write a JSON report.")]
struct Args {
    #[arg(long, default_value_t = 1)]
    warmups: usize,

    #[arg(long, default_value_t = 5)]
    samples: usize,

    #[arg(long, value_name = "NAME")]
    project: Vec<String>,

    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    warmups: usize,
    samples: usize,
    workers: usize,
    projects: Vec<ProjectReport>,
}

#[derive(Debug, Serialize)]
struct ProjectReport {
    benchmark: &'static str,
    repository: &'static str,
    commit: &'static str,
    python_version: String,
    paths: Vec<&'static str>,
    stats: StatsReport,
    durations_ms: Vec<f64>,
    min_ms: f64,
    median_ms: f64,
    mean_ms: f64,
    max_ms: f64,
    relative_std_dev_pct: f64,
}

#[derive(Debug, Serialize)]
struct StatsReport {
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    flaky: usize,
    slow: usize,
}

fn main() -> Result<()> {
    let args = Args::parse();
    anyhow::ensure!(args.samples > 0, "--samples must be greater than zero");

    let projects = selected_projects(&args.project)?;
    let mut reports = Vec::with_capacity(projects.len());
    for config in projects {
        reports.push(run_project(config, args.warmups, args.samples)?);
    }

    let report = BenchmarkReport {
        warmups: args.warmups,
        samples: args.samples,
        workers: karva_benchmark::WORKER_COUNT,
        projects: reports,
    };
    let json = serde_json::to_string_pretty(&report)?;

    if let Some(path) = args.output {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create report directory: {}", parent.display())
            })?;
        }
        std::fs::write(&path, &json)
            .with_context(|| format!("Failed to write benchmark report: {}", path.display()))?;
    }

    println!("{json}");

    Ok(())
}

fn selected_projects(names: &[String]) -> Result<Vec<&'static karva_benchmark::BenchmarkProject>> {
    if names.is_empty() {
        return Ok(karva_benchmark::BENCHMARK_PROJECTS.iter().collect());
    }

    let mut projects = Vec::with_capacity(names.len());
    for name in names {
        let project = karva_benchmark::find_benchmark_project(name).with_context(|| {
            let names = karva_benchmark::BENCHMARK_PROJECTS
                .iter()
                .map(|project| project.name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("Unknown benchmark project `{name}`. Available projects: {names}")
        })?;
        projects.push(project);
    }
    Ok(projects)
}

fn run_project(
    config: &'static karva_benchmark::BenchmarkProject,
    warmups: usize,
    samples: usize,
) -> Result<ProjectReport> {
    let project = karva_benchmark::prepare_benchmark_project(config)?;

    for _ in 0..warmups {
        karva_benchmark::clean_project_cache(project.cwd())?;
        karva_benchmark::try_run_project(&project)?;
    }

    let mut durations = Vec::with_capacity(samples);
    let mut stats = None;
    for _ in 0..samples {
        karva_benchmark::clean_project_cache(project.cwd())?;
        let start = Instant::now();
        let output = karva_benchmark::try_run_project(&project)?;
        durations.push(start.elapsed());
        stats = Some(output.results.stats);
    }

    let stats = stats.context("Benchmark did not record any test stats")?;
    ProjectReport::new(config, &stats, durations)
}

impl ProjectReport {
    fn new(
        config: &'static karva_benchmark::BenchmarkProject,
        stats: &TestResultStats,
        durations: Vec<Duration>,
    ) -> Result<Self> {
        let mut durations_ms: Vec<f64> = durations
            .into_iter()
            .map(|duration| duration.as_secs_f64() * 1_000.0)
            .collect();
        durations_ms.sort_by(f64::total_cmp);

        let mean_ms = mean(&durations_ms)?;
        let std_dev_ms = variance(&durations_ms, mean_ms)?.sqrt();

        Ok(Self {
            benchmark: config.name,
            repository: config.repository,
            commit: config.commit,
            python_version: config.python_version.to_string(),
            paths: config.paths.to_vec(),
            stats: StatsReport::from(stats),
            min_ms: durations_ms[0],
            median_ms: median(&durations_ms),
            mean_ms,
            max_ms: durations_ms[durations_ms.len() - 1],
            relative_std_dev_pct: if mean_ms.abs() < f64::EPSILON {
                0.0
            } else {
                (std_dev_ms / mean_ms) * 100.0
            },
            durations_ms,
        })
    }
}

impl From<&TestResultStats> for StatsReport {
    fn from(stats: &TestResultStats) -> Self {
        Self {
            total: stats.total(),
            passed: stats.passed(),
            failed: stats.failed(),
            skipped: stats.skipped(),
            flaky: stats.flaky(),
            slow: stats.slow(),
        }
    }
}

fn mean(values: &[f64]) -> Result<f64> {
    Ok(values.iter().sum::<f64>() / len_as_f64(values)?)
}

fn median(values: &[f64]) -> f64 {
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        f64::midpoint(values[mid - 1], values[mid])
    } else {
        values[mid]
    }
}

fn variance(values: &[f64], mean: f64) -> Result<f64> {
    Ok(values
        .iter()
        .map(|value| {
            let distance = value - mean;
            distance * distance
        })
        .sum::<f64>()
        / len_as_f64(values)?)
}

fn len_as_f64(values: &[f64]) -> Result<f64> {
    let len = u32::try_from(values.len()).context("Too many benchmark samples")?;
    Ok(f64::from(len))
}
