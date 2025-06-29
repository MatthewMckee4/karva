use ruff_python_ast::PythonVersion;

use crate::{
    path::{SystemPathBuf, TestPath, TestPathError},
    verbosity::VerbosityLevel,
};

#[derive(Default, Debug, Clone)]
pub struct ProjectMetadata {
    python_version: PythonVersion,
}

impl ProjectMetadata {
    #[must_use]
    pub const fn new(python_version: PythonVersion) -> Self {
        Self { python_version }
    }

    #[must_use]
    pub const fn python_version(&self) -> PythonVersion {
        self.python_version
    }
}

#[derive(Debug, Clone)]
pub struct ProjectOptions {
    test_prefix: String,
    verbosity: VerbosityLevel,
    show_output: bool,
}

impl ProjectOptions {
    #[must_use]
    pub const fn new(test_prefix: String, verbosity: VerbosityLevel, show_output: bool) -> Self {
        Self {
            test_prefix,
            verbosity,
            show_output,
        }
    }

    #[must_use]
    pub fn test_prefix(&self) -> &str {
        &self.test_prefix
    }

    #[must_use]
    pub const fn verbosity(&self) -> VerbosityLevel {
        self.verbosity
    }

    #[must_use]
    pub const fn show_output(&self) -> bool {
        self.show_output
    }
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
    metadata: ProjectMetadata,
    options: ProjectOptions,
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
    pub const fn metadata(&self) -> &ProjectMetadata {
        &self.metadata
    }

    #[must_use]
    pub fn with_options(mut self, options: ProjectOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub const fn options(&self) -> &ProjectOptions {
        &self.options
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
