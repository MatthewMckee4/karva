use std::process::Command;

use camino::Utf8PathBuf;

use karva_cache::{RunCache, RunHash};
use karva_cli::SubTestCommand;
use karva_metadata::ProjectSettings;
use karva_project::Project;
use karva_static::WorkerEnvVars;

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
        .env("PYTHONUNBUFFERED", "1")
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

    cli_args.push("--output-format".to_string());
    cli_args.push(settings.terminal().output_format.as_str().to_string());

    cli_args.push("--status-level".to_string());
    cli_args.push(settings.terminal().status_level.as_str().to_string());

    cli_args.push("--final-status-level".to_string());
    cli_args.push(settings.terminal().final_status_level.as_str().to_string());

    if let Some(color) = args.color {
        cli_args.push("--color".to_string());
        cli_args.push(color.as_str().to_string());
    }

    if settings.test().try_import_fixtures {
        cli_args.push("--try-import-fixtures".to_string());
    }

    if args.snapshot_update.unwrap_or(false) {
        cli_args.push("--snapshot-update".to_string());
    }

    if let Some(retry) = args.retry {
        cli_args.push("--retry".to_string());
        cli_args.push(retry.to_string());
    }

    if let Some(threshold) = settings.test().slow_timeout {
        cli_args.push("--slow-timeout".to_string());
        cli_args.push(format!("{}", threshold.as_secs_f64()));
    }

    if let Some(timeout) = settings.test().timeout {
        cli_args.push("--timeout".to_string());
        cli_args.push(format!("{}", timeout.as_secs_f64()));
    }

    for expr in &args.filter_expressions {
        cli_args.push("--filter".to_string());
        cli_args.push(expr.clone());
    }

    if let Some(mode) = args.run_ignored {
        cli_args.push("--run-ignored".to_string());
        cli_args.push(mode.as_str().to_string());
    }

    for source in &settings.coverage().sources {
        cli_args.push(format!("--cov={source}"));
    }

    for ovr in settings.overrides() {
        let json = serde_json::json!({
            "filter": ovr.filter.as_str(),
            "retries": ovr.retries,
            "timeout": ovr.timeout.map(|t| t.0),
            "slow-timeout": ovr.slow_timeout.map(|t| t.0),
        });
        cli_args.push("--override-json".to_string());
        cli_args.push(json.to_string());
    }

    cli_args
}
