use crate::path::{PythonTestPath, PythonTestPathError, SystemPathBuf};

pub struct Project {
    cwd: SystemPathBuf,
    paths: Vec<String>,
    test_prefix: String,
}

impl Project {
    #[must_use]
    pub const fn new(cwd: SystemPathBuf, paths: Vec<String>, test_prefix: String) -> Self {
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
    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    #[must_use]
    pub fn python_test_paths(&self) -> Vec<Result<PythonTestPath, PythonTestPathError>> {
        self.paths.iter().map(PythonTestPath::new).collect()
    }

    #[must_use]
    pub fn test_prefix(&self) -> &str {
        &self.test_prefix
    }
}
