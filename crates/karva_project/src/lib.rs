use std::{panic::RefUnwindSafe, sync::Arc};

use camino::Utf8PathBuf;
pub use envs::{EnvVars, max_parallelism};
pub use metadata::{
    ProjectMetadata,
    options::{
        Options, OutputFormat, ProjectOptionsOverrides, SrcOptions, TerminalOptions, TestOptions,
    },
    settings::ProjectSettings,
};
pub use path::{TestPath, TestPathError, absolute};
pub use project::Project;
pub use system::{OsSystem, System};
pub use utils::module_name;
pub use verbosity::VerbosityLevel;

mod envs;
mod metadata;
mod path;
mod project;
mod system;
mod utils;
mod verbosity;

use ruff_db::{
    diagnostic::{FileResolver, Input, UnifiedFile},
    files::File,
};
use ruff_notebook::NotebookIndex;

pub const KARVA_CONFIG_FILE_NAME: &str = "karva.toml";

pub trait Db: FileResolver + Send + Sync {
    fn system(&self) -> &dyn System;
    fn project(&self) -> &Project;
    fn project_mut(&mut self) -> &mut Project;
}

#[derive(Debug, Clone)]
pub struct ProjectDatabase {
    project: Option<Project>,

    system: Arc<dyn System + Send + Sync + RefUnwindSafe>,
}

impl ProjectDatabase {
    pub fn new<S>(project_metadata: ProjectMetadata, system: S) -> anyhow::Result<Self>
    where
        S: System + 'static + Send + Sync + RefUnwindSafe,
    {
        let mut db = Self {
            project: None,
            system: Arc::new(system),
        };

        db.project = Some(Project::from_metadata(project_metadata));

        Ok(db)
    }

    pub fn test_db(cwd: Utf8PathBuf, paths: &[Utf8PathBuf]) -> Self {
        let options = Options {
            src: Some(SrcOptions {
                include: Some(paths.iter().map(ToString::to_string).collect()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let metadata = ProjectMetadata {
            root: cwd.clone(),
            options,
            ..Default::default()
        };
        let system = OsSystem::new(cwd);
        Self::new(metadata, system).unwrap()
    }
}

impl Db for ProjectDatabase {
    fn system(&self) -> &dyn System {
        self.system.as_ref()
    }

    fn project(&self) -> &Project {
        self.project.as_ref().unwrap()
    }

    fn project_mut(&mut self) -> &mut Project {
        self.project.as_mut().unwrap()
    }
}

impl FileResolver for ProjectDatabase {
    fn path(&self, _file: File) -> &str {
        unimplemented!("Expected a Ruff file for rendering a Ruff diagnostic");
    }

    fn input(&self, _file: File) -> Input {
        unimplemented!("Expected a Ruff file for rendering a Ruff diagnostic");
    }

    fn notebook_index(&self, _file: &UnifiedFile) -> Option<NotebookIndex> {
        None
    }

    fn is_notebook(&self, _file: &UnifiedFile) -> bool {
        false
    }

    fn current_directory(&self) -> &std::path::Path {
        self.system.current_directory().as_std_path()
    }
}
