use std::io::{self, BufWriter, Write};
use std::process::{ExitCode, Termination};

use anyhow::Result;
use karva_core::db::path::{SystemPath, SystemPathBuf};

use crate::args::{Args, Command, TestCommand};
use crate::logging::setup_tracing;
use anyhow::{Context, anyhow};
use clap::Parser;
use colored::Colorize;

mod args;
mod logging;
mod version;

pub fn main() -> ExitStatus {
    run().unwrap_or_else(|error| {
        use std::io::Write;

        let mut stderr = std::io::stderr().lock();

        writeln!(stderr, "{}", "Karva failed".red().bold()).ok();
        for cause in error.chain() {
            if let Some(ioerr) = cause.downcast_ref::<io::Error>() {
                if ioerr.kind() == io::ErrorKind::BrokenPipe {
                    return ExitStatus::Success;
                }
            }

            writeln!(stderr, "  {} {cause}", "Cause:".bold()).ok();
        }

        ExitStatus::Error
    })
}

fn run() -> anyhow::Result<ExitStatus> {
    let args = wild::args_os();
    let args = argfile::expand_args_from(args, argfile::parse_fromfile, argfile::PREFIX)
        .context("Failed to read CLI arguments from file")?;
    let args = Args::parse_from(args);

    match args.command {
        Command::Test(test_args) => run_test(&test_args).map(|()| ExitStatus::Success),
        Command::Version => version().map(|()| ExitStatus::Success),
    }
}

pub(crate) fn version() -> Result<()> {
    let mut stdout = BufWriter::new(io::stdout().lock());
    let version_info = crate::version::version();
    writeln!(stdout, "karva {}", &version_info)?;
    Ok(())
}

pub(crate) fn run_test(args: &TestCommand) -> Result<()> {
    let verbosity = args.verbosity.level();
    let _guard = setup_tracing(verbosity)?;

    let cwd = {
        let cwd = std::env::current_dir().context("Failed to get the current working directory")?;
        SystemPathBuf::from_path_buf(cwd)
            .map_err(|path| {
                anyhow!(
                    "The current working directory `{}` contains non-Unicode characters. Karva only supports Unicode paths.",
                    path.display()
                )
            })?
    };

    let mut check_paths: Vec<_> = args
        .paths
        .iter()
        .map(|path| SystemPath::absolute(path, &cwd))
        .collect();

    if check_paths.is_empty() {
        tracing::debug!("No paths provided, using current working directory");
        check_paths.push(cwd);
    }

    Ok(())
}

#[derive(Copy, Clone)]
pub enum ExitStatus {
    /// Checking was successful and there were no errors.
    Success = 0,

    /// Checking was successful but there were errors.
    Failure = 1,

    /// Checking failed.
    Error = 2,
}

impl Termination for ExitStatus {
    fn report(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}
