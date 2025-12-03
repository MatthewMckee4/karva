use std::{
    ffi::OsString,
    io::{self, BufWriter, Write},
    process::{ExitCode, Termination},
};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::Parser;
use colored::Colorize;
use karva_core::{
    DummyReporter, Reporter, TestCaseReporter, TestRunner, utils::current_python_version,
};
use karva_project::{Project, ProjectMetadata, absolute};

use crate::{
    args::{Command, TerminalColor, TestCommand},
    logging::setup_tracing,
};

mod args;
mod logging;
mod version;

pub use args::Args;

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
    if let Some(version_info) = crate::version::version() {
        writeln!(stdout, "karva {}", &version_info)?;
    } else {
        writeln!(stdout, "Failed to get karva version")?;
    }

    Ok(())
}

pub(crate) fn test(args: TestCommand) -> Result<ExitStatus> {
    let verbosity = args.verbosity.level();

    set_colored_override(args.color);

    let _guard = setup_tracing(verbosity);

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

    let mut paths: Vec<_> = args.paths.iter().map(|path| absolute(path, &cwd)).collect();

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

    let reporter: Box<dyn Reporter> = if verbosity.is_quiet() || project.options().no_progress() {
        Box::new(DummyReporter)
    } else {
        Box::new(TestCaseReporter::default())
    };

    let result = project.test_with_reporter(&*reporter);

    let mut stdout = io::stdout().lock();

    if result.stats().total() > 0 {
        writeln!(stdout)?;
    }

    write!(stdout, "{}", result.display())?;

    if result.stats().is_success() {
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

impl ExitStatus {
    pub const fn to_i32(self) -> i32 {
        self as i32
    }
}

fn set_colored_override(color: Option<TerminalColor>) {
    let Some(color) = color else {
        return;
    };

    match color {
        TerminalColor::Auto => {
            colored::control::unset_override();
        }
        TerminalColor::Always => {
            colored::control::set_override(true);
        }
        TerminalColor::Never => {
            colored::control::set_override(false);
        }
    }
}
