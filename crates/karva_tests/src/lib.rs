use std::fs;

use karva_project::path::SystemPathBuf;
use tempfile::TempDir;

pub struct TestEnv {
    temp_dir: TempDir,
}

impl TestEnv {
    pub fn new() -> Self {
        Self {
            temp_dir: TempDir::new().expect("Failed to create temp directory"),
        }
    }

    pub fn create_file(&self, name: &str, content: &str) -> SystemPathBuf {
        let path = self.temp_dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        SystemPathBuf::from(path)
    }

    pub fn create_dir(&self, name: &str) -> SystemPathBuf {
        let path = self.temp_dir.path().join(name);
        fs::create_dir_all(&path).unwrap();
        SystemPathBuf::from(path)
    }

    pub fn temp_path(&self, name: &str) -> SystemPathBuf {
        SystemPathBuf::from(self.temp_dir.path().join(name))
    }

    pub fn cwd(&self) -> SystemPathBuf {
        SystemPathBuf::from(self.temp_dir.path())
    }
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}
