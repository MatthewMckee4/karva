use std::process::Command;

use camino::Utf8PathBuf;

use karva_cache::{RunArtifacts, RunHash};
use karva_cli::SubTestCommand;
use karva_ipc::ControllerEndpoint;
use karva_logging::TerminalColor;
use karva_metadata::{EnvironmentVariable, ProjectSettings};
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

    /// User CLI options that must be forwarded to workers.
    pub args: &'a SubTestCommand,

    /// Effective worker count exposed to test processes.
    pub num_workers: usize,

    /// Resolved configuration profile propagated through `KARVA_PROFILE`.
    pub profile: &'a str,

    /// Executable selected for the worker subprocess.
    pub worker_binary: &'a Utf8PathBuf,

    /// Whether each worker must write a coverage artifact.
    pub coverage_enabled: bool,
}

/// Builds one worker command with its resolved controller settings.
pub fn worker_command(spawn: &WorkerSpawn, worker_id: usize) -> Command {
    let mut cmd = Command::new(spawn.worker_binary);
    cmd.arg("--controller-address")
        .arg(spawn.controller_endpoint.to_argument())
        .arg("--run-id")
        .arg(spawn.run_hash.inner())
        .arg("--worker-id")
        .arg(worker_id.to_string())
        .current_dir(spawn.project.cwd())
        // Ensure python does not buffer output
        .env(PythonEnvVars::PYTHONUNBUFFERED, "1")
        .env(WorkerEnvVars::KARVA, "1")
        .env(WorkerEnvVars::KARVA_WORKER_ID, worker_id.to_string())
        .env(WorkerEnvVars::KARVA_RUN_ID, spawn.run_hash.inner())
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

    cmd.args(inner_cli_args(spawn.project.settings(), spawn.args));

    if spawn.coverage_enabled {
        let data_file = spawn.artifacts.coverage_data_file(worker_id);
        cmd.arg("--cov-data-file").arg(data_file.as_str());
    }

    cmd
}

fn inner_cli_args(settings: &ProjectSettings, args: &SubTestCommand) -> Vec<String> {
    let mut cli_args: Vec<String> = Vec::new();

    if let Some(arg) = args.verbosity.level().cli_arg() {
        cli_args.push(arg.to_string());
    }

    // Forward the resolved max-fail limit to workers. Omitting the flag
    // means "no limit", which matches the default when the user supplies
    // neither `--max-fail` nor a `max-fail` entry in `karva.toml`.
    if let Some(limit) = settings.test().max_fail.limit() {
        cli_args.push(format!("--max-fail={limit}"));
    }

    if settings.terminal().show_python_output {
        cli_args.push("-s".to_string());
    }

    push_value_arg(
        &mut cli_args,
        "--output-format",
        settings.terminal().output_format.as_str(),
    );

    push_value_arg(
        &mut cli_args,
        "--status-level",
        settings.terminal().status_level.as_str(),
    );

    push_value_arg(
        &mut cli_args,
        "--final-status-level",
        settings.terminal().final_status_level.as_str(),
    );

    let color = args.color.or_else(|| {
        colored::control::SHOULD_COLORIZE
            .should_colorize()
            .then_some(TerminalColor::Always)
    });
    if let Some(color) = color {
        push_value_arg(&mut cli_args, "--color", color.as_str());
    }

    if settings.test().try_import_fixtures {
        cli_args.push("--try-import-fixtures".to_string());
    }

    if settings.test().doctest_modules {
        cli_args.push("--doctest-modules".to_string());
    }

    if settings.test().strict_tags {
        cli_args.push("--strict-tags=true".to_string());
        for name in settings.tags().keys() {
            push_value_arg(&mut cli_args, "--registered-tag", name);
        }
    }

    if args.snapshot_update.unwrap_or(false) {
        cli_args.push("--snapshot-update".to_string());
    }

    if settings.test().retry > 0 {
        push_value_arg(&mut cli_args, "--retry", settings.test().retry);
    }

    push_value_arg(
        &mut cli_args,
        "--flaky-result",
        settings.test().flaky_result.as_str(),
    );

    push_value_arg(
        &mut cli_args,
        "--junit-flaky-fail-status",
        settings.junit().flaky_fail_status.as_str(),
    );

    if let Some(threshold) = settings.test().slow_timeout {
        push_value_arg(&mut cli_args, "--slow-timeout", threshold.as_secs_f64());
    }

    if let Some(budget) = settings.test().fail_slow {
        push_value_arg(&mut cli_args, "--fail-slow", budget.as_secs_f64());
    }

    if let Some(timeout) = settings.test().timeout {
        push_value_arg(&mut cli_args, "--timeout", timeout.as_secs_f64());
    }

    for expr in &args.filter_expressions {
        push_value_arg(&mut cli_args, "--filter", expr);
    }

    if let Some(mode) = args.run_ignored {
        push_value_arg(&mut cli_args, "--run-ignored", mode.as_str());
    }

    for source in &settings.coverage().sources {
        cli_args.push(format!("--cov={source}"));
    }

    for pattern in &settings.coverage().exclude_lines {
        cli_args.push(format!("--cov-exclude-line={}", pattern.as_str()));
    }

    for pattern in &settings.coverage().partial_branches {
        cli_args.push(format!("--cov-partial-branch={}", pattern.as_str()));
    }

    if let Some(context) = &settings.coverage().context {
        cli_args.push(format!("--cov-static-context={context}"));
    }

    if let Some(context) = args.cov_context {
        push_value_arg(&mut cli_args, "--cov-context", context.as_str());
    }

    if settings.coverage().branch {
        cli_args.push("--cov-branch".to_string());
    }

    for ovr in settings.overrides() {
        let json = serde_json::json!({
            "filter": ovr.filter.as_str(),
            "retries": ovr.retries,
            "flaky-result": ovr.flaky_result,
            "junit": {
                "flaky-fail-status": ovr.junit_flaky_fail_status,
            },
            "timeout": ovr.timeout.map(|t| t.0),
            "slow-timeout": ovr.slow_timeout.map(|t| t.0),
            "fail-slow": ovr.fail_slow.map(|t| t.0),
        });
        push_value_arg(&mut cli_args, "--override-json", json);
    }

    cli_args
}

fn push_value_arg(args: &mut Vec<String>, flag: &'static str, value: impl std::fmt::Display) {
    args.push(flag.to_string());
    args.push(value.to_string());
}
