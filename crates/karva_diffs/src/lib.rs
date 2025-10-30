//! Diagnostic diff testing for Karva on real-world projects.
//!
//! This crate tracks diagnostic changes across different versions of Karva
//! by running tests on real-world Python projects and comparing the
//! diagnostics output. This is similar to `mypy_primer` but focused on pytest
//! support tracking.

use karva_core::TestRunner;
use karva_project::{
    path::absolute,
    project::{Project, ProjectOptions},
    verbosity::VerbosityLevel,
};
// Re-export project registry from karva_test
pub use karva_test::get_real_world_projects;
use karva_test::{InstalledProject, RealWorldProject};

/// Helper function to create a Project from an `InstalledProject`
#[must_use]
pub fn create_project(installed: &InstalledProject) -> Project {
    let test_paths = installed.config().paths.clone();

    let absolute_test_paths = test_paths
        .iter()
        .map(|path| absolute(path, installed.path()))
        .collect();

    Project::new(installed.path().to_path_buf(), absolute_test_paths).with_options(
        ProjectOptions::new("test".to_string(), VerbosityLevel::Default, false, true),
    )
}

/// Serializable diagnostic summary for a single project
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ProjectDiagnostics {
    pub project_name: String,
    pub total_tests: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub error_count: usize,
    pub warning_count: usize,
}

/// Complete diagnostic report for all projects
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DiagnosticReport {
    pub projects: Vec<ProjectDiagnostics>,
}

impl DiagnosticReport {
    /// Create a new empty report
    #[must_use]
    pub const fn new() -> Self {
        Self {
            projects: Vec::new(),
        }
    }

    /// Add a project's diagnostics to the report
    pub fn add_project(&mut self, diagnostics: ProjectDiagnostics) {
        self.projects.push(diagnostics);
    }

    /// Serialize to JSON string
    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Create from JSON string
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

impl Default for DiagnosticReport {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectDiagnostics {
    /// Create diagnostics from a test run result
    #[must_use]
    pub fn from_test_result(
        project_name: String,
        result: &karva_core::runner::diagnostic::TestRunResult,
    ) -> Self {
        let stats = result.stats();
        let mut error_count = 0;
        let mut warning_count = 0;

        for diagnostic in result.diagnostics() {
            if diagnostic.severity().is_error() {
                error_count += 1;
            } else {
                warning_count += 1;
            }
        }

        Self {
            project_name,
            total_tests: stats.total(),
            passed: stats.passed(),
            failed: stats.failed(),
            skipped: stats.skipped(),
            error_count,
            warning_count,
        }
    }
}

/// Run diagnostics on a project and return the results
pub fn run_project_diagnostics(project: RealWorldProject) -> anyhow::Result<ProjectDiagnostics> {
    let project_name = project.name.to_string();

    // Setup the project (clone, install dependencies)
    let installed = project.setup()?;

    // Create and run the project
    let project = create_project(&installed);
    let result = project.test();

    // Create diagnostic summary
    Ok(ProjectDiagnostics::from_test_result(project_name, &result))
}
