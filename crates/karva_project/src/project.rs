use ruff_python_ast::PythonVersion;

use crate::{
    path::{SystemPathBuf, TestPath, TestPathError},
    verbosity::VerbosityLevel,
};

#[derive(Default, Debug, Clone)]
pub struct ProjectMetadata {
    pub python_version: PythonVersion,
}

#[derive(Debug, Clone)]
pub struct ProjectOptions {
    pub test_prefix: String,
    pub verbosity: VerbosityLevel,
    pub show_output: bool,
}

impl Default for ProjectOptions {
    fn default() -> Self {
        Self {
            test_prefix: "test".to_string(),
            verbosity: VerbosityLevel::default(),
            show_output: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    cwd: SystemPathBuf,
    paths: Vec<SystemPathBuf>,
    pub metadata: ProjectMetadata,
    pub options: ProjectOptions,
}

impl Project {
    #[must_use]
    pub fn new(cwd: SystemPathBuf, paths: Vec<SystemPathBuf>) -> Self {
        Self {
            cwd,
            paths,
            metadata: ProjectMetadata::default(),
            options: ProjectOptions::default(),
        }
    }

    #[must_use]
    pub const fn with_metadata(mut self, metadata: ProjectMetadata) -> Self {
        self.metadata = metadata;
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
    pub fn test_paths(&self) -> Vec<Result<TestPath, TestPathError>> {
        self.paths.iter().map(TestPath::new).collect()
    }
}
