use camino::Utf8PathBuf;
use ruff_db::diagnostic::DiagnosticFormat;
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
#[repr(transparent)]
pub struct TestPrefix(String);

impl TestPrefix {
    pub const fn new(prefix: String) -> Self {
        Self(prefix)
    }
}

impl Default for TestPrefix {
    fn default() -> Self {
        Self("test".to_string())
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Default)]
pub struct ProjectOptions {
    test_prefix: TestPrefix,
    verbosity: VerbosityLevel,
    show_output: bool,
    no_ignore: bool,
    fail_fast: bool,
    try_import_fixtures: bool,
    show_traceback: bool,
    diagnostic_format: DiagnosticFormat,
    no_progress: bool,
}

impl ProjectOptions {
    #[allow(clippy::fn_params_excessive_bools)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        test_prefix: TestPrefix,
        verbosity: VerbosityLevel,
        show_output: bool,
        no_ignore: bool,
        fail_fast: bool,
        try_import_fixtures: bool,
        show_traceback: bool,
        diagnostic_format: impl Into<DiagnosticFormat>,
        no_progress: bool,
    ) -> Self {
        Self {
            test_prefix,
            verbosity,
            show_output,
            no_ignore,
            fail_fast,
            try_import_fixtures,
            show_traceback,
            diagnostic_format: diagnostic_format.into(),
            no_progress,
        }
    }

    pub fn test_prefix(&self) -> &str {
        &self.test_prefix.0
    }

    #[must_use]
    pub fn with_test_prefix(mut self, test_prefix: &str) -> Self {
        self.test_prefix = TestPrefix(test_prefix.to_string());
        self
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

    #[must_use]
    pub const fn with_no_ignore(mut self, no_ignore: bool) -> Self {
        self.no_ignore = no_ignore;
        self
    }

    pub const fn fail_fast(&self) -> bool {
        self.fail_fast
    }

    #[must_use]
    pub const fn with_fail_fast(mut self, fail_fast: bool) -> Self {
        self.fail_fast = fail_fast;
        self
    }

    pub const fn try_import_fixtures(&self) -> bool {
        self.try_import_fixtures
    }

    #[must_use]
    pub const fn with_try_import_fixtures(mut self, try_import_fixtures: bool) -> Self {
        self.try_import_fixtures = try_import_fixtures;
        self
    }

    pub const fn show_traceback(&self) -> bool {
        self.show_traceback
    }

    #[must_use]
    pub const fn with_show_traceback(mut self, show_traceback: bool) -> Self {
        self.show_traceback = show_traceback;
        self
    }

    pub const fn diagnostic_format(&self) -> DiagnosticFormat {
        self.diagnostic_format
    }

    pub const fn no_progress(&self) -> bool {
        self.no_progress
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    cwd: Utf8PathBuf,
    paths: Vec<String>,
    metadata: ProjectMetadata,
    options: ProjectOptions,
}

impl Project {
    pub fn new(cwd: Utf8PathBuf, paths: Vec<Utf8PathBuf>) -> Self {
        Self {
            cwd,
            paths: paths.into_iter().map(|p| p.to_string()).collect(),
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

    pub const fn cwd(&self) -> &Utf8PathBuf {
        &self.cwd
    }

    pub fn test_paths(&self) -> Vec<Result<TestPath, TestPathError>> {
        self.paths.iter().map(|p| TestPath::new(p)).collect()
    }
}
