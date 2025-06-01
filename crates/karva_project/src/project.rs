use ruff_python_ast::PythonVersion;

use crate::path::{PythonTestPath, PythonTestPathError, SystemPathBuf};

#[derive(Default)]
pub struct ProjectMetadata {
    pub python_version: PythonVersion,
}

pub struct ProjectOptions {
    pub test_prefix: String,
    pub watch: bool,
}

impl Default for ProjectOptions {
    fn default() -> Self {
        Self {
            test_prefix: "test".to_string(),
            watch: false,
        }
    }
}

pub struct Project {
    cwd: SystemPathBuf,
    paths: Vec<String>,
    metadata: ProjectMetadata,
    options: ProjectOptions,
}

impl Project {
    #[must_use]
    pub fn new(cwd: SystemPathBuf, paths: Vec<String>) -> Self {
        Self {
            cwd,
            paths,
            metadata: ProjectMetadata::default(),
            options: ProjectOptions::default(),
        }
    }

    #[must_use]
    pub const fn with_metadata(mut self, config: ProjectMetadata) -> Self {
        self.metadata = config;
        self
    }

    #[must_use]
    pub fn with_options(mut self, options: ProjectOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub const fn cwd(&self) -> &SystemPathBuf {
        &self.cwd
    }

    #[must_use]
    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    #[must_use]
    pub fn python_test_paths(&self) -> Vec<Result<PythonTestPath, PythonTestPathError>> {
        self.paths.iter().map(PythonTestPath::new).collect()
    }

    #[must_use]
    pub fn test_prefix(&self) -> &str {
        &self.options.test_prefix
    }

    #[must_use]
    pub const fn python_version(&self) -> &PythonVersion {
        &self.metadata.python_version
    }
}
