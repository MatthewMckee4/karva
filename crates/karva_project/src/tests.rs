use std::fs;

use tempfile::TempDir;

use crate::path::SystemPathBuf;

pub struct TestEnv {
    temp_dir: TempDir,
}

impl TestEnv {
    #[must_use]
    pub fn new() -> Self {
        Self {
            temp_dir: TempDir::new().expect("Failed to create temp directory"),
        }
    }

    #[allow(clippy::must_use_candidate)]
    pub fn create_file(&self, name: &str, content: &str) -> SystemPathBuf {
        let path = self.temp_dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        SystemPathBuf::from(path)
    }

    #[must_use]
    pub fn create_dir(&self, name: &str) -> SystemPathBuf {
        let path = self.temp_dir.path().join(name);
        fs::create_dir_all(&path).unwrap();
        SystemPathBuf::from(path)
    }

    #[must_use]
    pub fn temp_path(&self, name: &str) -> SystemPathBuf {
        SystemPathBuf::from(self.temp_dir.path().join(name))
    }

    #[must_use]
    pub fn cwd(&self) -> SystemPathBuf {
        SystemPathBuf::from(self.temp_dir.path())
    }
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}
