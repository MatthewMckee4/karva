use std::io::{self, BufWriter, Write};
use std::process::{ExitCode, Termination};

use anyhow::Result;
use karva_core::diagnostics::StdoutDiagnosticWriter;
use karva_core::path::{PythonTestPath, SystemPath, SystemPathBuf};
use karva_core::project::Project;
use karva_core::runner::Runner;

use crate::args::{Args, Command, TestCommand};
use crate::logging::setup_tracing;
use anyhow::{anyhow, Context};
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
        Command::Test(test_args) => test(&test_args),
        Command::Version => version().map(|()| ExitStatus::Success),
    }
}

pub(crate) fn version() -> Result<()> {
    let mut stdout = BufWriter::new(io::stdout().lock());
    let version_info = crate::version::version();
    writeln!(stdout, "karva {}", &version_info)?;
    Ok(())
}

pub(crate) fn test(args: &TestCommand) -> Result<ExitStatus> {
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

    let diagnostics = Box::new(StdoutDiagnosticWriter::default());

    let mut paths: Vec<PythonTestPath> = args
        .paths
        .iter()
        .map(|path| SystemPath::absolute(path, &cwd))
        .filter_map(|path| {
            let path = PythonTestPath::new(&path);
            match path {
                Ok(path) => Some(path),
                Err(e) => {
                    eprintln!("{}", e.to_string().yellow());
                    None
                }
            }
        })
        .collect();

    if args.paths.is_empty() {
        tracing::debug!("No paths provided, trying to resolve current working directory");
        if let Ok(path) = PythonTestPath::new(&cwd) {
            paths.push(path);
        }
    }

    if paths.is_empty() {
        eprintln!(
            "{}",
            "No paths provided and could not resolve current working directory"
                .red()
                .bold()
        );
        return Ok(ExitStatus::Error);
    }

    let project = Project::new(cwd, paths, args.test_prefix.clone());
    let mut runner = Runner::new(&project, diagnostics);
    let runner_result = runner.run();

    if runner_result.passed() {
        Ok(ExitStatus::Success)
    } else {
        Ok(ExitStatus::Failure)
    }
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
