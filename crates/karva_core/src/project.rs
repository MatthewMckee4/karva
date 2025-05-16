use crate::path::PythonTestPath;

pub struct Project {
    paths: Vec<PythonTestPath>,
    test_prefix: String,
}

impl Project {
    pub fn new(paths: Vec<PythonTestPath>, test_prefix: String) -> Self {
        Self { paths, test_prefix }
    }

    pub fn paths(&self) -> &[PythonTestPath] {
        &self.paths
    }
}
