use std::{
    ffi::OsString,
    io::{self, BufWriter, Write},
    process::{ExitCode, Termination},
};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use colored::Colorize;
use karva_core::{
    diagnostic::reporter::{DummyReporter, Reporter},
    runner::TestRunner,
    utils::current_python_version,
};
use karva_project::{
    path::{SystemPath, SystemPathBuf},
    project::{Project, ProjectMetadata},
};

use crate::{
    args::{Command, TestCommand},
    logging::setup_tracing,
};

mod args;
mod logging;
mod version;

pub use args::Args;

#[must_use]
pub fn karva_main(f: impl FnOnce(Vec<OsString>) -> Vec<OsString>) -> ExitStatus {
    run(f).unwrap_or_else(|error| {
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

fn run(f: impl FnOnce(Vec<OsString>) -> Vec<OsString>) -> anyhow::Result<ExitStatus> {
    let args = wild::args_os();

    let args = f(
        argfile::expand_args_from(args, argfile::parse_fromfile, argfile::PREFIX)
            .context("Failed to read CLI arguments from file")?,
    );

    let args = Args::parse_from(args);

    match args.command {
        Command::Test(test_args) => test(test_args),
        Command::Version => version().map(|()| ExitStatus::Success),
    }
}

pub(crate) fn version() -> Result<()> {
    let mut stdout = BufWriter::new(io::stdout().lock());
    let version_info = crate::version::version();
    writeln!(stdout, "karva {}", &version_info)?;
    Ok(())
}

pub(crate) fn test(args: TestCommand) -> Result<ExitStatus> {
    let verbosity = args.verbosity.level();
    let _guard = setup_tracing(verbosity);

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

    let mut paths: Vec<_> = args
        .paths
        .iter()
        .map(|path| SystemPath::absolute(path, &cwd))
        .collect();

    if args.paths.is_empty() {
        tracing::debug!(
            "Could not resolve provided paths, trying to resolve current working directory"
        );
        paths.push(cwd.clone());
    }

    let options = args.into_options();

    let project = Project::new(cwd, paths)
        .with_metadata(ProjectMetadata::new(current_python_version()))
        .with_options(options);

    ctrlc::set_handler(move || {
        std::process::exit(0);
    })?;

    let mut reporter: Box<dyn Reporter> =
        if project.options().verbosity().is_default() && !project.options().show_output() {
            Box::new(ProgressReporter::default())
        } else {
            Box::new(DummyReporter)
        };

    let result = project.test_with_reporter(&mut *reporter);

    let mut stdout = io::stdout().lock();

    let passed = result.passed();

    for diagnostic in result.iter() {
        write!(stdout, "{}", diagnostic.display())?;
        writeln!(stdout)?;
    }

    write!(stdout, "{}", result.display())?;

    if passed {
        writeln!(stdout, "{}", "All checks passed!".green().bold())?;

        return Ok(ExitStatus::Success);
    }

    Ok(ExitStatus::Failure)
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

impl ExitStatus {
    #[must_use]
    pub const fn to_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Default)]
struct ProgressReporter(Option<indicatif::ProgressBar>);

impl karva_core::diagnostic::reporter::Reporter for ProgressReporter {
    fn set(&mut self, n: usize) {
        let progress = indicatif::ProgressBar::new(n as u64);
        progress.set_style(
            indicatif::ProgressStyle::with_template(
                r"{msg:10.dim} {bar:60.green/dim} {pos}/{len} tests",
            )
            .unwrap()
            .progress_chars("--"),
        );
        progress.set_message("Testing");

        self.0 = Some(progress);
    }

    fn report(&self) {
        if let Some(ref progress_bar) = self.0 {
            progress_bar.inc(1);
        }
    }
}
