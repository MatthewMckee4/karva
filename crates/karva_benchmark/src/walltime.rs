use divan::Bencher;
use karva_core::{TestRunner, testing::setup_module};
use karva_project::{
    path::absolute,
    project::{Project, ProjectOptions},
    verbosity::VerbosityLevel,
};
use karva_test::{InstalledProject, RealWorldProject};

pub struct ProjectBenchmark<'a> {
    installed_project: InstalledProject<'a>,
}

impl<'a> ProjectBenchmark<'a> {
    pub fn new(project: RealWorldProject<'a>) -> Self {
        let installed_project = project.setup(false).expect("Failed to setup project");
        Self { installed_project }
    }

    fn project(&self) -> Project {
        let test_paths = self.installed_project.config().paths;

        let absolute_test_paths = test_paths
            .iter()
            .map(|path| absolute(path, self.installed_project.path()))
            .collect();

        Project::new(
            self.installed_project.path().to_path_buf(),
            absolute_test_paths,
        )
        .with_options(ProjectOptions::new(
            "test".to_string(),
            VerbosityLevel::Default,
            false,
            true,
            false,
            false,
        ))
    }
}

pub fn bench_project(bencher: Bencher, benchmark: &ProjectBenchmark) {
    fn test_project(project: &Project) {
        let result = project.test();

        assert!(result.stats().total() > 0, "{:#?}", result.diagnostics());
    }

    setup_module();

    bencher
        .with_inputs(|| benchmark.project())
        .bench_local_refs(|db| test_project(db));
}
