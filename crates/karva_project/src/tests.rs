use std::fs;

use tempfile::TempDir;

use crate::path::SystemPathBuf;

pub struct TestEnv {
    temp_dir: TempDir,
}

impl TestEnv {
    #[must_use]
    pub fn new() -> Self {
        Self {
            temp_dir: TempDir::new().expect("Failed to create temp directory"),
        }
    }

    #[must_use]
    pub fn create_tests_dir(&self) -> SystemPathBuf {
        self.create_dir(format!("tests_{}", rand::random::<u32>()))
    }

    #[allow(clippy::must_use_candidate)]
    pub fn create_file(&self, path: impl AsRef<std::path::Path>, content: &str) -> SystemPathBuf {
        let path = self.temp_dir.path().join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        SystemPathBuf::from(path)
    }

    #[allow(clippy::must_use_candidate)]
    pub fn create_dir(&self, path: impl AsRef<std::path::Path>) -> SystemPathBuf {
        let path = self.temp_dir.path().join(path);
        fs::create_dir_all(&path).unwrap();
        SystemPathBuf::from(path)
    }

    #[must_use]
    pub fn temp_path(&self, path: impl AsRef<std::path::Path>) -> SystemPathBuf {
        SystemPathBuf::from(self.temp_dir.path().join(path))
    }

    #[must_use]
    pub fn cwd(&self) -> SystemPathBuf {
        SystemPathBuf::from(self.temp_dir.path())
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
        r"from collections.abc import Callable
from typing import Any

class FixtureFunctionMarker:
    scope: str
    name: str | None = None

    def __init__(self, scope: str, name: str | None = None) -> None:
        self.scope = scope
        self.name = name

    def __call__(self, function: Callable[..., object]):
        return FixtureFunctionDefinition(
            function=function,
            fixture_function_marker=self,
        )

class FixtureFunctionDefinition:
    def __init__(
        self,
        *,
        function: Callable[..., object],
        fixture_function_marker: FixtureFunctionMarker,
    ) -> None:
        self.name = fixture_function_marker.name or function.__name__
        self.__name__ = self.name
        self._fixture_function_marker = fixture_function_marker
        self._fixture_function = function

    def __get__(
        self,
        instance: object | None,
        owner: type | None = None,
    ):
        return self

    def __call__(self, *args: Any, **kwds: Any) -> Any:
        return self._fixture_function(*args, **kwds)

def fixture(
    fixture_function: Callable[..., object] | None = None,
    *,
    scope: str = 'function',
    name: str | None = None,
) -> FixtureFunctionMarker | FixtureFunctionDefinition:
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
