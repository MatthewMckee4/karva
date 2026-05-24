use std::ffi::OsString;
use std::io;
use std::process::{ExitCode, Termination};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use clap::Parser;
use colored::Colorize;
use karva_cache::{RunCache, RunHash};
use karva_cli::{SubTestCommand, Verbosity};
use karva_diagnostic::{
    DummyReporter, FileLineSink, LineSink, ProgressTrackingReporter, Reporter, TestCaseReporter,
};
use karva_logging::{Printer, StatusLevel, set_colored_override, setup_tracing};
use karva_metadata::RunIgnoredMode;
use karva_metadata::filter::FiltersetSet;
use karva_project::path::{TestPath, TestPathError, absolute};
use karva_python_semantic::current_python_version;
use karva_static::EnvVars;
use ruff_db::diagnostic::{DisplayDiagnosticConfig, FileResolver, Input, UnifiedFile};
use ruff_db::files::File;
use ruff_notebook::NotebookIndex;

/// Command-line arguments for the `karva_worker` process.
///
/// This struct is used internally when tests are distributed across
/// multiple worker processes for parallel execution.
#[derive(Parser)]
#[command(name = "karva_worker", about = "Karva test worker")]
struct Args {
    /// Directory where test results and duration cache are stored.
    #[arg(long)]
    cache_dir: Utf8PathBuf,

    /// Unique identifier for this test run, used for cache coordination.
    /// Encodes `<ms>-<uuid>`; the cache directory adds the `run-` prefix.
    #[arg(long)]
    run_id: String,

    /// Numeric identifier for this worker in a parallel test run.
    #[arg(long)]
    worker_id: usize,

    /// Shared test execution options inherited from the main CLI.
    #[clap(flatten)]
    sub_command: SubTestCommand,
}

impl Args {
    pub fn verbosity(&self) -> &Verbosity {
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
    pub fn to_i32(self) -> i32 {
        self as i32
    }
}
pub fn karva_worker_main(f: impl FnOnce(Vec<OsString>) -> Vec<OsString>) -> ExitStatus {
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

    // SAFETY: This is called during single-threaded initialization before any
    // concurrent work begins. The env var is read later by `assert_snapshot`.
    if args.sub_command.snapshot_update.unwrap_or(false) {
        unsafe {
            std::env::set_var(EnvVars::KARVA_SNAPSHOT_UPDATE, "1");
        }
    }

    let verbosity = args.verbosity().level();

    set_colored_override(args.sub_command.color);

    let printer = Printer::new(
        args.sub_command.status_level.unwrap_or_default(),
        args.sub_command.final_status_level.unwrap_or_default(),
    );

    let _guard = setup_tracing(verbosity);

    let cwd = cwd()?;

    let python_version = current_python_version();

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

    let filter = FiltersetSet::new(&args.sub_command.filter_expressions)
        .context("invalid `--filter` expression")?;

    let run_ignored = args
        .sub_command
        .run_ignored
        .map(RunIgnoredMode::from)
        .unwrap_or_default();

    let coverage = match (
        args.sub_command.cov.is_empty(),
        args.sub_command.cov_data_file.clone(),
    ) {
        (false, Some(data_file)) => Some(karva_test_semantic::CoverageConfig {
            sources: args.sub_command.cov.clone(),
            data_file,
        }),
        _ => None,
    };

    let mut settings = args.sub_command.into_options().to_settings();
    settings.set_filter(filter);
    settings.set_run_ignored(run_ignored);

    let run_hash = RunHash::from_existing(&args.run_id);

    let cache = RunCache::new(&args.cache_dir, &run_hash);

    let worker_dir = cache.worker_dir(args.worker_id);
    std::fs::create_dir_all(&worker_dir)
        .with_context(|| format!("Failed to create worker dir at {worker_dir}"))?;
    let progress_file = cache.progress_file(args.worker_id).into_std_path_buf();
    let reporter: Box<dyn Reporter> = if matches!(printer.status_level(), StatusLevel::None) {
        Box::new(ProgressTrackingReporter::new(DummyReporter, progress_file))
    } else {
        let output_path = cache.output_file(args.worker_id);
        let sink: Box<dyn LineSink> = Box::new(
            FileLineSink::open(output_path.as_std_path())
                .with_context(|| format!("Failed to open worker output file at {output_path}"))?,
        );
        Box::new(ProgressTrackingReporter::new(
            TestCaseReporter::new(printer, sink),
            progress_file,
        ))
    };

    let result = karva_test_semantic::run_tests(
        &cwd,
        &settings,
        python_version,
        reporter.as_ref(),
        test_paths,
        coverage.as_ref(),
    );

    let diagnostic_format = settings.terminal().output_format.into();

    let config = DisplayDiagnosticConfig::new("karva")
        .format(diagnostic_format)
        .color(colored::control::SHOULD_COLORIZE.should_colorize())
        .context(0);

    let diagnostic_resolver = DiagnosticFileResolver::new(&cwd);

    cache.write_result(args.worker_id, &result, &diagnostic_resolver, &config)?;

    // Propagate the stop signal to sibling workers whenever this worker has
    // reached (or exceeded) its configured max-fail budget. The budget is
    // enforced locally per worker inside `PackageRunner`, so hitting any
    // failure here while `max_fail` is set means we ran out of budget.
    let failed_count = u32::try_from(result.stats().failed()).unwrap_or(u32::MAX);
    if settings.max_fail().is_exceeded_by(failed_count) {
        cache.write_fail_fast_signal()?;
    }

    Ok(ExitStatus::Success)
}

/// Resolves file paths for diagnostic messages.
///
/// Implements the `FileResolver` trait to provide file path information
/// when rendering diagnostic error messages to the user.
struct DiagnosticFileResolver<'a> {
    cwd: &'a Utf8PathBuf,
}

impl<'a> DiagnosticFileResolver<'a> {
    fn new(cwd: &'a Utf8PathBuf) -> Self {
        Self { cwd }
    }
}

impl FileResolver for DiagnosticFileResolver<'_> {
    fn path(&self, _file: File) -> &str {
        unimplemented!("karva does not resolve file paths via ruff_db");
    }

    fn input(&self, _file: File) -> Input {
        unimplemented!("karva does not resolve file inputs via ruff_db");
    }

    fn notebook_index(&self, _file: &UnifiedFile) -> Option<NotebookIndex> {
        None
    }

    fn is_notebook(&self, _file: &UnifiedFile) -> bool {
        false
    }

    fn current_directory(&self) -> &std::path::Path {
        self.cwd.as_std_path()
    }
}

/// Get the current working directory as a UTF-8 path.
fn cwd() -> anyhow::Result<Utf8PathBuf> {
    let cwd = std::env::current_dir().context("Failed to get the current working directory")?;
    Utf8PathBuf::from_path_buf(cwd).map_err(|path| {
        anyhow::anyhow!(
            "The current working directory `{}` contains non-Unicode characters. karva only supports Unicode paths.",
            path.display()
        )
    })
}
