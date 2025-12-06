#![allow(clippy::print_stdout)]
use std::{
    fs,
    io::Write,
    process::{Command, ExitCode},
};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::Parser;
use karva_project::absolute;
use karva_projects::{RealWorldProject, all_projects};
use tempfile::NamedTempFile;

#[derive(Debug, Parser)]
struct Args {
    old_karva_binary: Utf8PathBuf,

    new_karva_binary: Utf8PathBuf,

    output_diff_file: Utf8PathBuf,

    output_new_file: Option<Utf8PathBuf>,
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

    if args.old_karva_binary.is_relative() {
        args.old_karva_binary = absolute(&args.old_karva_binary, &cwd);
    }

    if args.new_karva_binary.is_relative() {
        args.new_karva_binary = absolute(&args.new_karva_binary, &cwd);
    }

    let mut old_temp = NamedTempFile::new()?;
    let mut new_temp = NamedTempFile::new()?;
    let mut accumulation_temp = NamedTempFile::new()?;

    for project in all_projects() {
        run(
            &args,
            project.clone(),
            &mut old_temp,
            &mut new_temp,
            &mut accumulation_temp,
        )?;
    }

    old_temp.flush()?;
    new_temp.flush()?;

    let diff_output = Command::new("diff")
        .arg(old_temp.path())
        .arg(new_temp.path())
        .output()
        .context("Failed to run diff")?;

    fs::write(&args.output_diff_file, &diff_output.stdout)
        .context("Failed to write output file")?;

    if let Some(output_new_file) = &args.output_new_file {
        accumulation_temp.flush()?;
        fs::copy(accumulation_temp.path(), output_new_file)
            .context("Failed to write new output file")?;
    }

    Ok(ExitCode::SUCCESS)
}

fn run(
    args: &Args,
    project: RealWorldProject,
    old_temp: &mut NamedTempFile,
    new_temp: &mut NamedTempFile,
    accumulation_temp: &mut NamedTempFile,
) -> Result<()> {
    println!("testing {:?}", project.name);
    let installed_project = project.setup(true)?;

    let paths: Vec<String> = installed_project
        .config
        .paths
        .iter()
        .map(|path| installed_project.path.join(path).to_string())
        .collect();

    let old_output = Command::new("uv")
        .arg("run")
        .arg("--no-project")
        .arg(&args.old_karva_binary)
        .arg("test")
        .args(&paths)
        .arg("--output-format")
        .arg("concise")
        .arg("--no-progress")
        .arg("--color")
        .arg("never")
        .current_dir(&installed_project.path)
        .output()
        .context("Failed to run old karva binary")?;

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

    let new_output = Command::new("uv")
        .arg("run")
        .arg("--no-project")
        .arg(&args.new_karva_binary)
        .arg("test")
        .args(&paths)
        .arg("--output-format")
        .arg("concise")
        .arg("--no-progress")
        .arg("--color")
        .arg("never")
        .current_dir(&installed_project.path)
        .output()
        .context("Failed to run new karva binary")?;

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

    write!(
        accumulation_temp,
        "{}\n\nstdout\n\n{}\nstderr\n\n{}----------------\n\n",
        installed_project.config.name,
        String::from_utf8_lossy(&new_output.stdout),
        String::from_utf8_lossy(&new_output.stderr)
    )?;

    let old_result = extract_test_result(&old_output.stdout);

    let new_result = extract_test_result(&new_output.stdout);

    writeln!(old_temp, "{}", installed_project.config.name)?;
    writeln!(old_temp, "{old_result}")?;

    writeln!(new_temp, "{}", installed_project.config.name)?;
    writeln!(new_temp, "{new_result}")?;

    Ok(())
}

fn extract_test_result(output: &[u8]) -> String {
    let output_str = String::from_utf8_lossy(output);

    // Strip `; finished in` and all text after it.
    let strip_index = output_str.find("; finished in").unwrap_or(output_str.len());

    output_str[..strip_index].to_string()
}
