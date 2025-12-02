#![allow(clippy::print_stderr)]

//! Infrastructure for benchmarking real-world Python projects.
//!
//! The module uses a setup similar to mypy primer's, which should make it easy
//! to add new benchmarks for projects in [mypy primer's project's list](https://github.com/hauntsaninja/mypy_primer/blob/ebaa9fd27b51a278873b63676fd25490cec6823b/mypy_primer/projects.py#L74).
//!
//! The basic steps for a project are:
//! 1. Clone or update the project into a directory inside `./target`. The commits are pinnted to prevent flaky benchmark results due to new commits.
//! 2. For projects with dependencies, run uv to create a virtual environment and install the dependencies.
//! 3. (optionally) Copy the entire project structure into a memory file system to reduce the IO noise in benchmarks.
//! 4. (not in this module) Create a `ProjectDatabase` and run the benchmark.

use std::{path::PathBuf, process::Command};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use ruff_python_ast::PythonVersion;

/// Configuration for a real-world project to benchmark
#[derive(Debug, Clone)]
pub struct RealWorldProject<'a> {
    // The name of the project.
    pub name: &'a str,
    /// The project's GIT repository. Must be publicly accessible.
    pub repository: &'a str,
    /// Specific commit hash to checkout
    pub commit: &'a str,
    /// List of paths within the project to check (`ty check <paths>`)
    pub paths: &'a [&'a str],
    /// Dependencies to install via uv
    pub dependencies: &'a [&'a str],
    /// Python version to use
    pub python_version: PythonVersion,
}

impl<'a> RealWorldProject<'a> {
    /// Setup a real-world project for benchmarking
    pub fn setup(self, venv_in_project_dir: bool) -> Result<InstalledProject<'a>> {
        tracing::debug!("Setting up project {}", self.name);

        // Create project directory in cargo target
        let project_root = get_project_cache_dir(self.name)?;

        // Clone the repository if it doesn't exist, or update if it does
        if project_root.exists() {
            tracing::debug!("Updating repository for project '{}'...", self.name);
            let start = std::time::Instant::now();
            update_repository(&project_root, self.commit)?;
            tracing::debug!(
                "Repository update completed in {:.2}s",
                start.elapsed().as_secs_f64()
            );
        } else {
            tracing::debug!("Cloning repository for project '{}'...", self.name);
            let start = std::time::Instant::now();
            clone_repository(self.repository, &project_root, self.commit)?;
            tracing::debug!(
                "Repository clone completed in {:.2}s",
                start.elapsed().as_secs_f64()
            );
        }

        let checkout = Checkout {
            path: project_root,
            project: self,
        };

        let venv_dir = if venv_in_project_dir {
            Some(checkout.path.join(".venv").into_std_path_buf())
        } else {
            None
        };

        install_dependencies(&checkout, venv_dir)?;

        Ok(InstalledProject {
            path: checkout.path,
            config: checkout.project,
        })
    }
}

struct Checkout<'a> {
    project: RealWorldProject<'a>,
    path: Utf8PathBuf,
}

impl<'a> Checkout<'a> {
    const fn project(&self) -> &RealWorldProject<'a> {
        &self.project
    }

    const fn project_root(&self) -> &Utf8PathBuf {
        &self.path
    }
}

pub struct InstalledProject<'a> {
    /// Path to the cloned project
    pub path: Utf8PathBuf,
    /// Project configuration
    pub config: RealWorldProject<'a>,
}

impl<'a> InstalledProject<'a> {
    pub const fn config(&self) -> &RealWorldProject<'a> {
        &self.config
    }

    pub const fn test_paths(&self) -> &[&str] {
        self.config.paths
    }

    pub const fn path(&self) -> &Utf8PathBuf {
        &self.path
    }
}

/// Get the cache directory for a project in the cargo target directory
fn get_project_cache_dir(project_name: &str) -> Result<Utf8PathBuf> {
    let target_dir = cargo_target_directory()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("target"));
    let target_dir =
        std::path::absolute(target_dir).context("Failed to construct an absolute path")?;
    let cache_dir = target_dir.join("benchmark_cache").join(project_name);

    if let Some(parent) = cache_dir.parent() {
        std::fs::create_dir_all(parent).context("Failed to create cache directory")?;
    }

    Ok(Utf8PathBuf::from_path_buf(cache_dir).unwrap())
}

