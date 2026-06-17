//! Wall-time benchmarks for Karva.
//!
//! Each benchmark project is a pinned Git checkout under `target/benchmark_cache/`.
//! Dependencies are either synced from a lockfile or resolved with uv's
//! `--exclude-newer` cap so CI runs are reproducible.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use divan::Bencher;
use fs_err as fs;
use karva_cache::{CACHE_DIR, clean_cache};
use karva_cli::{OutputFormat, SubTestCommand};
use karva_logging::{FinalStatusLevel, Printer, StatusLevel};
use karva_metadata::{Options, ProjectMetadata, SrcOptions, TerminalOptions, TestOptions};
use karva_project::Project;
use karva_runner::RunOutput;
use karva_static::ToolEnvVars;
use ruff_python_ast::PythonVersion;

pub const WORKER_COUNT: usize = 1;

#[derive(Debug, Clone, Copy)]
pub enum DependencySetup {
    LockedUvSync {
        group: &'static str,
    },
    DateCappedUvSync {
        exclude_newer: &'static str,
        all_extras: bool,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct BenchmarkProject {
    pub name: &'static str,
    pub repository: &'static str,
    pub commit: &'static str,
    pub paths: &'static [&'static str],
    pub python_version: PythonVersion,
    pub dependency_setup: DependencySetup,
    pub try_import_fixtures: bool,
}

pub const SYNTHETIC_PROJECT: BenchmarkProject = BenchmarkProject {
    name: "karva-benchmark-1",
    repository: "https://github.com/MatthewMckee4/karva-benchmark-1",
    commit: "89791b99d8b13a1e104af7a0b55b3741e315268a",
    paths: &["tests"],
    python_version: PythonVersion::PY313,
    dependency_setup: DependencySetup::LockedUvSync { group: "dev" },
    try_import_fixtures: false,
};

pub const PACKAGING_PROJECT: BenchmarkProject = BenchmarkProject {
    name: "packaging",
    repository: "https://github.com/pypa/packaging",
    commit: "c901ded1a6b97acee3b6b1eb17526228129c4645",
    paths: &["tests"],
    python_version: PythonVersion::PY313,
    dependency_setup: DependencySetup::DateCappedUvSync {
        exclude_newer: "2026-01-01",
        all_extras: true,
    },
    try_import_fixtures: false,
};

pub const PARSE_PROJECT: BenchmarkProject = BenchmarkProject {
    name: "parse",
    repository: "https://github.com/r1chardj0n3s/parse",
    commit: "a285c6670773dcc3a2085b07fef281320a284a8e",
    paths: &["tests"],
    python_version: PythonVersion::PY313,
    dependency_setup: DependencySetup::DateCappedUvSync {
        exclude_newer: "2026-01-01",
        all_extras: true,
    },
    try_import_fixtures: false,
};

pub const H11_PROJECT: BenchmarkProject = BenchmarkProject {
    name: "h11",
    repository: "https://github.com/python-hyper/h11",
    commit: "62c5068c971579d61fa1b55373390e12f25fd856",
    paths: &["h11/tests"],
    python_version: PythonVersion::PY313,
    dependency_setup: DependencySetup::DateCappedUvSync {
        exclude_newer: "2026-01-01",
        all_extras: true,
    },
    try_import_fixtures: false,
};

pub const MARKUPSAFE_PROJECT: BenchmarkProject = BenchmarkProject {
    name: "markupsafe",
    repository: "https://github.com/pallets/markupsafe",
    commit: "b2e4d9c7687be25695fffbe93a37622302b24fb1",
    paths: &["tests"],
    python_version: PythonVersion::PY313,
    dependency_setup: DependencySetup::DateCappedUvSync {
        exclude_newer: "2026-01-01",
        all_extras: true,
    },
    try_import_fixtures: true,
};

pub const SNIFFIO_PROJECT: BenchmarkProject = BenchmarkProject {
    name: "sniffio",
    repository: "https://github.com/python-trio/sniffio",
    commit: "6996e05d9b9debe32f42f709c8041e744f850478",
    paths: &["sniffio/_tests"],
    python_version: PythonVersion::PY313,
    dependency_setup: DependencySetup::DateCappedUvSync {
        exclude_newer: "2026-01-01",
        all_extras: true,
    },
    try_import_fixtures: false,
};

pub const ITSDANGEROUS_PROJECT: BenchmarkProject = BenchmarkProject {
    name: "itsdangerous",
    repository: "https://github.com/pallets/itsdangerous",
    commit: "672971d66a2ef9f85151e53283113f33d642dabd",
    paths: &["tests"],
    python_version: PythonVersion::PY313,
    dependency_setup: DependencySetup::DateCappedUvSync {
        exclude_newer: "2026-01-01",
        all_extras: true,
    },
    try_import_fixtures: true,
};

pub const PYPARSING_PROJECT: BenchmarkProject = BenchmarkProject {
    name: "pyparsing",
    repository: "https://github.com/pyparsing/pyparsing",
    commit: "057a2e6d1b8391dc85abe725d4d12c0987a9ec10",
    paths: &["tests"],
    python_version: PythonVersion::PY313,
    dependency_setup: DependencySetup::DateCappedUvSync {
        exclude_newer: "2026-01-01",
        all_extras: true,
    },
    try_import_fixtures: true,
};

pub const BLINKER_PROJECT: BenchmarkProject = BenchmarkProject {
    name: "blinker",
    repository: "https://github.com/pallets-eco/blinker",
    commit: "c3364059663df1ddce32799d6b1922af89a345f6",
    paths: &["tests"],
    python_version: PythonVersion::PY313,
    dependency_setup: DependencySetup::DateCappedUvSync {
        exclude_newer: "2026-01-01",
        all_extras: true,
    },
    try_import_fixtures: false,
};

pub const JINJA_PROJECT: BenchmarkProject = BenchmarkProject {
    name: "jinja",
    repository: "https://github.com/pallets/jinja",
    commit: "5ef70112a1ff19c05324ff889dd30405b1002044",
    paths: &[
        "tests/test_runtime.py",
        "tests/test_idtracking.py",
        "tests/test_nodes.py",
    ],
    python_version: PythonVersion::PY313,
    dependency_setup: DependencySetup::DateCappedUvSync {
        exclude_newer: "2026-01-01",
        all_extras: true,
    },
    try_import_fixtures: true,
};

pub const INSTALLER_PROJECT: BenchmarkProject = BenchmarkProject {
    name: "installer",
    repository: "https://github.com/pypa/installer",
    commit: "5a2134bebaadf0c5087ddbaff6cd77abbd28271d",
    paths: &["tests"],
    python_version: PythonVersion::PY313,
    dependency_setup: DependencySetup::DateCappedUvSync {
        exclude_newer: "2026-01-01",
        all_extras: true,
    },
    try_import_fixtures: false,
};

pub const TOMLKIT_PROJECT: BenchmarkProject = BenchmarkProject {
    name: "tomlkit",
    repository: "https://github.com/python-poetry/tomlkit",
    commit: "ae1b6790d99b21bc0a339a5825e7d5e40e7e6f6a",
    paths: &[
        "tests/test_toml_file.py",
        "tests/test_parser.py",
        "tests/test_write.py",
        "tests/test_items.py",
        "tests/test_build.py",
        "tests/test_api.py",
        "tests/test_toml_document.py",
        "tests/test_utils.py",
    ],
    python_version: PythonVersion::PY313,
    dependency_setup: DependencySetup::DateCappedUvSync {
        exclude_newer: "2026-01-01",
        all_extras: true,
    },
    try_import_fixtures: false,
};

pub const OUTCOME_PROJECT: BenchmarkProject = BenchmarkProject {
    name: "outcome",
    repository: "https://github.com/python-trio/outcome",
    commit: "03ed6218b08001877745bb1a9e180c8c5cf7c903",
    paths: &["tests"],
    python_version: PythonVersion::PY313,
    dependency_setup: DependencySetup::DateCappedUvSync {
        exclude_newer: "2026-01-01",
        all_extras: true,
    },
    try_import_fixtures: false,
};

pub const BENCHMARK_PROJECTS: &[BenchmarkProject] = &[
    SYNTHETIC_PROJECT,
    PACKAGING_PROJECT,
    PARSE_PROJECT,
    H11_PROJECT,
    MARKUPSAFE_PROJECT,
    SNIFFIO_PROJECT,
    ITSDANGEROUS_PROJECT,
    PYPARSING_PROJECT,
    BLINKER_PROJECT,
    JINJA_PROJECT,
    INSTALLER_PROJECT,
    TOMLKIT_PROJECT,
    OUTCOME_PROJECT,
];

pub const CLI_BENCHMARK_PROJECTS: &[BenchmarkProject] = &[
    PACKAGING_PROJECT,
    PARSE_PROJECT,
    H11_PROJECT,
    SNIFFIO_PROJECT,
    ITSDANGEROUS_PROJECT,
    PYPARSING_PROJECT,
    BLINKER_PROJECT,
    JINJA_PROJECT,
    INSTALLER_PROJECT,
    TOMLKIT_PROJECT,
    OUTCOME_PROJECT,
];

pub fn prepare_benchmark_project(config: &BenchmarkProject) -> Result<Project> {
    let karva_wheel = karva_project::find_karva_wheel()
        .context("Karva wheel must be built before benchmarking")?;
    prepare_benchmark_project_with_wheel(config, &karva_wheel)
}

pub fn prepare_benchmark_project_with_wheel(
    config: &BenchmarkProject,
    karva_wheel: &Utf8Path,
) -> Result<Project> {
    let project = prepare_benchmark_project_environment(config)?;
    install_benchmark_tools(config, project.cwd(), karva_wheel)?;
    clean_project_cache(project.cwd()).context("Failed to clean benchmark cache")?;

    Ok(project)
}

pub fn prepare_benchmark_project_environment(config: &BenchmarkProject) -> Result<Project> {
    let project_root = ensure_checkout(config).context("Failed to checkout benchmark project")?;
    install_dependencies(config, &project_root)
        .context("Failed to install benchmark dependencies")?;
    clean_project_cache(&project_root).context("Failed to clean benchmark cache")?;

    let mut metadata = ProjectMetadata::new(project_root, config.python_version);
    metadata.options = Options {
        src: Some(SrcOptions {
            include: Some(config.paths.iter().map(ToString::to_string).collect()),
            ..SrcOptions::default()
        }),
        terminal: Some(TerminalOptions {
            status_level: Some(StatusLevel::None),
            final_status_level: Some(FinalStatusLevel::None),
            ..TerminalOptions::default()
        }),
        test: Some(TestOptions {
            try_import_fixtures: Some(config.try_import_fixtures),
            ..TestOptions::default()
        }),
        ..Options::default()
    };

    Ok(Project::from_metadata(metadata))
}

pub fn try_run_project(project: &Project) -> Result<RunOutput> {
    // Single worker keeps wall-time benchmarks deterministic across iterations:
    // no inter-process scheduling jitter, shared-cache contention, or variance
    // from OS worker balancing.
    let config = karva_runner::ParallelTestConfig {
        num_workers: WORKER_COUNT,
        no_cache: true,
        create_ctrlc_handler: false,
        last_failed: false,
        profile: None,
        partition: None,
        test_ordering: karva_runner::TestOrdering::Stable,
    };

    let args = SubTestCommand {
        no_ignore: Some(true),
        output_format: Some(OutputFormat::Concise),
        status_level: Some(StatusLevel::None),
        final_status_level: Some(FinalStatusLevel::None),
        ..SubTestCommand::default()
    };

    let printer = Printer::new(StatusLevel::None, FinalStatusLevel::None);
    let output = karva_runner::run_parallel_tests(project, &config, &args, printer)?;

    anyhow::ensure!(
        output.results.stats.total() > 0,
        "Benchmark project did not run any tests",
    );
    anyhow::ensure!(
        output.results.stats.is_success(),
        "Benchmark project had {} failing tests",
        output.results.stats.failed(),
    );

    Ok(output)
}

pub fn clean_project_cache(project_root: &Utf8Path) -> Result<bool> {
    clean_cache(&project_root.join(CACHE_DIR)).context("Failed to remove Karva benchmark cache")
}

pub fn bench_project(bencher: Bencher, config: &'static BenchmarkProject) {
    bencher
        .with_inputs(move || {
            prepare_benchmark_project(config).expect("Failed to prepare benchmark project")
        })
        .bench_local_refs(|project| {
            try_run_project(project).expect("Karva benchmark run failed");
        });
}

pub fn find_benchmark_project(name: &str) -> Option<&'static BenchmarkProject> {
    BENCHMARK_PROJECTS
        .iter()
        .find(|project| project.name == name)
}

