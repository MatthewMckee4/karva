use std::net::SocketAddr;
use std::{ffi::OsString, io};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use clap::Parser;
use colored::Colorize;
use karva_cli::{ExitStatus, SubTestCommand, Verbosity};
use karva_diagnostic::{DiagnosticFormat, DisplayDiagnosticConfig, TestCaseReporter};
use karva_ipc::WorkerClient;
use karva_logging::{Printer, set_colored_override, setup_tracing};
use karva_metadata::filter::FiltersetSet;
use karva_metadata::{OutputFormat, RunIgnoredMode};
use karva_project::path::{TestPath, TestPathError, absolute};
use karva_python_semantic::current_python_version;
use karva_static::EnvVars;

use crate::reporter::WorkerReporter;

/// Command-line arguments for the `karva_worker` process.
///
/// This struct is used internally when tests are distributed across
/// multiple worker processes for parallel execution.
#[derive(Parser)]
#[command(name = "karva_worker", about = "Karva test worker")]
struct Args {
    /// Controller endpoint used for runtime events and final results.
    #[arg(long)]
    controller_address: SocketAddr,

    /// Unique identifier correlating events for this test run.
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
    /// Returns verbosity inherited from the controller invocation.
    pub fn verbosity(&self) -> &Verbosity {
        &self.sub_command.verbosity
    }
}

