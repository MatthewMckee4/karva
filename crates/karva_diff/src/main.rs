#![allow(clippy::print_stdout)]
use std::fs::File;
use std::io::Write;
use std::process::{Command, ExitCode};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::Parser;
use karva_system::path::absolute;
use karva_test_projects::{RealWorldProject, all_projects};
use tempfile::NamedTempFile;

#[derive(Debug, Parser)]
struct Args {
    old_karva_wheel: Utf8PathBuf,

    new_karva_wheel: Utf8PathBuf,

    output_diff_file: Utf8PathBuf,
}

fn main() -> Result<ExitCode> {
    let mut args = Args::parse();

    let cwd = {
        let cwd = std::env::current_dir().context("Failed to get the current working directory")?;
        Utf8PathBuf::from_path_buf(cwd)
                .map_err(|path| {
                    anyhow::anyhow!(
                        "The current working directory `{}` contains non-Unicode characters. ty only supports Unicode paths.",
                        path.display()
                    )
                })?
    };

    if args.old_karva_wheel.is_relative() {
        args.old_karva_wheel = absolute(&args.old_karva_wheel, &cwd);
    }

    if args.new_karva_wheel.is_relative() {
        args.new_karva_wheel = absolute(&args.new_karva_wheel, &cwd);
    }

    let mut output_file =
        File::create(&args.output_diff_file).context("Failed to create output file")?;

    for project in all_projects() {
        if let Some(diff) = run(&args, project.clone())? {
            writeln!(output_file, "## {}\n", diff.project_name)?;
            writeln!(output_file, "{}", diff.diff)?;
        }
    }

    Ok(ExitCode::SUCCESS)
}

struct ProjectDiff {
    project_name: String,
    diff: String,
}

fn run(args: &Args, project: RealWorldProject) -> Result<Option<ProjectDiff>> {
    println!("testing {:?}", project.name);
    let installed_project = project.setup(true)?;

    let paths: Vec<String> = installed_project
        .config
        .paths
        .iter()
        .map(|path| installed_project.path.join(path).to_string())
        .collect();

    let retry = installed_project
        .config
        .retry
        .unwrap_or_default()
        .to_string();

    // Install old wheel
    Command::new("uv")
        .arg("pip")
        .arg("install")
        .arg(&args.old_karva_wheel)
        .current_dir(&installed_project.path)
        .output()
        .context("Failed to install old karva wheel")?;

    let old_output = Command::new("uv")
        .arg("run")
        .arg("--no-project")
        .arg("karva")
        .arg("test")
        .args(&paths)
        .arg("--output-format")
        .arg("concise")
        .arg("--no-progress")
        .arg("--try-import-fixtures")
        .arg("--retry")
        .arg(retry.clone())
        .arg("--color")
        .arg("never")
        .current_dir(&installed_project.path)
        .output()
        .context("Failed to run old karva")?;

    // Uninstall old wheel
    Command::new("uv")
        .arg("pip")
        .arg("uninstall")
        .arg("karva")
        .arg("-y")
        .current_dir(&installed_project.path)
        .output()
        .context("Failed to uninstall old karva wheel")?;

    if !old_output.stdout.is_empty() {
        println!(
            "Old karva stdout:\n{}",
            String::from_utf8_lossy(&old_output.stdout)
        );
    }
    if !old_output.stderr.is_empty() {
        println!(
            "Old karva stderr:\n{}",
            String::from_utf8_lossy(&old_output.stderr)
        );
    }

    // Install new wheel
    Command::new("uv")
        .arg("pip")
        .arg("install")
        .arg(&args.new_karva_wheel)
        .current_dir(&installed_project.path)
        .output()
        .context("Failed to install new karva wheel")?;

    let new_output = Command::new("uv")
        .arg("run")
        .arg("--no-project")
        .arg("karva")
        .arg("test")
        .args(&paths)
        .arg("--output-format")
        .arg("concise")
        .arg("--no-progress")
        .arg("--try-import-fixtures")
        .arg("--retry")
        .arg(retry)
        .arg("--color")
        .arg("never")
        .current_dir(&installed_project.path)
        .output()
        .context("Failed to run new karva")?;

    if !new_output.stdout.is_empty() {
        println!(
            "New karva stdout:\n{}",
            String::from_utf8_lossy(&new_output.stdout)
        );
    }
    if !new_output.stderr.is_empty() {
        println!(
            "New karva stderr:\n{}",
            String::from_utf8_lossy(&new_output.stderr)
        );
    }

    let old_result = extract_test_result(&old_output.stdout);
    let new_result = extract_test_result(&new_output.stdout);

    if old_result == new_result {
        return Ok(None);
    }

    let mut old_temp = NamedTempFile::new()?;
    let mut new_temp = NamedTempFile::new()?;

    writeln!(old_temp, "{old_result}")?;
    writeln!(new_temp, "{new_result}")?;

    old_temp.flush()?;
    new_temp.flush()?;

    let diff_output = Command::new("diff")
        .arg(old_temp.path())
        .arg(new_temp.path())
        .output()
        .context("Failed to run diff")?;

    Ok(Some(ProjectDiff {
        project_name: installed_project.config.name.to_string(),
        diff: String::from_utf8_lossy(&diff_output.stdout).into_owned(),
    }))
}

fn extract_test_result(output: &[u8]) -> String {
    let output_str = String::from_utf8_lossy(output);

    // Find the line with `; finished in` and return everything before it on that line.
    output_str
        .lines()
        .find_map(|line| {
            line.find("; finished in")
                .map(|pos| line[..pos].to_string())
        })
        .unwrap_or_default()
}
