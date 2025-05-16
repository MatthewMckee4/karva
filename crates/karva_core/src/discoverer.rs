use crate::path::PythonTestPath;
use crate::project::Project;

struct Discoverer {
    project: Project,
}

impl Discoverer {
    pub fn new(project: Project) -> Self {
        Self { project }
    }

    pub fn discover(&self) -> Vec<DiscoveredTest> {
        let mut discovered_tests = Vec::new();

        for path in self.project.paths() {}

        discovered_tests
    }
}

pub struct DiscoveredTest {
    path: PythonTestPath,
    function_name: String,
}
