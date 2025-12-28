use std::panic::RefUnwindSafe;
use std::sync::Arc;

use camino::Utf8PathBuf;
use ruff_db::diagnostic::{FileResolver, Input, UnifiedFile};
use ruff_db::files::File;
use ruff_notebook::NotebookIndex;

use karva_metadata::{Options, ProjectMetadata, SrcOptions};
use karva_system::{OsSystem, System};

pub use project::Project;

mod project;

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
