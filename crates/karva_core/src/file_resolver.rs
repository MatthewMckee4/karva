use ruff_db::{
    diagnostic::{FileResolver, Input, UnifiedFile},
    files::File,
};
use ruff_notebook::NotebookIndex;

pub struct DefaultFileResolver {
    cwd: std::path::PathBuf,
}

impl DefaultFileResolver {
    pub const fn new(cwd: std::path::PathBuf) -> Self {
        Self { cwd }
    }
}

impl FileResolver for DefaultFileResolver {
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
        &self.cwd
    }
}
