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
use karva_python_semantic::current_python_version;
use karva_system::System;
use karva_system::path::{TestPath, TestPathError};
use karva_system::{OsSystem, path::absolute};
use ruff_db::diagnostic::{DisplayDiagnosticConfig, FileResolver, Input, UnifiedFile};
use ruff_db::files::File;
use ruff_notebook::NotebookIndex;

use crate::Context;
use crate::discovery::StandardDiscoverer;
use crate::runner::PackageRunner;
use crate::utils::attach_with_project;

/// Command-line arguments for the `karva_core` worker process.
///
/// This struct is used internally when tests are distributed across
/// multiple worker processes for parallel execution.
#[derive(Parser)]
#[command(name = "karva_core", about = "Karva test worker")]
struct Args {
    /// Directory where test results and duration cache are stored.
    #[arg(long)]
    cache_dir: Utf8PathBuf,

    /// Unique identifier for this test run, used for cache coordination.
    #[arg(long)]
    run_hash: String,

    /// Numeric identifier for this worker in a parallel test run.
    #[arg(long)]
    worker_id: usize,

    /// Shared test execution options inherited from the main CLI.
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

    let python_version = current_python_version();

    let system = OsSystem::new(&cwd);

    let test_paths: Vec<Utf8PathBuf> = args
        .sub_command
        .paths
        .iter()
        .map(|p| absolute(p, cwd.clone()))
        .collect();

    let test_paths: Vec<Result<TestPath, TestPathError>> = test_paths
        .iter()
        .map(|p| TestPath::new(p.as_str()))
        .collect();

    let settings = args.sub_command.into_options().to_settings();

    let run_hash = RunHash::from_existing(&args.run_hash);

    let cache_writer = CacheWriter::new(&args.cache_dir, &run_hash, args.worker_id)?;

    let reporter: Box<dyn Reporter> = if verbosity.is_quiet() {
        Box::new(DummyReporter)
    } else {
        Box::new(TestCaseReporter::new(printer))
    };

    let context = Context::new(&system, &settings, python_version, reporter.as_ref());

    let result = attach_with_project(settings.terminal().show_python_output, |py| {
        let session = StandardDiscoverer::new(&context).discover_with_py(py, test_paths);

        PackageRunner::new(&context).execute(py, &session);

        context.into_result()
    });

    let diagnostic_format = settings.terminal().output_format.into();

    let config = DisplayDiagnosticConfig::default()
        .format(diagnostic_format)
        .color(colored::control::SHOULD_COLORIZE.should_colorize());

    let diagnostic_resolver = DiagnosticFileResolver::new(&system);

    cache_writer.write_result(&result, &diagnostic_resolver, &config)?;

    Ok(ExitStatus::Success)
}

/// Resolves file paths for diagnostic messages.
///
/// Implements the `FileResolver` trait to provide file path information
/// when rendering diagnostic error messages to the user.
struct DiagnosticFileResolver<'a> {
    /// Reference to the system abstraction for file operations.
    system: &'a dyn System,
}

impl<'a> DiagnosticFileResolver<'a> {
    fn new(system: &'a dyn System) -> Self {
        Self { system }
    }
}

impl FileResolver for DiagnosticFileResolver<'_> {
    fn path(&self, _file: File) -> &str {
        unimplemented!("Expected a Ruff file for rendering a Ruff diagnostic");
    }

    fn input(&self, _file: File) -> Input {
        unimplemented!("Expected a Ruff file for rendering a Ruff diagnostic");
    }

    fn notebook_index(&self, _file: &UnifiedFile) -> Option<NotebookIndex> {
        None
    }

    fn is_notebook(&self, _file: &UnifiedFile) -> bool {
        false
    }

    fn current_directory(&self) -> &std::path::Path {
        self.system.current_directory().as_std_path()
    }
}
