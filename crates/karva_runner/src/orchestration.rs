use std::fmt::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use karva_cache::{CacheReader, generate_run_hash};
use karva_cli::{OutputFormat, SubTestCommand};
use karva_collector::ParallelCollector;
use karva_logging::Printer;
use karva_project::{Db, ProjectDatabase};
use karva_python_semantic::QualifiedFunctionName;

pub struct ParallelTestConfig {
    pub num_workers: usize,
    pub cache_dir: Utf8PathBuf,
}

/// Run tests in parallel using multiple karva core worker processes
pub fn run_parallel_tests(
    db: &ProjectDatabase,
    config: &ParallelTestConfig,
    args: &SubTestCommand,
    printer: &Printer,
) -> Result<bool> {
    let start_time = std::time::Instant::now();

    let test_paths: Vec<_> = db
        .project()
        .test_paths()
        .into_iter()
        .filter_map(Result::ok)
        .collect();

    tracing::debug!(path_count = test_paths.len(), "Found test paths");

    tracing::debug!("Collecting tests from discovered paths");
    let collector = ParallelCollector::new(
        db.system(),
        db.project().metadata(),
        db.project().settings(),
    );
    let collected = collector.collect_all(test_paths);

    let test_paths = collected_to_test_paths(&collected, db.project().cwd());

    tracing::info!(test_count = test_paths.len(), "Collected tests");

    let partitions = partition_tests(test_paths, config.num_workers);

    let run_hash = generate_run_hash();

    let core_binary = find_karva_core_binary()?;

    tracing::info!(worker_count = partitions.len(), "Spawning worker processes");
    let mut workers = Vec::new();
    for (worker_id, paths) in partitions.iter().enumerate() {
        if paths.is_empty() {
            tracing::debug!(worker_id = worker_id, "Skipping worker with no tests");
            continue;
        }

        let mut cmd = Command::new(&core_binary);
        cmd.arg("--project-root")
            .arg(db.project().cwd())
            .arg("--cache-dir")
            .arg(&config.cache_dir)
            .arg("--run-hash")
            .arg(&run_hash.0)
            .arg("--worker-id")
            .arg(worker_id.to_string());

        if let Some(arg) = args.verbosity.level().cli_arg() {
            cmd.arg(arg);
        }

        if args.fail_fast.is_some_and(|fail_fast| fail_fast) {
            cmd.arg("--fail-fast");
        }

        if args.show_output.is_some_and(|show_output| show_output) {
            cmd.arg("-s");
        }

        if let Some(output) = args.output_format {
            cmd.arg("--output-format").arg(output.as_str());
        }

        for path in paths {
            cmd.arg(path);
        }

        let child = cmd
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn karva_core worker process")?;

        tracing::debug!(worker_id = worker_id, pid = child.id(), "Worker spawned");
        workers.push(child);
    }

    tracing::info!(
        workers = workers.len(),
        "All workers spawned, waiting for completion"
    );

    for (idx, worker) in workers.into_iter().enumerate() {
        tracing::debug!(worker_id = idx, "Waiting for worker to complete");
        let output = worker
            .wait_with_output()
            .expect("Failed to wait for worker process");

        if output.status.success() {
            tracing::debug!(worker_id = idx, "Worker completed successfully");
        } else {
            tracing::error!(
                worker_id = idx,
                exit_code = ?output.status.code(),
                "Worker exited with non-zero status"
            );
        }
    }

    tracing::debug!("All workers completed");

    let reader = CacheReader::new(&config.cache_dir, &run_hash)?;
    let result = reader.aggregate_results(config.num_workers)?;

    let mut stdout = printer.stream_for_details().lock();

    let is_concise = matches!(args.output_format, Some(OutputFormat::Concise));

    if (!result.diagnostics.is_empty() || !result.discovery_diagnostics.is_empty())
        && result.stats.total() > 0
        && stdout.is_enabled()
    {
        writeln!(stdout)?;
    }

    if !result.discovery_diagnostics.is_empty() {
        writeln!(stdout, "discovery diagnostics:")?;
        writeln!(stdout)?;
        write!(stdout, "{}", result.discovery_diagnostics)?;

        if is_concise {
            writeln!(stdout)?;
        }
    }

    if !result.diagnostics.is_empty() {
        writeln!(stdout, "diagnostics:")?;
        writeln!(stdout)?;
        write!(stdout, "{}", result.diagnostics)?;

        if is_concise {
            writeln!(stdout)?;
        }
    }

    if (result.diagnostics.is_empty() && result.discovery_diagnostics.is_empty())
        && result.stats.total() > 0
        && stdout.is_enabled()
    {
        writeln!(stdout)?;
    }

    let mut result_stdout = printer.stream_for_failure_summary().lock();

    write!(result_stdout, "{}", result.stats.display(start_time))?;

    Ok(result.stats.is_success())
}

