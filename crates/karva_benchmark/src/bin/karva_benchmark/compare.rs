use std::path::PathBuf;

use anyhow::{Context as _, Result};
use camino::Utf8Path;
use clap::Parser;
use karva_benchmark::{BENCHMARK_PROJECTS, BenchmarkProject};

use crate::metric::{BenchmarkMetric, percent_change};
use crate::report::{ComparisonReport, Measurement, ProjectComparison, write_json, write_markdown};
use crate::runner::{run_project_cli, warm_project_cache};

#[derive(Debug, Parser)]
pub struct CompareArgs {
    #[arg(long, value_enum, default_value_t = BenchmarkMetric::WallTime)]
    metric: BenchmarkMetric,

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

struct Subject<'a> {
    label: &'a str,
    wheel: &'a Utf8Path,
    values: &'a mut Vec<f64>,
}

pub fn compare(args: CompareArgs) -> Result<()> {
    anyhow::ensure!(args.iterations > 0, "iterations must be greater than zero");

    let baseline_wheel = crate::utf8_path(args.baseline_wheel)?;
    let candidate_wheel = crate::utf8_path(args.candidate_wheel)?;
    let output_json = crate::utf8_path(args.output_json)?;
    let output_markdown = crate::utf8_path(args.output_markdown)?;
    let projects = selected_projects(&args.projects)?;
    let mut comparisons = Vec::with_capacity(projects.len());

    for config in projects {
        eprintln!("Preparing benchmark project `{}`", config.name);
        let project = karva_benchmark::prepare_benchmark_project_environment(config)
            .with_context(|| format!("Failed to prepare benchmark project `{}`", config.name))?;
        let mut baseline_values = Vec::with_capacity(args.iterations);
        let mut candidate_values = Vec::with_capacity(args.iterations);

        for iteration in 0..args.iterations {
            let mut baseline = Subject {
                label: &args.baseline_label,
                wheel: &baseline_wheel,
                values: &mut baseline_values,
            };
            let mut candidate = Subject {
                label: &args.candidate_label,
                wheel: &candidate_wheel,
                values: &mut candidate_values,
            };

            if iteration.is_multiple_of(2) {
                run_subject(args.metric, config, project.cwd(), &mut baseline)?;
                run_subject(args.metric, config, project.cwd(), &mut candidate)?;
            } else {
                run_subject(args.metric, config, project.cwd(), &mut candidate)?;
                run_subject(args.metric, config, project.cwd(), &mut baseline)?;
            }
        }

        let baseline = Measurement::new(baseline_values);
        let candidate = Measurement::new(candidate_values);
        let percent_change = percent_change(baseline.median, candidate.median);

        comparisons.push(ProjectComparison {
            name: config.name.to_string(),
            iterations: args.iterations,
            baseline,
            candidate,
            percent_change,
        });
    }

    let report = ComparisonReport {
        metric: args.metric,
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
    metric: BenchmarkMetric,
    config: &BenchmarkProject,
    project_root: &Utf8Path,
    subject: &mut Subject<'_>,
) -> Result<()> {
    karva_benchmark::install_benchmark_tools(config, project_root, subject.wheel)
        .with_context(|| format!("Failed to install `{}` benchmark wheel", subject.label))?;
    karva_benchmark::clean_project_cache(project_root)
        .with_context(|| format!("Failed to clean benchmark cache for `{}`", config.name))?;
    warm_project_cache(config, project_root)
        .with_context(|| format!("Failed to warm benchmark cache for `{}`", config.name))?;

    let value = run_project_cli(metric, config, project_root)
        .with_context(|| format!("Failed to run `{}` with `{}`", config.name, subject.label))?;
    subject.values.push(value);

    eprintln!(
        "{} / {} / {}: {}",
        config.name,
        subject.label,
        metric.mode_label(),
        metric.format_value(value)
    );

    Ok(())
}
