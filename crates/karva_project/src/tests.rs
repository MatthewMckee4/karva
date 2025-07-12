use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Context;
use tempfile::TempDir;

use crate::path::SystemPathBuf;

/// Find the karva wheel in the target/wheels directory.
/// Returns the path to the wheel file.
fn find_karva_wheel() -> anyhow::Result<PathBuf> {
    let karva_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| anyhow::anyhow!("Could not determine KARVA_ROOT"))?
        .to_path_buf();

    let wheels_dir = karva_root.join("target").join("wheels");

    let entries = std::fs::read_dir(&wheels_dir)
        .with_context(|| format!("Could not read wheels directory: {}", wheels_dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        if let Some(name) = file_name.to_str() {
            if name.starts_with("karva-")
                && std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("whl"))
            {
                return Ok(entry.path());
            }
        }
    }

    anyhow::bail!("Could not find karva wheel in target/wheels directory");
}

pub struct TestEnv {
    _temp_dir: TempDir,
    project_dir: PathBuf,
}

impl TestEnv {
    #[must_use]
    pub fn new() -> Self {
        let temp_dir = TempDir::with_prefix("karva-test-env").unwrap();

        let project_dir = dunce::simplified(
            &temp_dir
                .path()
                .canonicalize()
                .context("Failed to canonicalize project path")
                .unwrap(),
        )
        .to_path_buf();

        let karva_wheel = find_karva_wheel().unwrap();

        let venv_path = project_dir.join(".venv");

        // Set up a bare uv project and install pytest, mimicking the Python TestEnv
        let commands = [
            vec![
                "uv",
                "init",
                "--bare",
                "--directory",
                project_dir.to_str().unwrap(),
            ],
            vec!["uv", "venv", venv_path.to_str().unwrap()],
            vec![
                "uv",
                "pip",
                "install",
                "--python",
                venv_path.to_str().unwrap(),
                karva_wheel.to_str().unwrap(),
                "pytest",
            ],
        ];

        for command in &commands {
            let output = Command::new(command[0])
                .args(&command[1..])
                .current_dir(&project_dir)
                .output()
                .with_context(|| format!("Failed to run command: {command:?}"))
                .unwrap();
            if output.status.success() {
                eprintln!(
                    "Command succeeded: {:?}\nstdout:\n{}\nstderr:\n{}",
                    command,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            } else {
                eprintln!(
                    "Command failed: {:?}\nstdout:\n{}\nstderr:\n{}",
                    command,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                panic!("Command failed: {command:?}");
            }
        }

        Self {
            project_dir,
            _temp_dir: temp_dir,
        }
    }

    #[must_use]
    pub fn create_test_dir(&self) -> SystemPathBuf {
        self.create_dir(format!("tests_{}", rand::random::<u32>()))
    }

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
