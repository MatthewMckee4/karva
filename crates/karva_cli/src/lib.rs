use std::{
    ffi::OsString,
    fmt::Write,
    io::{self},
    process::{ExitCode, Termination},
    time::Instant,
};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::Parser;
use colored::Colorize;
use karva_core::{
    DummyReporter, Printer, Reporter, TestCaseReporter, TestRunResult, TestRunner,
    utils::current_python_version,
};
use karva_project::{
    Db, OsSystem, ProjectDatabase, ProjectMetadata, ProjectOptionsOverrides, System,
    VerbosityLevel, absolute,
};
use ruff_db::diagnostic::{DiagnosticFormat, DisplayDiagnosticConfig, DisplayDiagnostics};

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
    let mut stdout = Printer::default().stream_for_requested_summary().lock();
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

    let printer = Printer::new(verbosity, args.no_progress.unwrap_or(false));

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

    let python_version = current_python_version();

    let system = OsSystem::new(&cwd);

    let config_file = args.config_file.as_ref().map(|path| absolute(path, &cwd));

    let mut project_metadata = match &config_file {
        Some(config_file) => {
            ProjectMetadata::from_config_file(config_file.clone(), &system, python_version)?
        }
        None => ProjectMetadata::discover(system.current_directory(), &system, python_version)?,
    };

    let project_options_overrides = ProjectOptionsOverrides::new(config_file, args.into_options());
    project_metadata.apply_overrides(&project_options_overrides);

    let mut db = ProjectDatabase::new(project_metadata, system)?;

    db.project_mut()
        .set_verbose(verbosity >= VerbosityLevel::Verbose);

    // Listen to Ctrl+C and abort
    ctrlc::set_handler(move || {
        std::process::exit(0);
    })?;

    let reporter: Box<dyn Reporter> = if verbosity.is_quiet() {
        Box::new(DummyReporter)
    } else {
        Box::new(TestCaseReporter::new(printer))
    };

    let start_time = Instant::now();

    let result = db.test_with_reporter(&*reporter);

    display_test_output(&db, &result, printer, start_time)
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

fn display_test_output(
    db: &dyn Db,
    result: &TestRunResult,
    printer: Printer,
    start_time: Instant,
) -> Result<ExitStatus> {
    let discovery_diagnostics = result.discovery_diagnostics();

    let diagnostics = result.diagnostics();

    let mut stdout = printer.stream_for_details().lock();

    let diagnostic_format = db.project().settings().terminal().output_format.into();

    let config = DisplayDiagnosticConfig::default()
        .format(diagnostic_format)
        .color(colored::control::SHOULD_COLORIZE.should_colorize());

    let is_concise = matches!(diagnostic_format, DiagnosticFormat::Concise);

    if (!diagnostics.is_empty() || !discovery_diagnostics.is_empty())
        && result.stats().total() > 0
        && stdout.is_enabled()
    {
        writeln!(stdout)?;
    }

    if !discovery_diagnostics.is_empty() && stdout.is_enabled() {
        writeln!(stdout, "discovery diagnostics:")?;
        writeln!(stdout)?;
        write!(
            stdout,
            "{}",
            DisplayDiagnostics::new(db, &config, discovery_diagnostics)
        )?;

        if is_concise {
            writeln!(stdout)?;
        }
    }

    if !diagnostics.is_empty() && stdout.is_enabled() {
        writeln!(stdout, "diagnostics:")?;
        writeln!(stdout)?;
        write!(
            stdout,
            "{}",
            DisplayDiagnostics::new(db, &config, diagnostics)
        )?;

        if is_concise {
            writeln!(stdout)?;
        }
    }

    if (diagnostics.is_empty() && discovery_diagnostics.is_empty())
        && result.stats().total() > 0
        && stdout.is_enabled()
    {
        writeln!(stdout)?;
    }

    let mut result_stdout = printer.stream_for_failure_summary().lock();

    write!(result_stdout, "{}", result.stats().display(start_time))?;

    if result.is_success() {
        Ok(ExitStatus::Success)
    } else {
        Ok(ExitStatus::Failure)
    }
}
