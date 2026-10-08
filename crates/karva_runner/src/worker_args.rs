use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use karva_cache::{RunArtifacts, RunHash};
use karva_cli::SubTestCommand;
use karva_ipc::{ControllerEndpoint, WorkerConfiguration};
use karva_logging::TerminalColor;
use karva_metadata::EnvironmentVariable;
use karva_project::Project;
use karva_static::{EnvVars, PythonEnvVars, WorkerEnvVars};

/// Inputs shared by every worker spawned in a single run.
pub struct WorkerSpawn<'a> {
    /// Project whose tests the worker executes.
    pub project: &'a Project,

    /// Run-scoped files used to collect worker coverage data.
    pub artifacts: &'a RunArtifacts,

    /// Local endpoint receiving worker runtime state.
    pub controller_endpoint: ControllerEndpoint,

    /// Identifier shared by controller and all workers in this run.
    pub run_hash: &'a RunHash,

    /// Invocation controls for the test process environment.
    pub args: &'a SubTestCommand,

    /// Run configuration delivered through the authenticated controller connection.
    pub configuration: Arc<WorkerConfiguration>,

    /// Effective worker count exposed to test processes.
    pub num_workers: usize,

    /// Resolved configuration profile propagated through `KARVA_PROFILE`.
    pub profile: &'a str,

    /// Executable selected for the worker subprocess.
    pub worker_binary: &'a Path,

    /// Whether each worker must write a coverage artifact.
    pub coverage_enabled: bool,
}

/// Builds one worker command with its bootstrap identity and test environment.
pub fn worker_command(spawn: &WorkerSpawn, worker_id: usize) -> Command {
    let mut cmd = Command::new(spawn.worker_binary);
    cmd.arg("__worker")
        .current_dir(spawn.project.cwd())
        // Ensure python does not buffer output
        .env(PythonEnvVars::PYTHONUNBUFFERED, "1")
        .env(WorkerEnvVars::KARVA, "1")
        .env(
            WorkerEnvVars::KARVA_WORKSPACE_ROOT,
            spawn.project.cwd().as_str(),
        )
        .env(WorkerEnvVars::KARVA_PROFILE, spawn.profile)
        .env(
            WorkerEnvVars::KARVA_TEST_THREADS,
            spawn.num_workers.to_string(),
        )
        .env(WorkerEnvVars::KARVA_VERSION, karva_version::version());

    match spawn.args.snapshot_update {
        Some(true) => {
            cmd.env(EnvVars::KARVA_SNAPSHOT_UPDATE, "1");
        }
        Some(false) => {
            cmd.env(EnvVars::KARVA_SNAPSHOT_UPDATE, "0");
        }
        None => {}
    }

    for (name, variable) in spawn.project.settings().env() {
        match variable {
            EnvironmentVariable::Set(value) => {
                cmd.env(name.as_str(), value);
            }
            EnvironmentVariable::Preserve(value) => {
                if std::env::var_os(name.as_str()).is_none() {
                    cmd.env(name.as_str(), value);
                }
            }
            EnvironmentVariable::Unset => {
                cmd.env_remove(name.as_str());
            }
        }
    }

    // Bootstrap identity replaces any inherited values before the worker starts.
    cmd.env(
        WorkerEnvVars::KARVA_CONTROLLER_ENDPOINT,
        spawn.controller_endpoint.encode(),
    )
    .env(WorkerEnvVars::KARVA_RUN_ID, spawn.run_hash.inner())
    .env(WorkerEnvVars::KARVA_WORKER_ID, worker_id.to_string());

    cmd
}

/// Captures merged options once for initial and replacement workers.
pub fn worker_configuration(
    project: &Project,
    args: &SubTestCommand,
    failed_first_active: bool,
) -> Arc<WorkerConfiguration> {
    let mut options = project.metadata().options.clone();
    options.env.clear();
    // Runtime ordering is active only when the selected assignment has a cached failure.
    options.test.get_or_insert_default().failed_first = Some(failed_first_active);
    if let Some(coverage) = &mut options.coverage {
        // CLI-only disablement is not serialized by the configuration schema.
        if coverage.disabled.take().unwrap_or_default() {
            coverage.sources = Some(Vec::new());
        }
    }
    if let Some(src) = &mut options.src {
        src.include = None;
    }
    Arc::new(WorkerConfiguration {
        verbosity: args.verbosity.level(),
        color: args.color.or_else(|| {
            colored::control::SHOULD_COLORIZE
                .should_colorize()
                .then_some(TerminalColor::Always)
        }),
        options,
        tags: project.settings().tags().clone(),
        filter_expressions: args.filter_expressions.clone(),
        run_ignored: args.run_ignored.map(Into::into).unwrap_or_default(),
        coverage_test_contexts: args.cov_context == Some(karva_cli::CovContext::Test),
    })
}

#[cfg(test)]
mod tests {
    use karva_cli::SubTestCommand;
    use karva_metadata::ProjectMetadata;
    use karva_project::Project;
    use rstest::rstest;
    use ruff_python_ast::PythonVersion;

    use super::worker_configuration;

    #[rstest]
    fn configuration_preserves_options_without_environment_or_test_paths(
        #[values(false, true)] disable_coverage: bool,
        #[values(false, true)] failed_first_active: bool,
    ) {
        let mut metadata = ProjectMetadata::new("/project".into(), PythonVersion::default());
        metadata.options = serde_json::from_value(serde_json::json!({
            "env": {"SECRET": "process-only"},
            "src": {"include": ["tests"], "respect-ignore-files": false},
            "test": {"test-function-prefix": "check", "retry": 2, "failed-first": true},
            "overrides": [{"filter": "tag(slow)", "timeout": 0.0}],
            "coverage": {"sources": ["src"]},
        }))
        .expect("valid options");
        metadata
            .options
            .coverage
            .as_mut()
            .expect("coverage options")
            .disabled = Some(disable_coverage);
        let project = Project::from_metadata(metadata);
        let args = SubTestCommand {
            filter_expressions: vec!["tag(slow)".to_owned()],
            run_ignored: Some(karva_cli::RunIgnored::All),
            cov_context: Some(karva_cli::CovContext::Test),
            ..SubTestCommand::default()
        };

        let configuration = worker_configuration(&project, &args, failed_first_active);
        let mut expected = project.metadata().options.clone();
        expected.env.clear();
        expected.test.as_mut().expect("test options").failed_first = Some(failed_first_active);
        expected.src.as_mut().expect("source options").include = None;
        let coverage = expected.coverage.as_mut().expect("coverage options");
        coverage.disabled = None;
        if disable_coverage {
            coverage.sources = Some(Vec::new());
        }
        assert_eq!(configuration.options, expected);
        let encoded = serde_json::to_vec(&configuration.options).expect("encode worker options");
        let received: karva_metadata::Options =
            serde_json::from_slice(&encoded).expect("decode worker options");
        assert_eq!(received, expected);
        assert_eq!(
            received.to_settings().test().failed_first,
            failed_first_active
        );
        assert_eq!(
            received.to_settings().coverage().sources,
            project.settings().coverage().sources
        );
        assert_eq!(configuration.filter_expressions, args.filter_expressions);
        assert_eq!(
            configuration.run_ignored,
            karva_metadata::RunIgnoredMode::All
        );
        assert!(configuration.coverage_test_contexts);
        assert_eq!(project.metadata().options.env.len(), 1);
        assert_eq!(project.settings().src().include_paths, ["tests"]);
    }
}