fn ensure_checkout(config: &BenchmarkProject) -> Result<Utf8PathBuf> {
    let project_root = project_cache_dir(config.name)?;
    if !project_root.exists() {
        clone_repository(config, &project_root)?;
    }
    fetch_and_checkout(config, &project_root)?;
    Ok(project_root)
}

fn project_cache_dir(project_name: &str) -> Result<Utf8PathBuf> {
    let target_dir = cargo_target_directory()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("target"));
    let target_dir =
        std::path::absolute(target_dir).context("Failed to construct an absolute path")?;
    let cache_dir = target_dir.join("benchmark_cache").join(project_name);

    if let Some(parent) = cache_dir.parent() {
        fs::create_dir_all(parent).context("Failed to create cache directory")?;
    }

    Utf8PathBuf::from_path_buf(cache_dir).map_err(|path| {
        anyhow::anyhow!(
            "Benchmark cache path is not valid UTF-8: {}",
            path.display()
        )
    })
}

fn clone_repository(config: &BenchmarkProject, target_dir: &Utf8PathBuf) -> Result<()> {
    if let Some(parent) = target_dir.parent() {
        fs::create_dir_all(parent).context("Failed to create parent directory for clone")?;
    }

    run_command(
        Command::new("git").args([
            "clone",
            "--filter=blob:none",
            "--no-checkout",
            config.repository,
            target_dir.as_ref(),
        ]),
        "Git clone failed",
    )
}

