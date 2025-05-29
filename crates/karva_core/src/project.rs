use crate::path::{PythonTestPath, SystemPathBuf};

pub struct Project {
    cwd: SystemPathBuf,
    paths: Vec<PythonTestPath>,
    test_prefix: String,
}

impl Project {
    #[must_use]
    pub const fn new(cwd: SystemPathBuf, paths: Vec<PythonTestPath>, test_prefix: String) -> Self {
        Self {
            cwd,
            paths,
            test_prefix,
        }
    }

    #[must_use]
    pub const fn cwd(&self) -> &SystemPathBuf {
        &self.cwd
    }

    #[must_use]
    pub fn paths(&self) -> &[PythonTestPath] {
        &self.paths
    }

    #[must_use]
    pub fn test_prefix(&self) -> &str {
        &self.test_prefix
    }
}
