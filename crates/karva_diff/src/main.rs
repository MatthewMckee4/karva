#![allow(clippy::print_stdout)]
use std::{
    fs,
    io::Write,
    process::{Command, ExitCode},
};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::Parser;
use karva_test::{RealWorldProject, all_projects};
use tempfile::NamedTempFile;

#[derive(Debug, Parser)]
struct Args {
    old_karva_binary: Utf8PathBuf,

    new_karva_binary: Utf8PathBuf,

    output_diff_file: Utf8PathBuf,

    output_new_file: Option<Utf8PathBuf>,
}

fn main() -> Result<ExitCode> {
    let args = Args::parse();

    let mut old_temp = NamedTempFile::new()?;
    let mut new_temp = NamedTempFile::new()?;

    for project in all_projects() {
        run(&args, project, &mut old_temp, &mut new_temp)?;
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

    if let Some(output_new_file) = args.output_new_file {
        fs::copy(new_temp.path(), &output_new_file).context("Failed to copy new file")?;
    }

    Ok(ExitCode::SUCCESS)
}

fn run(
    args: &Args,
    project: RealWorldProject,
    old_temp: &mut NamedTempFile,
    new_temp: &mut NamedTempFile,
) -> Result<()> {
    let installed_project = project.setup()?;

    let paths: Vec<String> = installed_project
        .config
        .paths
        .iter()
        .map(|path| installed_project.path.join(path).to_string())
        .collect();

    let old_output = Command::new(&args.old_karva_binary)
        .arg("test")
        .arg("-vv")
        .args(&paths)
        .output()
        .context("Failed to run old karva binary")?;

    println!("Old output: {old_output:?}");

    let new_output = Command::new(&args.new_karva_binary)
        .arg("test")
        .arg("-vv")
        .args(&paths)
        .output()
        .context("Failed to run new karva binary")?;

    println!("New output: {new_output:?}");

    let old_result = extract_test_result(&old_output.stdout)?;

    let new_result = extract_test_result(&new_output.stdout)?;

    writeln!(old_temp, "{}", installed_project.config.name)?;
    writeln!(old_temp, "{old_result}")?;

    writeln!(new_temp, "{}", installed_project.config.name)?;
    writeln!(new_temp, "{new_result}")?;

    Ok(())
}

fn extract_test_result(output: &[u8]) -> Result<String> {
    let output_str = String::from_utf8_lossy(output);

    let result = output_str
        .lines()
        .filter(|line| line.starts_with("test result"))
        .next_back()
        .context("No line starting with 'test result' found")?;

    Ok(result.to_string())
}
