use camino::Utf8PathBuf;

use crate::{ProjectMetadata, ProjectSettings, TestPath, TestPathError, absolute};

#[derive(Debug, Clone)]
pub struct Project {
    settings: ProjectSettings,

    metadata: ProjectMetadata,

    is_verbose: bool,
}

impl Project {
    pub fn from_metadata(metadata: ProjectMetadata) -> Self {
        let settings = metadata.options.to_settings();
        Self {
            settings,
            metadata,
            is_verbose: false,
        }
    }

    #[must_use]
    pub fn with_settings(mut self, settings: ProjectSettings) -> Self {
        self.settings = settings;
        self
    }

    pub const fn settings(&self) -> &ProjectSettings {
        &self.settings
    }

    pub const fn cwd(&self) -> &Utf8PathBuf {
        self.metadata.root()
    }

    pub fn test_paths(&self) -> Vec<Result<TestPath, TestPathError>> {
        let mut discovered_paths: Vec<Utf8PathBuf> = self
            .settings
            .src()
            .include_paths
            .iter()
            .map(|p| absolute(p, self.cwd()))
            .collect();

        if discovered_paths.is_empty() {
            discovered_paths.push(self.cwd().clone());
        }

        let test_paths: Vec<Result<TestPath, TestPathError>> = discovered_paths
            .iter()
            .map(|p| TestPath::new(p.as_str()))
            .collect();

        test_paths
    }

    pub const fn is_verbose(&self) -> bool {
        self.is_verbose
    }

    pub const fn metadata(&self) -> &ProjectMetadata {
        &self.metadata
    }

    pub const fn set_verbose(&mut self, yes: bool) {
        self.is_verbose = yes;
    }
}
