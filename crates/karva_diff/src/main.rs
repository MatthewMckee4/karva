use std::process::{Command, ExitCode};

use anyhow::Result;
use camino::Utf8PathBuf;
use clap::Parser;
use karva_test::{RealWorldProject, all_projects};

#[derive(Debug, Parser)]
struct Args {
    old_karva_binary: Utf8PathBuf,

    new_karva_binary: Utf8PathBuf,
}

fn main() -> Result<ExitCode> {
    let args = Args::parse();

    for project in all_projects() {
        run(&args, &project)?;
    }

    Ok(ExitCode::SUCCESS)
}

fn run(args: &Args, project: &RealWorldProject) -> Result<()> {
    Command::new(&args.old_karva_binary)
        .arg("test")
        .arg(&args.new_karva_binary)
        .status()?;

    Ok(())
}