fn fetch_and_checkout(config: &BenchmarkProject, project_root: &Utf8PathBuf) -> Result<()> {
    run_command(
        Command::new("git")
            .args(["fetch", "origin", config.commit])
            .current_dir(project_root),
        "Git fetch failed",
    )?;

    run_command(
        Command::new("git")
            .args(["checkout", config.commit])
            .current_dir(project_root),
        "Git checkout failed",
    )?;

    run_command(
        Command::new("git")
            .args(["reset", "--hard", config.commit])
            .current_dir(project_root),
        "Git reset failed",
    )?;

    run_command(
        Command::new("git")
            .args(["clean", "-ffdx", "-e", ".venv"])
            .current_dir(project_root),
        "Git clean failed",
    )
}

fn install_dependencies(config: &BenchmarkProject, project_root: &Utf8PathBuf) -> Result<()> {
    let uv_check = Command::new("uv")
        .arg("--version")
        .output()
        .context("Failed to execute uv version check.")?;

    anyhow::ensure!(
        uv_check.status.success(),
        "uv is not installed or not found in PATH. \
         If you need to install it, follow the instructions at \
         https://docs.astral.sh/uv/getting-started/installation/",
    );

    let python_version = config.python_version.to_string();
    match config.dependency_setup {
        DependencySetup::LockedUvSync { group } => {
            run_command(
                Command::new("uv")
                    .args([
                        "sync",
                        "--locked",
                        "--group",
                        group,
                        "--python",
                        &python_version,
                        "--compile-bytecode",
                    ])
                    .current_dir(project_root),
                "Failed to sync locked benchmark project environment",
            )?;
        }
        DependencySetup::DateCappedUvSync {
            exclude_newer,
            all_extras,
        } => {
            let mut command = Command::new("uv");
            command.args([
                "sync",
                "--python",
                &python_version,
                "--exclude-newer",
                exclude_newer,
                "--compile-bytecode",
            ]);
            if all_extras {
                command.arg("--all-extras");
            }
            command.current_dir(project_root);
            run_command(
                &mut command,
                "Failed to sync date-capped benchmark project environment",
            )?;
        }
    }

    Ok(())
}

