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

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use karva_project::{path::SystemPathBuf, tests::find_karva_wheel};
use ruff_python_ast::PythonVersion;
use tempfile::TempDir;

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
    pub paths: Vec<SystemPathBuf>,
    /// Dependencies to install via uv
    pub dependencies: Vec<&'a str>,
    /// Python version to use
    pub python_version: PythonVersion,
}

impl<'a> RealWorldProject<'a> {
    /// Setup a real-world project for benchmarking
    pub fn setup(self) -> Result<InstalledProject<'a>> {
        tracing::debug!("Setting up project {}", self.name);

        let temp_dir = TempDir::with_prefix("karva-benchmark-project").unwrap();

        let project_root = temp_dir.path().join(self.name);

        clone_repository(self.repository, &project_root, self.commit)?;

        let checkout = Checkout {
            temp_dir,
            project: self,
        };

        install_dependencies(&checkout)?;

        Ok(InstalledProject {
            temp_dir: checkout.temp_dir,
            config: checkout.project,
        })
    }
}

struct Checkout<'a> {
    project: RealWorldProject<'a>,
    temp_dir: TempDir,
}

impl<'a> Checkout<'a> {
    fn path(&self) -> PathBuf {
        self.temp_dir.path().join(self.project.name)
    }

    fn venv_path(&self) -> PathBuf {
        self.path().join(".venv")
    }

    const fn project(&self) -> &RealWorldProject<'a> {
        &self.project
    }
}

/// Checked out project with its dependencies installed.
pub struct InstalledProject<'a> {
    /// Path to the cloned project
    pub temp_dir: TempDir,
    /// Project configuration
    pub config: RealWorldProject<'a>,
}

impl<'a> InstalledProject<'a> {
    /// Get the project configuration
    #[must_use]
    pub const fn config(&self) -> &RealWorldProject<'a> {
        &self.config
    }

    /// Get the benchmark paths as `SystemPathBuf`
    #[must_use]
    pub fn test_paths(&self) -> Vec<SystemPathBuf> {
        self.config.paths.clone()
    }

    /// Get the path to the cloned project
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.temp_dir.path().join(self.config.name)
    }

    /// Get the virtual environment path
    #[must_use]
    pub fn venv_path(&self) -> PathBuf {
        self.path().join(".venv")
    }

    /// Get the path to the Python executable
    #[must_use]
    pub fn python_path(&self) -> PathBuf {
        if cfg!(windows) {
            self.venv_path().join("Scripts/python.exe")
        } else {
            self.venv_path().join("bin/python")
        }
    }
}

/// Clone a git repository to the specified directory
fn clone_repository(repo_url: &str, target_dir: &Path, commit: &str) -> Result<()> {
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
            target_dir.to_str().unwrap(),
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
fn install_dependencies(checkout: &Checkout) -> Result<()> {
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

    let venv_path = checkout.venv_path();
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

    let karva_wheel = find_karva_wheel().unwrap();

    // Install dependencies with date constraint in the isolated environment
    let mut cmd = Command::new("uv");
    cmd.args(["pip", "install", "--python", venv_path.to_str().unwrap()])
        .args(&checkout.project().dependencies)
        .arg(karva_wheel);

    let output = cmd
        .output()
        .context("Failed to execute uv pip install command")?;

    anyhow::ensure!(
        output.status.success(),
        "Dependency installation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Install the package
    let mut cmd = Command::new("uv");
    cmd.args(["pip", "install", "--python", venv_path.to_str().unwrap()])
        .arg("-e")
        .arg(checkout.path());

    let output = cmd
        .output()
        .context("Failed to execute uv pip install command")?;

    anyhow::ensure!(
        output.status.success(),
        "Package installation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Run `uv pip list` and print its output
    let mut cmd = Command::new("uv");
    cmd.args(["pip", "list", "--python", venv_path.to_str().unwrap()]);

    let output = cmd
        .output()
        .context("Failed to execute uv pip list command")?;

    eprintln!(
        "uv pip list output:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );

    Ok(())
}
