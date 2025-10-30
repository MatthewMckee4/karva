#![allow(clippy::print_stdout)]

use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use karva_core::testing::setup_module;
use karva_diffs::{DiagnosticReport, get_real_world_projects, run_project_diagnostics};

#[derive(Parser)]
#[command(name = "karva-diagnostics")]
#[command(about = "Run diagnostic tests on real-world projects", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run diagnostics on all configured projects and output JSON
    Run {
        /// Output file for the diagnostic report (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Compare two diagnostic reports and output a markdown diff
    Diff {
        /// Path to the base report (e.g., from main branch)
        #[arg(long)]
        base: PathBuf,

        /// Path to the head report (e.g., from PR branch)
        #[arg(long)]
        head: PathBuf,

        /// Output file for the diff markdown (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    // Initialize Python module
    setup_module();

    // Setup tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run { output } => run_diagnostics(output),
        Commands::Diff { base, head, output } => compare_diagnostics(&base, &head, output),
    }
}

fn run_diagnostics(output: Option<PathBuf>) -> Result<()> {
    let projects = get_real_world_projects();
    let mut report = DiagnosticReport::new();

    eprintln!("Running diagnostics on {} project(s)...", projects.len());

    for project in projects {
        eprintln!("  Testing project: {}", project.name);
        match run_project_diagnostics(project) {
            Ok(diagnostics) => {
                eprintln!(
                    "    ✓ {} tests ({} passed, {} failed, {} skipped)",
                    diagnostics.total_tests,
                    diagnostics.passed,
                    diagnostics.failed,
                    diagnostics.skipped
                );
                report.add_project(diagnostics);
            }
            Err(e) => {
                eprintln!("    ✗ Failed to run diagnostics: {e}");
                return Err(e);
            }
        }
    }

    let json = report.to_json()?;

    if let Some(path) = output {
        fs::write(&path, json).context("Failed to write output file")?;
        eprintln!("\nReport written to: {}", path.display());
    } else {
        println!("{json}");
    }

    Ok(())
}

fn compare_diagnostics(base: &PathBuf, head: &PathBuf, output: Option<PathBuf>) -> Result<()> {
    let base_json = fs::read_to_string(base)
        .context(format!("Failed to read base file: {:?}", base.display()))?;
    let head_json = fs::read_to_string(head)
        .context(format!("Failed to read head file: {:?}", head.display()))?;

    let base_report = DiagnosticReport::from_json(&base_json)?;
    let head_report = DiagnosticReport::from_json(&head_json)?;

    let diff = generate_diff(&base_report, &head_report);

    if let Some(path) = output {
        fs::write(&path, diff).context("Failed to write output file")?;
        eprintln!("Diff written to: {}", path.display());
    } else {
        println!("{diff}");
    }

    Ok(())
}

fn generate_diff(base: &DiagnosticReport, head: &DiagnosticReport) -> String {
    let mut diff = String::new();
    diff.push_str("# Diagnostic Diff Report\n\n");

    // Summary table
    diff.push_str("## Summary\n\n");
    diff.push_str("| Project | Tests | Passed | Failed | Skipped | Errors | Warnings |\n");
    diff.push_str("|---------|-------|--------|--------|---------|--------|----------|\n");

    for head_project in &head.projects {
        let base_project = base
            .projects
            .iter()
            .find(|p| p.project_name == head_project.project_name);

        if let Some(base_proj) = base_project {
            diff.push_str(&format!(
                "| {} | {} {} | {} {} | {} {} | {} {} | {} {} | {} {} |\n",
                head_project.project_name,
                head_project.total_tests,
                format_diff(base_proj.total_tests, head_project.total_tests),
                head_project.passed,
                format_diff(base_proj.passed, head_project.passed),
                head_project.failed,
                format_diff(base_proj.failed, head_project.failed),
                head_project.skipped,
                format_diff(base_proj.skipped, head_project.skipped),
                head_project.error_count,
                format_diff(base_proj.error_count, head_project.error_count),
                head_project.warning_count,
                format_diff(base_proj.warning_count, head_project.warning_count),
            ));
        } else {
            diff.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} |\n",
                head_project.project_name,
                head_project.total_tests,
                head_project.passed,
                head_project.failed,
                head_project.skipped,
                head_project.error_count,
                head_project.warning_count,
            ));
        }
    }

    diff.push_str("\n## Detailed Changes\n\n");

    for head_project in &head.projects {
        let base_project = base
            .projects
            .iter()
            .find(|p| p.project_name == head_project.project_name);

        if let Some(base_proj) = base_project {
            let has_changes = base_proj.total_tests != head_project.total_tests
                || base_proj.passed != head_project.passed
                || base_proj.failed != head_project.failed
                || base_proj.skipped != head_project.skipped
                || base_proj.error_count != head_project.error_count
                || base_proj.warning_count != head_project.warning_count;

            if has_changes {
                diff.push_str(&format!("### {}\n\n", head_project.project_name));

                if base_proj.passed != head_project.passed {
                    diff.push_str(&format!(
                        "- **Passed tests:** {} → {} {}\n",
                        base_proj.passed,
                        head_project.passed,
                        change_emoji(base_proj.passed, head_project.passed, true)
                    ));
                }

                if base_proj.failed != head_project.failed {
                    diff.push_str(&format!(
                        "- **Failed tests:** {} → {} {}\n",
                        base_proj.failed,
                        head_project.failed,
                        change_emoji(base_proj.failed, head_project.failed, false)
                    ));
                }

                if base_proj.error_count != head_project.error_count {
                    diff.push_str(&format!(
                        "- **Errors:** {} → {} {}\n",
                        base_proj.error_count,
                        head_project.error_count,
                        change_emoji(base_proj.error_count, head_project.error_count, false)
                    ));
                }

                if base_proj.warning_count != head_project.warning_count {
                    diff.push_str(&format!(
                        "- **Warnings:** {} → {} {}\n",
                        base_proj.warning_count,
                        head_project.warning_count,
                        change_emoji(base_proj.warning_count, head_project.warning_count, false)
                    ));
                }

                diff.push('\n');
            }
        }
    }

    diff
}

fn format_diff(base: usize, head: usize) -> String {
    if base == head {
        String::new()
    } else {
        let diff = head - base;
        if diff > 0 {
            format!("(+{diff})")
        } else {
            format!("({diff})")
        }
    }
}

const fn change_emoji(base: usize, head: usize, increase_is_good: bool) -> &'static str {
    if base == head {
        ""
    } else if head > base {
        if increase_is_good { "✅" } else { "❌" }
    } else if increase_is_good {
        "❌"
    } else {
        "✅"
    }
}
