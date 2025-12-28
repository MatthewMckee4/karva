use std::ffi::OsString;
use std::io;
use std::process::{ExitCode, Termination};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use clap::Parser;
use colored::Colorize;
use karva_cache::{CacheWriter, RunHash};
use karva_cli::{SubTestCommand, Verbosity};
use karva_diagnostic::{DummyReporter, Reporter, TestCaseReporter};
use karva_logging::{Printer, set_colored_override, setup_tracing};
use karva_metadata::{ProjectMetadata, ProjectOptionsOverrides};
use karva_project::{Db, ProjectDatabase};
use karva_system::{OsSystem, System, path::absolute};
use ruff_db::diagnostic::DisplayDiagnosticConfig;

use crate::runner::StandardTestRunner;
use crate::utils::current_python_version;

#[derive(Parser)]
#[command(name = "karva_core", about = "Karva test worker")]
struct Args {
    /// Cache directory
    #[arg(long)]
    cache_dir: Utf8PathBuf,

    /// Run hash
    #[arg(long)]
    run_hash: String,

    /// Worker ID
    #[arg(long)]
    worker_id: usize,

    /// Shared test command options
    #[clap(flatten)]
    sub_command: SubTestCommand,
}

impl Args {
    pub const fn verbosity(&self) -> &Verbosity {
        &self.sub_command.verbosity
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
pub fn karva_core_main(f: impl FnOnce(Vec<OsString>) -> Vec<OsString>) -> ExitStatus {
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

    let python_version = current_python_version();

    tracing::debug!(version = %python_version, "Detected Python version");

    let system = OsSystem::new(&cwd);

    let config_file = args
        .sub_command
        .config_file
        .as_ref()
        .map(|path| absolute(path, &cwd));

    let mut project_metadata = if let Some(config_file) = &config_file {
        ProjectMetadata::from_config_file(config_file.clone(), &system, python_version)?
    } else {
        ProjectMetadata::discover(system.current_directory(), &system, python_version)?
    };

    // We have already checked the include paths in the main worker.
    if let Some(src) = project_metadata.options.src.as_mut() {
        src.include = None;
    }

    let project_options_overrides =
        ProjectOptionsOverrides::new(config_file, args.sub_command.into_options());
    project_metadata.apply_overrides(&project_options_overrides);

    let db = ProjectDatabase::new(project_metadata, system)?;

    let run_hash = RunHash::from_existing(&args.run_hash);

    let cache_writer = CacheWriter::new(&args.cache_dir, &run_hash, args.worker_id)?;

    let reporter: Box<dyn Reporter> = if verbosity.is_quiet() {
        Box::new(DummyReporter)
    } else {
        Box::new(TestCaseReporter::new(printer))
    };

    let exit_code = execute_test_paths(&db, &cache_writer, reporter.as_ref())?;

    std::process::exit(exit_code);
}

pub fn execute_test_paths(
    db: &ProjectDatabase,
    cache_writer: &CacheWriter,
    reporter: &dyn Reporter,
) -> anyhow::Result<i32> {
    let test_runner = StandardTestRunner::new(db);
    let result = test_runner.test_with_reporter(reporter);

    let diagnostic_format = db.project().settings().terminal().output_format.into();
    let config = DisplayDiagnosticConfig::default()
        .format(diagnostic_format)
        .color(false);

    cache_writer.write_result(&result, db, &config)?;

    Ok(0)
}