pub fn install_benchmark_tools(
    config: &BenchmarkProject,
    project_root: &Utf8Path,
    karva_wheel: &Utf8Path,
) -> Result<()> {
    let venv_path = project_root.join(".venv");

    let mut command = Command::new("uv");
    command
        .args([
            "pip",
            "install",
            "--python",
            venv_path.as_str(),
            "--reinstall-package",
            "karva",
        ])
        .arg(karva_wheel);

    if let DependencySetup::DateCappedUvSync { exclude_newer, .. } = config.dependency_setup {
        command.args(["--exclude-newer", exclude_newer, "pytest"]);
    }

    run_command(&mut command, "Benchmark tool installation failed")?;

    Ok(())
}

fn run_command(command: &mut Command, failure: &str) -> Result<()> {
    let output = command
        .output()
        .with_context(|| format!("Failed to execute command for: {failure}"))?;

    if !output.status.success() {
        anyhow::bail!("{}", command_failure_message(failure, &output));
    }

    Ok(())
}

fn command_failure_message(failure: &str, output: &std::process::Output) -> String {
    format_command_failure(
        failure,
        &output.status.to_string(),
        &output.stdout,
        &output.stderr,
    )
}

fn format_command_failure(failure: &str, status: &str, stdout: &[u8], stderr: &[u8]) -> String {
    let mut message = format!("{failure}\nstatus: {status}");
    append_command_stream(
        &mut message,
        "stdout",
        String::from_utf8_lossy(stdout).trim_end(),
    );
    append_command_stream(
        &mut message,
        "stderr",
        String::from_utf8_lossy(stderr).trim_end(),
    );
    message
}

