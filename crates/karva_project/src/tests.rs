use std::{fs, path::PathBuf, process::Command};

use anyhow::{Context, Result};
use tempfile::TempDir;

use crate::path::SystemPathBuf;

pub struct TestEnv {
    project_dir: PathBuf,
}

impl TestEnv {
    #[must_use]
    pub fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let project_dir = temp_dir.path().to_path_buf();

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
    pub fn create_tests_dir(&self) -> SystemPathBuf {
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
        SystemPathBuf::from(self.project_dir.clone())
    }
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}

pub struct MockFixture {
    pub name: String,
    pub scope: String,
    pub body: String,
    pub args: String,
}

#[must_use]
pub fn mock_fixture(fixtures: &[MockFixture]) -> String {
    let fixtures = fixtures
        .iter()
        .map(|fixture| {
            format!(
                r"@fixture(scope='{scope}')
def {name}({args}):
    {body}
",
                name = fixture.name,
                scope = fixture.scope,
                args = fixture.args,
                body = fixture.body,
            )
        })
        .collect::<Vec<String>>()
        .join("\n");

    format!(
        r"

class FixtureFunctionMarker:
    def __init__(self, scope, name = None):
        self.scope = scope
        self.name = name

    def __call__(self, function):
        return FixtureFunctionDefinition(
            function=function,
            fixture_function_marker=self,
        )

class FixtureFunctionDefinition:
    def __init__(
        self,
        *,
        function,
        fixture_function_marker,
    ):
        self.name = fixture_function_marker.name or function.__name__
        self.__name__ = self.name
        self._fixture_function_marker = fixture_function_marker
        self._fixture_function = function

    def __get__(
        self,
        instance = None,
        owner = None,
    ):
        return self

    def __call__(self, *args, **kwds):
        return self._fixture_function(*args, **kwds)

def fixture(
    fixture_function = None,
    *,
    scope = 'function',
    name = None,
):
    fixture_marker = FixtureFunctionMarker(
        scope=scope,
        name=name,
    )
    if fixture_function:
        return fixture_marker(fixture_function)
    return fixture_marker

{fixtures}
"
    )
}
