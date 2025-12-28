use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use karva_cache::{CacheReader, generate_run_hash};
use karva_collector::ParallelCollector;
use karva_diagnostic::TestRunResult;
use karva_project::{Db, ProjectDatabase};
use karva_python_semantic::QualifiedFunctionName;

pub struct ParallelTestConfig {
    pub num_workers: usize,
    pub cache_dir: Utf8PathBuf,
    pub fail_fast: bool,
    pub show_output: bool,
}

/// Run tests in parallel using multiple karva_core worker processes
pub fn run_parallel_tests(
    db: &ProjectDatabase,
    config: ParallelTestConfig,
) -> Result<TestRunResult> {
    tracing::info!(
        num_workers = config.num_workers,
        cache_dir = %config.cache_dir,
        "Starting parallel test orchestration"
    );

    // 1. Get test paths from the project
    let test_paths: Vec<_> = db
        .project()
        .test_paths()
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();

    tracing::debug!(path_count = test_paths.len(), "Found test paths");

    // 2. Collect tests using karva_collector
    tracing::debug!("Collecting tests from discovered paths");
    let collector = ParallelCollector::new(
        db.system(),
        db.project().metadata(),
        db.project().settings(),
    );
    let collected = collector.collect_all(test_paths);

    // 2. Convert collected tests to test path strings
    let test_paths = collected_to_test_paths(&collected, db.project().cwd());

    tracing::info!(test_count = test_paths.len(), "Collected tests");

    if test_paths.is_empty() {
        tracing::warn!("No tests found, returning empty result");
        // No tests found, return empty result
        return Ok(TestRunResult::default());
    }

    // 3. Partition test paths into N groups
    let partitions = partition_tests(&test_paths, config.num_workers);

    for (i, partition) in partitions.iter().enumerate() {
        tracing::debug!(
            worker_id = i,
            test_count = partition.len(),
            "Partition created"
        );
    }

    // 4. Generate run hash for this test run
    let run_hash = generate_run_hash();
    tracing::debug!(run_hash = %run_hash.0, "Generated run hash");

    // 5. Find karva_core binary
    tracing::debug!("Looking for karva_core binary");
    let core_binary = find_karva_core_binary()?;
    tracing::info!(binary = %core_binary, "Found karva_core binary");

    // 6. Spawn worker processes
    tracing::info!(worker_count = partitions.len(), "Spawning worker processes");
    let mut workers = Vec::new();
    for (worker_id, paths) in partitions.iter().enumerate() {
        if paths.is_empty() {
            tracing::debug!(worker_id = worker_id, "Skipping worker with no tests");
            continue;
        }

        tracing::debug!(
            worker_id = worker_id,
            test_count = paths.len(),
            "Spawning worker"
        );

        let mut cmd = Command::new(&core_binary);
        cmd.arg("--project-root")
            .arg(db.project().cwd())
            .arg("--cache-dir")
            .arg(&config.cache_dir)
            .arg("--run-hash")
            .arg(&run_hash.0)
            .arg("--worker-id")
            .arg(worker_id.to_string());

        // Add boolean flags only if true
        if config.fail_fast {
            cmd.arg("--fail-fast");
        }
        if config.show_output {
            cmd.arg("--show-output");
        }

        cmd.arg("--test-paths");

        // Add all test paths for this worker
        for path in paths {
            cmd.arg(path);
        }

        let child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn karva_core worker process")?;

        tracing::debug!(worker_id = worker_id, pid = child.id(), "Worker spawned");
        workers.push(child);
    }

    tracing::info!(
        workers = workers.len(),
        "All workers spawned, waiting for completion"
    );

    // 7. Wait for all workers to complete
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

    tracing::info!("All workers completed");

    // 8. Read and aggregate results from cache
    tracing::debug!("Reading and aggregating results from cache");
    let reader = CacheReader::new(config.cache_dir.clone(), run_hash)?;
    let aggregated = reader.aggregate_results(config.num_workers)?;

    tracing::info!(
        passed = aggregated.stats.passed(),
        failed = aggregated.stats.failed(),
        skipped = aggregated.stats.skipped(),
        "Results aggregated"
    );

    // 9. Print diagnostics directly to stderr (already formatted by workers)
    let mut stderr = std::io::stderr().lock();

    if !aggregated.discovery_diagnostics.is_empty() {
        writeln!(stderr)?;
        writeln!(stderr, "discovery diagnostics:")?;
        writeln!(stderr)?;
        write!(stderr, "{}", aggregated.discovery_diagnostics)?;
    }

    if !aggregated.diagnostics.is_empty() {
        writeln!(stderr)?;
        writeln!(stderr, "diagnostics:")?;
        writeln!(stderr)?;
        write!(stderr, "{}", aggregated.diagnostics)?;
    }

    // 10. Cleanup cache
    tracing::debug!("Cleaning up cache");
    reader.cleanup()?;

    // Return a TestRunResult with stats but no diagnostics (already printed)
    Ok(TestRunResult::default().with_stats(aggregated.stats))
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
fn partition_tests(test_paths: &[String], num_workers: usize) -> Vec<Vec<String>> {
    let mut partitions = vec![Vec::new(); num_workers];

    for (i, path) in test_paths.iter().enumerate() {
        partitions[i % num_workers].push(path.clone());
    }

    partitions
}

/// Find the karva_core binary
fn find_karva_core_binary() -> Result<Utf8PathBuf> {
    tracing::debug!("Searching for karva-core binary");

    // Option 1: Look in same directory as current executable (works for installed binaries and deps tests)
    if let Ok(current_exe) = std::env::current_exe() {
        tracing::trace!(current_exe = ?current_exe, "Current executable path");

        if let Some(current_dir) = current_exe.parent() {
            let core_path = current_dir.join("karva-core");
            tracing::trace!(path = ?core_path, "Checking same directory as executable");

            if core_path.exists() {
                if let Ok(path) = Utf8PathBuf::try_from(core_path.clone()) {
                    tracing::debug!(path = %path, "Found binary in same directory as executable");
                    return Ok(path);
                }
            }

            // Also check if current exe is a test binary in target/debug/deps
            // In that case, look in ../karva-core (one level up from deps)
            if current_dir.ends_with("deps") {
                tracing::trace!("Current exe is in deps directory, checking parent");
                if let Some(target_dir) = current_dir.parent() {
                    let core_path = target_dir.join("karva-core");
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
    if let Ok(path) = which::which("karva-core") {
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
                "karva-core.exe"
            } else {
                "karva-core"
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