fn append_command_stream(message: &mut String, label: &str, output: &str) {
    if output.is_empty() {
        return;
    }

    message.push_str("\n\n[");
    message.push_str(label);
    message.push_str("]\n");
    message.push_str(output);
}

static CARGO_TARGET_DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

fn cargo_target_directory() -> Option<&'static PathBuf> {
    CARGO_TARGET_DIR
        .get_or_init(|| {
            #[derive(serde::Deserialize)]
            struct Metadata {
                target_directory: PathBuf,
            }

            std::env::var_os(ToolEnvVars::CARGO_TARGET_DIR)
                .map(PathBuf::from)
                .or_else(|| {
                    let output = Command::new(std::env::var_os(ToolEnvVars::CARGO)?)
                        .args(["metadata", "--format-version", "1"])
                        .output()
                        .ok()?;
                    let metadata: Metadata = serde_json::from_slice(&output.stdout).ok()?;
                    Some(metadata.target_directory)
                })
        })
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::format_command_failure;

    #[test]
    fn command_failure_includes_status_stdout_and_stderr() {
        assert_eq!(
            format_command_failure(
                "uv sync failed",
                "exit status: 1",
                b"resolved\n",
                b"denied\n"
            ),
            "uv sync failed\nstatus: exit status: 1\n\n[stdout]\nresolved\n\n[stderr]\ndenied"
        );
    }

    #[test]
    fn command_failure_includes_stdout_when_stderr_is_empty() {
        assert_eq!(
            format_command_failure(
                "git fetch failed",
                "exit status: 128",
                b"fatal details\n",
                b""
            ),
            "git fetch failed\nstatus: exit status: 128\n\n[stdout]\nfatal details"
        );
    }

    #[test]
    fn command_failure_keeps_empty_output_compact() {
        assert_eq!(
            format_command_failure("git clean failed", "exit status: 1", b"", b""),
            "git clean failed\nstatus: exit status: 1"
        );
    }
}