/// Convert collected package to test path strings
fn collected_to_test_paths(
    package: &karva_collector::CollectedPackage,
    project_root: &Utf8PathBuf,
) -> Vec<String> {
    let mut test_paths = Vec::new();

    // Recursively collect test paths from the package
    collect_test_paths_recursive(package, project_root, &mut test_paths);

    test_paths
}

fn collect_test_paths_recursive(
    package: &karva_collector::CollectedPackage,
    project_root: &Utf8PathBuf,
    test_paths: &mut Vec<String>,
) {
    // For each module in package
    for module in package.modules.values() {
        // Create a module path for this file
        let Some(module_path) =
            karva_python_semantic::ModulePath::new(module.path.path().clone(), project_root)
        else {
            continue;
        };

        // For each test function in the module
        for test_fn_def in &module.test_function_defs {
            let qualified_name =
                QualifiedFunctionName::new(test_fn_def.name.to_string(), module_path.clone());
            test_paths.push(qualified_name.to_string());
        }
    }

    // Recurse into subpackages
    for subpackage in package.packages.values() {
        collect_test_paths_recursive(subpackage, project_root, test_paths);
    }
}

/// Partition test paths into N groups using round-robin distribution
fn partition_tests(test_paths: Vec<String>, num_workers: usize) -> Vec<Vec<String>> {
    let mut partitions = vec![Vec::new(); num_workers];

    for (i, path) in test_paths.into_iter().enumerate() {
        partitions[i % num_workers].push(path);
    }

    partitions
}

const KARVA_CORE_BINARY_NAME: &str = "karva-core";

/// Find the `karva_core` binary
fn find_karva_core_binary() -> Result<Utf8PathBuf> {
    tracing::debug!("Searching for karva-core binary");

    // Option 1: Look in same directory as current executable (works for installed binaries and deps tests)
    if let Ok(current_exe) = std::env::current_exe() {
        tracing::trace!(current_exe = ?current_exe, "Current executable path");

        if let Some(current_dir) = current_exe.parent() {
            let core_path = current_dir.join(KARVA_CORE_BINARY_NAME);
            tracing::trace!(path = ?core_path, "Checking same directory as executable");

            if core_path.exists() {
                if let Ok(path) = Utf8PathBuf::try_from(core_path) {
                    tracing::debug!(path = %path, "Found binary in same directory as executable");
                    return Ok(path);
                }
            }

            // Also check if current exe is a test binary in target/debug/deps
            // In that case, look in ../karva-core (one level up from deps)
            if current_dir.ends_with("deps") {
                tracing::trace!("Current exe is in deps directory, checking parent");
                if let Some(target_dir) = current_dir.parent() {
                    let core_path = target_dir.join(KARVA_CORE_BINARY_NAME);
                    tracing::trace!(path = ?core_path, "Checking target directory");

                    if core_path.exists() {
                        if let Ok(path) = Utf8PathBuf::try_from(core_path) {
                            tracing::debug!(path = %path, "Found binary in target directory");
                            return Ok(path);
                        }
                    }
                }
            }
        }
    }

    // Option 2: Look in PATH
    tracing::trace!("Searching PATH for karva-core");
    if let Ok(path) = which::which(KARVA_CORE_BINARY_NAME) {
        if let Ok(utf8_path) = Utf8PathBuf::try_from(path) {
            tracing::debug!(path = %utf8_path, "Found binary in PATH");
            return Ok(utf8_path);
        }
    }

    // Option 3: Look in `.venv/bin/karva-core`
    if let Ok(current_dir) = std::env::current_dir() {
        let venv_path = current_dir
            .join(".venv")
            .join(if cfg!(windows) { "Scripts" } else { "bin" })
            .join(if cfg!(windows) {
                format!("{KARVA_CORE_BINARY_NAME}.exe")
            } else {
                KARVA_CORE_BINARY_NAME.to_string()
            });

        tracing::trace!(path = ?venv_path, "Checking venv directory");

        if venv_path.exists() {
            if let Ok(path) = Utf8PathBuf::try_from(venv_path) {
                tracing::debug!(path = %path, "Found binary in venv");
                return Ok(path);
            }
        }
    }

    tracing::error!("Could not find karva-core binary in any location");
    anyhow::bail!(
        "Could not find karva_core binary. Make sure it's in the same directory as karva or in your PATH."
    )
}
