use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use camino::{Utf8Path, Utf8PathBuf};
use clap::Parser;
use fs_err::{self as fs, File};
use serde::{Deserialize, Serialize};

use karva_benchmark::BENCHMARK_PROJECTS;

use crate::metric::{
    BenchmarkMetric, MATERIAL_CHANGE_PERCENT, format_percent, is_material_change, median, trend,
    trend_marker,
};

#[derive(Debug, Parser)]
pub struct MergeReportsArgs {
    #[arg(long, value_name = "PATH")]
    input_dir: PathBuf,

    #[arg(long, value_name = "PATH")]
    output_markdown: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ComparisonReport {
    pub metric: BenchmarkMetric,
    pub baseline_label: String,
    pub baseline_wheel: Utf8PathBuf,
    pub candidate_label: String,
    pub candidate_wheel: Utf8PathBuf,
    pub projects: Vec<ProjectComparison>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProjectComparison {
    pub name: String,
    pub iterations: usize,
    pub baseline: Measurement,
    pub candidate: Measurement,
    pub percent_change: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Measurement {
    pub values: Vec<f64>,
    pub median: f64,
}

#[derive(Debug, Default)]
struct ReportSummary {
    faster: usize,
    slower: usize,
    unchanged: usize,
}

impl Measurement {
    pub fn new(values: Vec<f64>) -> Self {
        Self {
            median: median(&values),
            values,
        }
    }
}

pub fn merge_reports(args: MergeReportsArgs) -> Result<()> {
    let input_dir = crate::utf8_path(args.input_dir)?;
    let output_markdown = crate::utf8_path(args.output_markdown)?;
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

pub fn write_json(path: &Utf8Path, report: &ComparisonReport) -> Result<()> {
    create_parent_dir(path)?;
    let mut file = File::create(path).with_context(|| format!("Failed to create `{path}`"))?;
    serde_json::to_writer_pretty(&mut file, report)
        .with_context(|| format!("Failed to write `{path}`"))?;
    writeln!(file).with_context(|| format!("Failed to finish writing `{path}`"))?;
    Ok(())
}

pub fn write_markdown(path: &Utf8Path, report: &ComparisonReport) -> Result<()> {
    create_parent_dir(path)?;
    let body = markdown_report(report).context("Failed to render markdown benchmark report")?;
    fs::write(path, body).with_context(|| format!("Failed to write `{path}`"))
}

fn create_parent_dir(path: &Utf8Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent directory `{parent}`"))?;
    }
    Ok(())
}

fn markdown_report(report: &ComparisonReport) -> std::result::Result<String, std::fmt::Error> {
    let summary = ReportSummary::new(&report.projects);
    let mut body = String::from(report.metric.marker());
    body.push('\n');
    writeln!(body, "### {}", verdict(report.metric, &summary))?;
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
    writeln!(body)?;

    if summary.slower > 0 {
        writeln!(body, "> [!WARNING]")?;
        writeln!(
            body,
            "> Benchmark regressions were detected. Review the {} changes before merging.",
            report.metric.warning_label()
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
            "| {} | {} | `{}` | {} | {} | {} | {} |",
            trend_marker(project.percent_change),
            report.metric.mode_label(),
            project.name,
            report.metric.format_value(project.baseline.median),
            report.metric.format_value(project.candidate.median),
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

fn verdict(metric: BenchmarkMetric, summary: &ReportSummary) -> &'static str {
    if summary.slower > 0 {
        metric.regression_verdict()
    } else if summary.faster > 0 {
        metric.improvement_verdict()
    } else {
        metric.unchanged_verdict()
    }
}

fn write_summary_line(
    body: &mut String,
    marker: &str,
    count: usize,
    singular_label: &str,
) -> std::result::Result<(), std::fmt::Error> {
    let suffix = if count == 1 { "" } else { "s" };
    writeln!(body, "{marker} **{count}** {singular_label}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::{ComparisonReport, Measurement, ProjectComparison, markdown_report};
    use crate::metric::{BenchmarkMetric, percent_change};

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
    fn markdown_report_renders_memory_metric() {
        let report = report_with_metric(
            BenchmarkMetric::Memory,
            vec![project("memory-project", 21, 100_000.0, 90_000.0)],
        );

        let markdown = markdown_report(&report).expect("report should render");

        assert!(markdown.contains("<!-- karva-memory-benchmark-comparison -->"));
        assert!(markdown.contains("Merging this PR reduces memory usage"));
        assert!(markdown.contains("median peak RSS"));
        assert!(
            markdown.contains(
                "| :zap: | Memory | `memory-project` | 97.7 MiB | 87.9 MiB | -10.0% | 21 |"
            )
        );
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
            percent_change: percent_change(baseline, candidate),
        }
    }

    fn measurement(median: f64) -> Measurement {
        Measurement {
            values: vec![median],
            median,
        }
    }
}