/// Update an existing repository
fn update_repository(project_root: &Utf8PathBuf, commit: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["fetch", "origin", commit])
        .current_dir(project_root)
        .output()
        .context("Failed to execute git fetch command")?;

    if !output.status.success() {
        anyhow::bail!(
            "Git fetch of commit {} failed: {}",
            commit,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Checkout specific commit
    let output = Command::new("git")
        .args(["checkout", commit])
        .current_dir(project_root)
        .output()
        .context("Failed to execute git checkout command")?;

    anyhow::ensure!(
        output.status.success(),
        "Git checkout of commit {} failed: {}",
        commit,
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}

/// Clone a git repository to the specified directory
fn clone_repository(repo_url: &str, target_dir: &Utf8PathBuf, commit: &str) -> Result<()> {
    // Create parent directory if it doesn't exist
    if let Some(parent) = target_dir.parent() {
        std::fs::create_dir_all(parent).context("Failed to create parent directory for clone")?;
    }

    // Clone with minimal depth and fetch only the specific commit
    let output = Command::new("git")
        .args([
            "clone",
            "--filter=blob:none", // Don't download large files initially
            "--no-checkout",      // Don't checkout files yet
            repo_url,
            target_dir.as_ref(),
        ])
        .output()
        .context("Failed to execute git clone command")?;

    anyhow::ensure!(
        output.status.success(),
        "Git clone failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Fetch the specific commit
    let output = Command::new("git")
        .args(["fetch", "origin", commit])
        .current_dir(target_dir)
        .output()
        .context("Failed to execute git fetch command")?;

    anyhow::ensure!(
        output.status.success(),
        "Git fetch of commit {} failed: {}",
        commit,
        String::from_utf8_lossy(&output.stderr)
    );

    // Checkout the specific commit
    let output = Command::new("git")
        .args(["checkout", commit])
        .current_dir(target_dir)
        .output()
        .context("Failed to execute git checkout command")?;

    anyhow::ensure!(
        output.status.success(),
        "Git checkout of commit {} failed: {}",
        commit,
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}

/// Install dependencies using uv with date constraints
fn install_dependencies(checkout: &Checkout, venv_dir: Option<PathBuf>) -> Result<()> {
    // Check if uv is available
    let uv_check = Command::new("uv")
        .arg("--version")
        .output()
        .context("Failed to execute uv version check.")?;

    if !uv_check.status.success() {
        anyhow::bail!(
            "uv is not installed or not found in PATH. If you need to install it, follow the instructions at https://docs.astral.sh/uv/getting-started/installation/"
        );
    }

    let venv_path = venv_dir.unwrap_or_else(global_venv_path);
    let python_version_str = checkout.project().python_version.to_string();

    let output = Command::new("uv")
        .args(["venv", "--python", &python_version_str, "--allow-existing"])
        .arg(&venv_path)
        .output()
        .context("Failed to execute uv venv command")?;

    anyhow::ensure!(
        output.status.success(),
        "Failed to create virtual environment: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    if checkout.project().dependencies.is_empty() {
        tracing::debug!(
            "No dependencies to install for project '{}'",
            checkout.project().name
        );
        return Ok(());
    }

    let output = Command::new("uv")
        .args(["pip", "install", "--python", venv_path.to_str().unwrap()])
        .args(checkout.project().dependencies)
        .output()
        .context("Failed to execute uv pip install command")?;

    anyhow::ensure!(
        output.status.success(),
        "Dependency installation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new("uv")
        .args(["pip", "install", "--python", venv_path.to_str().unwrap()])
        .arg(checkout.project_root())
        .output()
        .context("Failed to execute uv pip install command")?;

    anyhow::ensure!(
        output.status.success(),
        "Package installation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}

fn global_venv_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".venv")
}

static CARGO_TARGET_DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

fn cargo_target_directory() -> Option<&'static PathBuf> {
    CARGO_TARGET_DIR
        .get_or_init(|| {
            #[derive(serde::Deserialize)]
            struct Metadata {
                target_directory: PathBuf,
            }

            std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .or_else(|| {
                    let output = Command::new(std::env::var_os("CARGO")?)
                        .args(["metadata", "--format-version", "1"])
                        .output()
                        .ok()?;
                    let metadata: Metadata = serde_json::from_slice(&output.stdout).ok()?;
                    Some(metadata.target_directory)
                })
        })
        .as_ref()
}

pub static AFFECT_PROJECT: RealWorldProject<'static> = RealWorldProject {
    name: "affect",
    repository: "https://github.com/MatthewMckee4/affect",
    commit: "9a5198e46a5895ae3b3d56be80beb3bbb92c629e",
    paths: &["tests"],
    dependencies: &["pydantic", "pydantic-settings", "pytest"],
    python_version: PythonVersion::PY313,
};

pub static SQLMODEL_PROJECT: RealWorldProject<'static> = RealWorldProject {
    name: "sqlmodel",
    repository: "https://github.com/fastapi/sqlmodel",
    commit: "43570910db2d7ab2e5efd96f60a0e2a3a61c5474",
    paths: &["tests"],
    dependencies: &[
        "pydantic",
        "SQLAlchemy",
        "pytest",
        "dirty-equals",
        "fastapi",
        "httpx",
        "coverage",
        "black",
        "jinja2",
    ],
    python_version: PythonVersion::PY313,
};

pub static TYPER_PROJECT: RealWorldProject<'static> = RealWorldProject {
    name: "typer",
    repository: "https://github.com/fastapi/typer",
    commit: "a9a6595ad74ed59805c085f9d5369e666b955818",
    paths: &["tests"],
    dependencies: &["click", "typing-extensions", "pytest", "coverage"],
    python_version: PythonVersion::PY313,
};

pub static PYDANTIC_SETTINGS_PROJECT: RealWorldProject<'static> = RealWorldProject {
    name: "pydantic-settings",
    repository: "https://github.com/pydantic/pydantic-settings",
    commit: "ffb3ac673633e4f26a512b0fbf986d9bf8821a50",
    paths: &["tests"],
    dependencies: &["pydantic", "pytest", "python-dotenv", "typing-extensions"],
    python_version: PythonVersion::PY313,
};

pub static PYDANTIC_PROJECT: RealWorldProject<'static> = RealWorldProject {
    name: "pydantic",
    repository: "https://github.com/pydantic/pydantic",
    commit: "ed67e3ebf7c9a55de75de0e8995dbce36551eaca",
    paths: &["tests"],
    dependencies: &[
        "pytest",
        "typing-extensions",
        "annotated-types",
        "pydantic-core --only-binary :all:",
        "typing-inspection",
    ],
    python_version: PythonVersion::PY313,
};

pub fn all_projects() -> Vec<&'static RealWorldProject<'static>> {
    vec![
        &AFFECT_PROJECT,
        &PYDANTIC_SETTINGS_PROJECT,
        &PYDANTIC_PROJECT,
        &SQLMODEL_PROJECT,
        &TYPER_PROJECT,
    ]
}
