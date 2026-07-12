use std::process::Command;

use camino::Utf8PathBuf;

use karva_cache::{RunCache, RunHash};
use karva_cli::SubTestCommand;
use karva_logging::TerminalColor;
use karva_metadata::ProjectSettings;
use karva_project::Project;
use karva_static::{EnvVars, PythonEnvVars, WorkerEnvVars};

use crate::partition::Partition;

/// Inputs shared by every worker spawned in a single run.
pub struct WorkerSpawn<'a> {
    pub project: &'a Project,
    pub cache_dir: &'a Utf8PathBuf,
    pub cache: &'a RunCache,
    pub run_hash: &'a RunHash,
    pub args: &'a SubTestCommand,
    pub num_workers: usize,
    pub profile: &'a str,
    pub worker_binary: &'a Utf8PathBuf,
    pub coverage_enabled: bool,
}

/// Build the `Command` for a single worker.
pub fn worker_command(spawn: &WorkerSpawn, worker_id: usize, partition: &Partition) -> Command {
    let mut cmd = Command::new(spawn.worker_binary);
    cmd.arg("--cache-dir")
        .arg(spawn.cache_dir)
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

    for path in partition.tests() {
        cmd.arg(path);
    }

    cmd.args(inner_cli_args(spawn.project.settings(), spawn.args));

    if spawn.coverage_enabled {
        let data_file = spawn.cache.coverage_data_file(worker_id);
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

    if args.snapshot_update.unwrap_or(false) {
        cli_args.push("--snapshot-update".to_string());
    }

    if settings.test().retry > 0 {
        push_value_arg(&mut cli_args, "--retry", settings.test().retry);
    }

    if let Some(threshold) = settings.test().slow_timeout {
        push_value_arg(&mut cli_args, "--slow-timeout", threshold.as_secs_f64());
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
            "timeout": ovr.timeout.map(|t| t.0),
            "slow-timeout": ovr.slow_timeout.map(|t| t.0),
        });
        push_value_arg(&mut cli_args, "--override-json", json);
    }

    cli_args
}

fn push_value_arg(args: &mut Vec<String>, flag: &'static str, value: impl std::fmt::Display) {
    args.push(flag.to_string());
    args.push(value.to_string());
}
