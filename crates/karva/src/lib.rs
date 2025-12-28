use std::ffi::OsString;
use std::fmt::Write;
use std::io::{self};
use std::process::{ExitCode, Termination};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::Parser;
use colored::Colorize;
use karva_logging::{Printer, VerbosityLevel, set_colored_override, setup_tracing};
use karva_metadata::{ProjectMetadata, ProjectOptionsOverrides};
use karva_project::{Db, ProjectDatabase};
use karva_system::{OsSystem, System, path::absolute};

use karva_cli::{Args, Command, TestCommand};

mod version;

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
    let mut stdout = Printer::default().stream_for_requested_summary().lock();
    if let Some(version_info) = crate::version::version() {
        writeln!(stdout, "karva {}", &version_info)?;
    } else {
        writeln!(stdout, "Failed to get karva version")?;
    }

    Ok(())
}

pub(crate) fn test(args: TestCommand) -> Result<ExitStatus> {
    let verbosity = args.verbosity().level();

    set_colored_override(args.sub_command.color);

    let printer = Printer::new(verbosity, args.sub_command.no_progress.unwrap_or(false));

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

    tracing::debug!(cwd = %cwd, "Working directory");

    let system = OsSystem::new(&cwd);

    let config_file = args
        .sub_command
        .config_file
        .as_ref()
        .map(|path| absolute(path, &cwd));

    let mut project_metadata = if let Some(config_file) = &config_file {
        ProjectMetadata::from_config_file(config_file.clone(), &system)?
    } else {
        ProjectMetadata::discover(system.current_directory(), &system)?
    };

    let sub_command = args.sub_command.clone();

    let no_parallel = args.no_parallel.unwrap_or(false);
    let num_workers = args.num_workers;

    let project_options_overrides = ProjectOptionsOverrides::new(config_file, args.into_options());
    project_metadata.apply_overrides(&project_options_overrides);

    let mut db = ProjectDatabase::new(project_metadata, system)?;

    db.project_mut()
        .set_verbose(verbosity >= VerbosityLevel::Verbose);

    // Listen to Ctrl+C and abort
    ctrlc::set_handler(move || {
        std::process::exit(0);
    })?;

    let num_workers = if no_parallel {
        1
    } else {
        num_workers.unwrap_or_else(|| karva_system::max_parallelism().get())
    };

    let config = karva_runner::ParallelTestConfig { num_workers };

    let result = karva_runner::run_parallel_tests(&db, &config, &sub_command, printer)?;

    if result {
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
