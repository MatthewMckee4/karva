//! Worker startup from process bootstrap identity and authenticated IPC configuration.

use std::collections::{BTreeMap, BTreeSet};
use std::{env, io};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use colored::Colorize;
use karva_cli::ExitStatus;
use karva_diagnostic::{
    DiagnosticFormat, DisplayDiagnosticConfig, TestCaseReporter, render_diagnostic,
};
use karva_ipc::{ControllerEndpoint, WorkerClient, WorkerConfiguration, WorkerEvent};
use karva_logging::{Printer, set_colored_override, setup_tracing};
use karva_metadata::filter::FiltersetSet;
use karva_metadata::{OutputFormat, ProjectSettings};
use karva_project::path::{TestPath, TestPathError};
use karva_python_semantic::{current_python_version, enable_faulthandler};
use karva_static::WorkerEnvVars;

use crate::reporter::WorkerReporter;

/// Runs one worker invocation, translating broken pipes into successful exits.
pub fn karva_worker_main() -> ExitStatus {
    run().unwrap_or_else(|error| {
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

fn run() -> anyhow::Result<ExitStatus> {
    let endpoint = env::var_os(WorkerEnvVars::KARVA_CONTROLLER_ENDPOINT)
        .context("karva-worker requires KARVA_CONTROLLER_ENDPOINT from its controller")?;
    let controller_endpoint = ControllerEndpoint::decode(&endpoint).map_err(anyhow::Error::msg)?;
    let run_id = env::var(WorkerEnvVars::KARVA_RUN_ID)
        .context("karva-worker requires a Unicode KARVA_RUN_ID from its controller")?;
    let worker_id = env::var(WorkerEnvVars::KARVA_WORKER_ID)
        .context("karva-worker requires KARVA_WORKER_ID from its controller")?
        .parse()
        .context("karva-worker requires KARVA_WORKER_ID to be an unsigned integer")?;
    let (client, selection) = WorkerClient::connect(&controller_endpoint, &run_id, worker_id)?;
    let configuration = selection.configuration;
    let verbosity = configuration.verbosity;
    set_colored_override(configuration.color);
    let _guard = setup_tracing(verbosity);
    let cwd = cwd()?;
    let python_version = current_python_version();
    enable_faulthandler().context("Failed to enable Python faulthandler")?;
    let filter = FiltersetSet::new(&configuration.filter_expressions)
        .context("invalid worker test filter expression")?;
    let mut settings = configuration
        .options
        .to_settings()
        .with_tags(configuration.tags.clone());
    settings.set_filter(filter);
    settings.set_run_ignored(configuration.run_ignored);
    let printer = Printer::new(
        settings.terminal().status_level,
        settings.terminal().final_status_level,
    );
    let coverage = worker_coverage_config(&settings, &configuration, selection.coverage_data_file)?;
    drop(configuration);

    let diagnostic_format = match settings.terminal().output_format {
        OutputFormat::Full => DiagnosticFormat::Full,
        OutputFormat::Concise => DiagnosticFormat::Concise,
    };
    let diagnostic_config = DisplayDiagnosticConfig::new(
        diagnostic_format,
        colored::control::SHOULD_COLORIZE.should_colorize(),
    );
    let resume_skip = selection.resume_skip.into_iter().collect::<BTreeSet<_>>();
    let resume_attempts = selection
        .resume_attempts
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    // Controller selectors come from collected absolute module paths.
    let test_paths: Vec<Result<TestPath, TestPathError>> = selection
        .test_paths
        .into_iter()
        .map(|path| TestPath::new(path.as_ref()))
        .collect();
    let reporter = WorkerReporter::new(
        TestCaseReporter::new(printer),
        client.clone(),
        cwd.clone(),
        diagnostic_config,
    );

    let result = karva_test_semantic::run_tests(karva_test_semantic::RunRequest {
        cwd: &cwd,
        settings: &settings,
        python_version,
        reporter: &reporter,
        test_paths,
        resume_skip: &resume_skip,
        resume_attempts: &resume_attempts,
        coverage: coverage.as_ref(),
        verbose: !verbosity.is_default(),
    });
    reporter.finish()?;
    drop(reporter);
    for diagnostic in &result {
        client.send_event(WorkerEvent::RunDiagnostic(render_diagnostic(
            diagnostic,
            &cwd,
            diagnostic_config,
        )))?;
    }
    drop(result);
    client.complete()?;

    Ok(ExitStatus::Success)
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

/// Builds worker coverage settings and rejects incomplete controller assignments.
fn worker_coverage_config(
    settings: &ProjectSettings,
    configuration: &WorkerConfiguration,
    data_file: Option<Utf8PathBuf>,
) -> anyhow::Result<Option<karva_test_semantic::CoverageConfig>> {
    let coverage = settings.coverage();
    if coverage.sources.is_empty() {
        return Ok(None);
    }

    let Some(data_file) = data_file else {
        anyhow::bail!(
            "worker assignment requires a coverage data file when coverage sources are configured"
        );
    };

    Ok(Some(karva_test_semantic::CoverageConfig {
        sources: coverage.sources.clone(),
        data_file,
        contexts: configuration.coverage_test_contexts,
        static_context: coverage.context.clone(),
        branches: coverage.branch,
        exclude_lines: coverage
            .exclude_lines
            .iter()
            .map(|pattern| pattern.as_str().to_owned())
            .collect(),
        partial_branches: coverage
            .partial_branches
            .iter()
            .map(|pattern| pattern.as_str().to_owned())
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use karva_ipc::WorkerConfiguration;

    use super::worker_coverage_config;

    #[test]
    fn coverage_config_is_absent_without_sources() {
        let configuration = WorkerConfiguration::default();
        let settings = configuration.options.to_settings();
        assert!(
            worker_coverage_config(&settings, &configuration, None)
                .expect("coverage config")
                .is_none()
        );
    }

    #[test]
    fn coverage_config_requires_worker_artifact() {
        let configuration = WorkerConfiguration {
            options: serde_json::from_value(serde_json::json!({"coverage": {"sources": ["src"]}}))
                .expect("valid options"),
            ..WorkerConfiguration::default()
        };
        let settings = configuration.options.to_settings();
        let error = worker_coverage_config(&settings, &configuration, None)
            .expect_err("missing coverage artifact");
        assert_eq!(
            error.to_string(),
            "worker assignment requires a coverage data file when coverage sources are configured"
        );
    }

    #[test]
    fn coverage_config_preserves_measurement_settings() {
        let configuration = WorkerConfiguration {
            options: serde_json::from_value(serde_json::json!({"coverage": {
                "sources": ["", "pkg"], "branch": true, "context": "CI",
                "exclude-lines": ["# excluded"], "partial-branches": ["# partial"],
            }}))
            .expect("valid options"),
            coverage_test_contexts: true,
            ..WorkerConfiguration::default()
        };
        let settings = configuration.options.to_settings();
        let data_file = Utf8PathBuf::from(".coverage.worker-0");
        let coverage = worker_coverage_config(&settings, &configuration, Some(data_file.clone()))
            .expect("coverage config")
            .expect("enabled coverage");
        assert_eq!(coverage.sources, ["", "pkg"]);
        assert_eq!(coverage.data_file, data_file);
        assert!(coverage.contexts);
        assert!(coverage.branches);
        assert_eq!(coverage.static_context.as_deref(), Some("CI"));
        assert_eq!(coverage.exclude_lines, ["# excluded"]);
        assert_eq!(coverage.partial_branches, ["# partial"]);
    }
}
