use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Instant;

use anyhow::{Context as _, Result};
use camino::{Utf8Path, Utf8PathBuf};
use karva_benchmark::{BenchmarkProject, WORKER_COUNT};
use karva_static::ToolEnvVars;

use crate::metric::BenchmarkMetric;

struct KarvaInvocation {
    binary: Utf8PathBuf,
    path: std::ffi::OsString,
    args: Vec<String>,
}

pub fn warm_project_cache(config: &BenchmarkProject, project_root: &Utf8Path) -> Result<()> {
    let invocation = karva_invocation(config, project_root)?;
    let output = run_invocation(&invocation, project_root)?;
    ensure_karva_success(&output, config)
}

pub fn run_project_cli(
    metric: BenchmarkMetric,
    config: &BenchmarkProject,
    project_root: &Utf8Path,
) -> Result<f64> {
    match metric {
        BenchmarkMetric::WallTime => run_project_wall_time(config, project_root),
        BenchmarkMetric::Memory => run_project_peak_rss_kib(config, project_root),
    }
}

fn run_project_wall_time(config: &BenchmarkProject, project_root: &Utf8Path) -> Result<f64> {
    let invocation = karva_invocation(config, project_root)?;

    let start = Instant::now();
    let output = run_invocation(&invocation, project_root)?;
    let elapsed = start.elapsed();

    ensure_karva_success(&output, config)?;

    Ok(elapsed.as_secs_f64())
}

fn run_project_peak_rss_kib(config: &BenchmarkProject, project_root: &Utf8Path) -> Result<f64> {
    #[cfg(target_os = "linux")]
    {
        let invocation = karva_invocation(config, project_root)?;
        let report_path = memory_report_path(project_root, config.name);

        let output = Command::new("/usr/bin/time")
            .current_dir(project_root)
            .env(ToolEnvVars::PATH, &invocation.path)
            .args(["-f", "%M", "-o", report_path.as_str()])
            .arg(invocation.binary.as_str())
            .args(&invocation.args)
            .output()
            .context("Failed to execute `/usr/bin/time` for memory benchmark")?;

        ensure_karva_success(&output, config)?;

        let peak_rss_kib = read_peak_rss_kib(&report_path)?;
        if let Err(err) = fs_err::remove_file(&report_path) {
            eprintln!("failed to remove memory benchmark report `{report_path}`: {err}");
        }

        Ok(peak_rss_kib)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = config;
        let _ = project_root;
        anyhow::bail!("Memory benchmarks require Linux and GNU `/usr/bin/time`")
    }
}

fn run_invocation(invocation: &KarvaInvocation, project_root: &Utf8Path) -> Result<Output> {
    invocation.command(project_root).output().with_context(|| {
        format!(
            "Failed to execute `{}`",
            invocation.binary.as_std_path().display()
        )
    })
}

fn karva_invocation(config: &BenchmarkProject, project_root: &Utf8Path) -> Result<KarvaInvocation> {
    let bin_dir = venv_bin_dir(project_root);
    let binary = bin_dir.join(executable_name("karva"));
    let path = path_with_venv_first(&bin_dir)?;
    let worker_count = WORKER_COUNT.to_string();

    let mut args = vec![
        "test".to_string(),
        "--num-workers".to_string(),
        worker_count,
        "--no-ignore".to_string(),
        "--output-format".to_string(),
        "concise".to_string(),
    ];

    if config.try_import_fixtures {
        args.push("--try-import-fixtures".to_string());
    }
    args.extend(config.paths.iter().map(ToString::to_string));

    Ok(KarvaInvocation { binary, path, args })
}

impl KarvaInvocation {
    fn command(&self, project_root: &Utf8Path) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .current_dir(project_root)
            .env(ToolEnvVars::PATH, &self.path)
            .args(&self.args);
        command
    }
}

fn ensure_karva_success(output: &Output, config: &BenchmarkProject) -> Result<()> {
    anyhow::ensure!(
        output.status.success(),
        "Karva exited with status {} for `{}`\nstdout:\n{}\nstderr:\n{}",
        output.status,
        config.name,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    Ok(())
}

#[cfg(target_os = "linux")]
fn memory_report_path(project_root: &Utf8Path, project_name: &str) -> Utf8PathBuf {
    project_root.join(format!(
        ".karva-benchmark-memory-{project_name}-{}.txt",
        std::process::id()
    ))
}

#[cfg(target_os = "linux")]
fn read_peak_rss_kib(path: &Utf8Path) -> Result<f64> {
    let raw = fs_err::read_to_string(path)
        .with_context(|| format!("Failed to read memory benchmark report `{path}`"))?;
    raw.trim()
        .parse::<f64>()
        .with_context(|| format!("Failed to parse peak RSS from `{path}`: {raw:?}"))
}

fn venv_bin_dir(project_root: &Utf8Path) -> Utf8PathBuf {
    if cfg!(target_os = "windows") {
        project_root.join(".venv").join("Scripts")
    } else {
        project_root.join(".venv").join("bin")
    }
}

fn executable_name(name: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn path_with_venv_first(bin_dir: &Utf8Path) -> Result<std::ffi::OsString> {
    let mut paths = vec![PathBuf::from(bin_dir.as_str())];
    if let Some(existing_path) = std::env::var_os(ToolEnvVars::PATH) {
        paths.extend(std::env::split_paths(&existing_path));
    }
    std::env::join_paths(paths).context("Failed to construct PATH for benchmark command")
}

#[cfg(test)]
mod tests {
    use camino::Utf8Path;

    use super::karva_invocation;

    #[test]
    fn cli_benchmark_invocation_uses_normal_cached_status_output() {
        let invocation = karva_invocation(
            &karva_benchmark::SYNTHETIC_PROJECT,
            Utf8Path::new("/tmp/project"),
        )
        .expect("invocation should build");

        assert!(
            !invocation
                .args
                .iter()
                .any(|arg| arg == "--status-level" || arg == "--final-status-level")
        );
        assert!(!invocation.args.iter().any(|arg| arg == "--no-cache"));
    }
}
