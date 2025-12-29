use std::panic::RefUnwindSafe;
use std::sync::Arc;

use camino::Utf8PathBuf;
use karva_metadata::{ProjectMetadata, ProjectSettings};
use karva_system::{
    System,
    path::{TestPath, TestPathError, absolute},
};

pub trait Db: Send + Sync {
    fn system(&self) -> &dyn System;
    fn project(&self) -> &Project;
}

#[derive(Debug, Clone)]
pub struct ProjectDatabase {
    project: Project,

    system: Arc<dyn System + Send + Sync + RefUnwindSafe>,
}

impl ProjectDatabase {
    pub fn new<S>(project_metadata: ProjectMetadata, system: S) -> Self
    where
        S: System + 'static + Send + Sync + RefUnwindSafe,
    {
        Self {
            project: Project::from_metadata(project_metadata),
            system: Arc::new(system),
        }
    }
}

impl Db for ProjectDatabase {
    fn system(&self) -> &dyn System {
        self.system.as_ref()
    }

    fn project(&self) -> &Project {
        &self.project
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    settings: ProjectSettings,

    metadata: ProjectMetadata,
}

impl Project {
    pub fn from_metadata(metadata: ProjectMetadata) -> Self {
        let settings = metadata.options.to_settings();
        Self { settings, metadata }
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

    pub const fn metadata(&self) -> &ProjectMetadata {
        &self.metadata
    }
}
