use std::path::PathBuf;

use ruff_python_ast::PythonVersion;

use crate::{
    path::{TestPath, TestPathError},
    verbosity::VerbosityLevel,
};

#[derive(Default, Debug, Clone)]
pub struct ProjectMetadata {
    python_version: PythonVersion,
}

impl ProjectMetadata {
    pub const fn new(python_version: PythonVersion) -> Self {
        Self { python_version }
    }

    pub const fn python_version(&self) -> PythonVersion {
        self.python_version
    }
}

#[derive(Debug, Clone)]
pub struct ProjectOptions {
    test_prefix: String,
    verbosity: VerbosityLevel,
    show_output: bool,
    no_ignore: bool,
}

impl ProjectOptions {
    pub const fn new(
        test_prefix: String,
        verbosity: VerbosityLevel,
        show_output: bool,
        no_ignore: bool,
    ) -> Self {
        Self {
            test_prefix,
            verbosity,
            show_output,
            no_ignore,
        }
    }

    pub fn test_prefix(&self) -> &str {
        &self.test_prefix
    }

    pub const fn verbosity(&self) -> VerbosityLevel {
        self.verbosity
    }

    pub const fn show_output(&self) -> bool {
        self.show_output
    }

    pub const fn no_ignore(&self) -> bool {
        self.no_ignore
    }
}

impl Default for ProjectOptions {
    fn default() -> Self {
        Self {
            test_prefix: "test".to_string(),
            verbosity: VerbosityLevel::default(),
            show_output: false,
            no_ignore: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    cwd: PathBuf,
    paths: Vec<String>,
    metadata: ProjectMetadata,
    options: ProjectOptions,
}

impl Project {
    pub fn new(cwd: PathBuf, paths: Vec<PathBuf>) -> Self {
        Self {
            cwd,
            paths: paths.into_iter().map(|p| p.display().to_string()).collect(),
            metadata: ProjectMetadata::default(),
            options: ProjectOptions::default(),
        }
    }

    #[must_use]
    pub const fn with_metadata(mut self, metadata: ProjectMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    pub const fn metadata(&self) -> &ProjectMetadata {
        &self.metadata
    }

    #[must_use]
    pub fn with_options(mut self, options: ProjectOptions) -> Self {
        self.options = options;
        self
    }

    pub const fn options(&self) -> &ProjectOptions {
        &self.options
    }

    pub const fn cwd(&self) -> &PathBuf {
        &self.cwd
    }

    pub fn test_paths(&self) -> Vec<Result<TestPath, TestPathError>> {
        self.paths.iter().map(|p| TestPath::new(p)).collect()
    }
}
