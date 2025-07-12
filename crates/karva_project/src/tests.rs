use std::{fs, path::PathBuf, process::Command};

use anyhow::{Context, Result};
use tempfile::TempDir;

use crate::path::SystemPathBuf;

pub struct TestEnv {
    project_dir: SystemPathBuf,
}

impl TestEnv {
    #[must_use]
    pub fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let project_dir = SystemPathBuf::from(temp_dir.path().to_path_buf());

        fs::create_dir_all(&project_dir).expect("Failed to create project directory");

        Self { project_dir }
    }

    fn venv_path(&self) -> PathBuf {
        self.project_dir.join(".venv")
    }

    pub fn with_dependencies(&self, dependencies: &[&str]) -> Result<()> {
        let venv_path = self.venv_path();

        let output = Command::new("uv")
            .arg("venv")
            .arg(&venv_path)
            .output()
            .context("Failed to execute uv venv command")?;

        if !output.status.success() {
            anyhow::bail!("uv venv failed with status: {:?}", output.status);
        }

        let output = Command::new("uv")
            .arg("pip")
            .arg("install")
            .arg("--python")
            .arg(&venv_path)
            .args(dependencies)
            .output()
            .context("Failed to execute uv pip install command")?;

        if !output.status.success() {
            anyhow::bail!("uv pip install failed with status: {:?}", output.status);
        }

        Ok(())
    }

    #[must_use]
    pub fn create_test_dir(&self) -> SystemPathBuf {
        self.create_dir(format!("tests_{}", rand::random::<u32>()))
    }

    #[allow(clippy::must_use_candidate)]
    pub fn create_file(&self, path: impl AsRef<std::path::Path>, content: &str) -> SystemPathBuf {
        let path = path.as_ref();
        let path = self.project_dir.join(path);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, &*ruff_python_trivia::textwrap::dedent(content)).unwrap();

        SystemPathBuf::from(path)
    }

    #[allow(clippy::must_use_candidate)]
    pub fn create_dir(&self, path: impl AsRef<std::path::Path>) -> SystemPathBuf {
        let path = self.project_dir.join(path);
        fs::create_dir_all(&path).unwrap();
        SystemPathBuf::from(path)
    }

    #[must_use]
    pub fn temp_path(&self, path: impl AsRef<std::path::Path>) -> SystemPathBuf {
        SystemPathBuf::from(self.project_dir.join(path))
    }

    #[must_use]
    pub fn cwd(&self) -> SystemPathBuf {
        self.project_dir.clone()
    }

    #[must_use]
    pub fn relative_path(&self, path: &SystemPathBuf) -> SystemPathBuf {
        SystemPathBuf::from(path.strip_prefix(self.cwd()).unwrap())
    }
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}
