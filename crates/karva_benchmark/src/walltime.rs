use codspeed_criterion_compat::SamplingMode;
use karva_core::{TestRunner, testing::setup_module};
use karva_project::{
    path::absolute,
    project::{Project, ProjectOptions},
    verbosity::VerbosityLevel,
};
use karva_test::{InstalledProject, RealWorldProject};

use crate::criterion::{BatchSize, Criterion};

pub struct ProjectBenchmark<'a> {
    installed_project: InstalledProject<'a>,
}

impl<'a> ProjectBenchmark<'a> {
    pub fn new(project: RealWorldProject<'a>) -> Self {
        let installed_project = project.setup(false).expect("Failed to setup project");
        Self { installed_project }
    }

    fn project(&self) -> Project {
        let test_paths = self.installed_project.config().paths.clone();

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
        ))
    }
}

pub fn bench_project(
    benchmark: &ProjectBenchmark,
    criterion: &mut Criterion,
    batch_size: BatchSize,
) {
    fn test_project(project: &Project) {
        let result = project.test();

        assert!(result.stats().total() > 0, "{:#?}", result.diagnostics());
    }

    setup_module();

    let mut group = criterion.benchmark_group("project");

    group.sampling_mode(SamplingMode::Auto);
    group.bench_function(benchmark.installed_project.config().name, |b| {
        b.iter_batched_ref(|| benchmark.project(), |db| test_project(db), batch_size);
    });
}
