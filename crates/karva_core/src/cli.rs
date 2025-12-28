use std::ffi::OsString;
use std::io;
use std::process::{ExitCode, Termination};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use clap::Parser;
use colored::Colorize;
use karva_cache::{CacheWriter, RunHash};
use karva_cli::SubTestCommand;
use karva_metadata::{ProjectMetadata, ProjectOptionsOverrides};
use karva_project::ProjectDatabase;
use karva_system::{OsSystem, System, absolute};

use crate::executor::execute_test_paths;
use crate::utils::current_python_version;

#[derive(Parser)]
#[command(name = "karva_core", about = "Karva test worker")]
struct Args {
    /// Test paths to execute (e.g., "tests.test_foo::test_example")
    #[arg(long)]
    test_paths: Vec<String>,

    /// Project root directory
    #[arg(long)]
    project_root: Utf8PathBuf,

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
    // Set up tracing subscriber for worker process
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("karva_core=debug,karva_cache=debug"))
        )
        .with_writer(std::io::stderr)
        .init();

    let args = wild::args_os();

    let args = f(
        argfile::expand_args_from(args, argfile::parse_fromfile, argfile::PREFIX)
            .context("Failed to read CLI arguments from file")?,
    );

    let args = Args::parse_from(args);

    tracing::info!(
        worker_id = args.worker_id,
        test_count = args.test_paths.len(),
        run_hash = %args.run_hash,
        "Worker process started"
    );

    // Initialize project database (similar to karva crate setup)
    let cwd = args.project_root.clone();
    tracing::debug!(project_root = %cwd, "Setting project root");

    let python_version = current_python_version();
    tracing::debug!(version = %python_version, "Detected Python version");

    let system = OsSystem::new(&cwd);

    let config_file = args
        .sub_command
        .config_file
        .as_ref()
        .map(|path| absolute(path, &cwd));

    let mut project_metadata = match &config_file {
        Some(config_file) => {
            tracing::debug!(config_file = %config_file, "Loading project metadata from config file");
            ProjectMetadata::from_config_file(config_file.clone(), &system, python_version)?
        }
        None => {
            tracing::debug!("Discovering project metadata");
            ProjectMetadata::discover(system.current_directory(), &system, python_version)?
        }
    };

    // Apply any project option overrides
    let project_options_overrides = ProjectOptionsOverrides::default();
    project_metadata.apply_overrides(&project_options_overrides);

    tracing::debug!("Initializing project database");
    let db = ProjectDatabase::new(project_metadata, system)
        .context("Failed to initialize project database")?;

    // Initialize cache writer
    let run_hash = RunHash(args.run_hash.clone());
    tracing::debug!(
        cache_dir = %args.cache_dir,
        worker_id = args.worker_id,
        run_hash = %args.run_hash,
        "Initializing cache writer"
    );
    let cache_writer = CacheWriter::new(args.cache_dir, run_hash, args.worker_id)?;

    // Execute tests
    tracing::debug!("Starting test execution");
    let exit_code = execute_test_paths(
        &db,
        &args.test_paths,
        &cache_writer,
        args.sub_command.fail_fast.unwrap_or(false),
        args.sub_command.show_output.unwrap_or(false),
    )?;

    tracing::info!(exit_code = exit_code, "Worker process exiting");
    std::process::exit(exit_code);
}
