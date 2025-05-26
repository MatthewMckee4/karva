use crate::core::path::{PythonTestPath, SystemPathBuf};

pub struct Project {
    cwd: SystemPathBuf,
    paths: Vec<PythonTestPath>,
    test_prefix: String,
}

impl Project {
    pub fn new(cwd: SystemPathBuf, paths: Vec<PythonTestPath>, test_prefix: String) -> Self {
        Self {
            cwd,
            paths,
            test_prefix,
        }
    }

    pub fn cwd(&self) -> &SystemPathBuf {
        &self.cwd
    }

    pub fn paths(&self) -> &[PythonTestPath] {
        &self.paths
    }

    pub fn test_prefix(&self) -> &str {
        &self.test_prefix
    }
}