/// Runs one worker invocation, translating broken pipes into successful exits.
pub fn karva_worker_main(f: impl FnOnce(Vec<OsString>) -> Vec<OsString>) -> ExitStatus {
    run(f).unwrap_or_else(|error| {
        use io::Write;

        // Exit "gracefully" on broken pipe errors.
        //
        // See: https://github.com/BurntSushi/ripgrep/blob/bf63fe8f258afc09bae6caa48f0ae35eaf115005/crates/core/main.rs#L47C1-L61C14
        if error.chain().any(|cause| {
            cause
                .downcast_ref::<io::Error>()
                .is_some_and(|ioerr| ioerr.kind() == io::ErrorKind::BrokenPipe)
        }) {
            return ExitStatus::Success;
        }

        // Use `writeln` instead of `eprintln` to avoid panicking when the stderr pipe is broken.
        let mut stderr = io::stderr().lock();

        // This communicates that this isn't a linter error but karva itself hard-errored for
        // some reason (e.g. failed to resolve the configuration)
        writeln!(stderr, "{}", "karva failed".red().bold()).ok();
        // Currently we generally only see one error, but e.g. with io errors when resolving
        // the configuration it is help to chain errors ("resolving configuration failed" ->
        // "failed to read file: subdir/pyproject.toml")
        for cause in error.chain() {
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

    if args.sub_command.snapshot_update.unwrap_or(false) {
        enable_snapshot_update_env_var();
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

    let test_paths: Vec<Result<TestPath, TestPathError>> = args
        .sub_command
        .paths
        .iter()
        .map(|path| {
            let path = absolute(path, &cwd);
            TestPath::new(path.as_str())
        })
        .collect();

    let filter = FiltersetSet::new(&args.sub_command.filter_expressions)
        .context("invalid `--filter` expression")?;

    let run_ignored = args
        .sub_command
        .run_ignored
        .map(RunIgnoredMode::from)
        .unwrap_or_default();

    let coverage = worker_coverage_config(&args.sub_command)?;

    let registered_tags = args.sub_command.registered_tag.clone();
    let mut settings = args.sub_command.into_options().to_settings().with_tags(
        registered_tags
            .into_iter()
            .map(|name| (name, String::new()))
            .collect(),
    );
    settings.set_filter(filter);
    settings.set_run_ignored(run_ignored);

    let diagnostic_format = match settings.terminal().output_format {
        OutputFormat::Full => DiagnosticFormat::Full,
        OutputFormat::Concise => DiagnosticFormat::Concise,
    };
    let diagnostic_config = DisplayDiagnosticConfig::new(
        diagnostic_format,
        colored::control::SHOULD_COLORIZE.should_colorize(),
    );
    let client = WorkerClient::connect(args.controller_address, &args.run_id, args.worker_id)?;
    let reporter = WorkerReporter::new(
        TestCaseReporter::new(printer),
        client.clone(),
        cwd.clone(),
        diagnostic_config,
    );

    drop(karva_test_semantic::run_tests(
        &cwd,
        &settings,
        python_version,
        &reporter,
        test_paths,
        coverage.as_ref(),
        !verbosity.is_default(),
    ));
    reporter.finish()?;
    drop(reporter);
    client.complete()?;

    Ok(ExitStatus::Success)
}

#[expect(
    unsafe_code,
    reason = "worker startup sets this before concurrent test execution"
)]
fn enable_snapshot_update_env_var() {
    // SAFETY: This is called during single-threaded initialization before any
    // concurrent work begins. The env var is read later by `assert_snapshot`.
    unsafe {
        std::env::set_var(EnvVars::KARVA_SNAPSHOT_UPDATE, "1");
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

/// Builds worker coverage settings and rejects incomplete controller arguments.
fn worker_coverage_config(
    sub_command: &SubTestCommand,
) -> anyhow::Result<Option<karva_test_semantic::CoverageConfig>> {
    if sub_command.cov.is_empty() {
        return Ok(None);
    }

    let Some(data_file) = sub_command.cov_data_file.clone() else {
        anyhow::bail!("karva-worker requires `--cov-data-file` when `--cov` is set");
    };

    Ok(Some(karva_test_semantic::CoverageConfig {
        sources: sub_command.cov.clone(),
        data_file,
        contexts: sub_command.cov_context == Some(karva_cli::CovContext::Test),
        static_context: sub_command.cov_static_context.clone(),
        branches: sub_command.cov_branch,
        exclude_lines: sub_command.cov_exclude_line.clone(),
        partial_branches: sub_command.cov_partial_branch.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use karva_cli::SubTestCommand;

    use super::worker_coverage_config;

    #[test]
    fn coverage_config_is_absent_without_sources() {
        let sub_command = SubTestCommand::default();

        let coverage = worker_coverage_config(&sub_command).expect("coverage config");

        assert!(coverage.is_none());
    }

    #[test]
    fn coverage_config_requires_data_file_when_sources_are_set() {
        let sub_command = SubTestCommand {
            cov: vec!["src".to_string()],
            ..SubTestCommand::default()
        };

        let err = worker_coverage_config(&sub_command)
            .expect_err("missing worker coverage data file should be rejected");

        assert_eq!(
            err.to_string(),
            "karva-worker requires `--cov-data-file` when `--cov` is set"
        );
    }

    #[test]
    fn coverage_config_preserves_sources_and_data_file() {
        let data_file = Utf8PathBuf::from(".coverage.worker-0");
        let sub_command = SubTestCommand {
            cov: vec![String::new(), "pkg".to_string()],
            cov_data_file: Some(data_file.clone()),
            ..SubTestCommand::default()
        };

        let coverage = worker_coverage_config(&sub_command)
            .expect("coverage config")
            .expect("coverage should be enabled");

        assert_eq!(coverage.sources, vec![String::new(), "pkg".to_string()]);
        assert_eq!(coverage.data_file, data_file);
        assert!(!coverage.contexts);
        assert!(!coverage.branches);
    }

    #[test]
    fn coverage_config_preserves_context_mode() {
        let data_file = Utf8PathBuf::from(".coverage.worker-0");
        let sub_command = SubTestCommand {
            cov: vec!["pkg".to_string()],
            cov_context: Some(karva_cli::CovContext::Test),
            cov_data_file: Some(data_file),
            ..SubTestCommand::default()
        };

        let coverage = worker_coverage_config(&sub_command)
            .expect("coverage config")
            .expect("coverage should be enabled");

        assert!(coverage.contexts);
    }

    #[test]
    fn coverage_config_preserves_branch_mode() {
        let data_file = Utf8PathBuf::from(".coverage.worker-0");
        let sub_command = SubTestCommand {
            cov: vec!["pkg".to_string()],
            cov_branch: true,
            cov_data_file: Some(data_file),
            ..SubTestCommand::default()
        };

        let coverage = worker_coverage_config(&sub_command)
            .expect("coverage config")
            .expect("coverage should be enabled");

        assert!(coverage.branches);
    }
}
