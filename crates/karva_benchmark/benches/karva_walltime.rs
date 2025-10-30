use std::sync::Once;

use karva_benchmark::{
    InstalledProject, RealWorldProject, affect_project,
    criterion::{BatchSize, Criterion, criterion_group, criterion_main},
};
use karva_core::{TestRunner, testing::setup_module};
use karva_project::{
    path::absolute,
    project::{Project, ProjectOptions},
    verbosity::VerbosityLevel,
};

static SETUP_MODULE_ONCE: Once = Once::new();

fn setup_module_once() {
    SETUP_MODULE_ONCE.call_once(|| {
        setup_module();
    });
}

struct ProjectBenchmark<'a> {
    installed_project: InstalledProject<'a>,
}

impl<'a> ProjectBenchmark<'a> {
    fn new(project: RealWorldProject<'a>) -> Self {
        let installed_project = project.setup().expect("Failed to setup project");
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
        ))
    }
}

fn bench_project(benchmark: &ProjectBenchmark, criterion: &mut Criterion) {
    fn test_project(project: &Project) {
        let result = project.test();

        assert!(result.stats().total() > 0, "{:#?}", result.diagnostics());
    }

    setup_module_once();

    let mut group = criterion.benchmark_group("project");

    group.sampling_mode(karva_benchmark::criterion::SamplingMode::Flat);
    group.bench_function(benchmark.installed_project.config().name, |b| {
        b.iter_batched_ref(
            || benchmark.project(),
            |db| test_project(db),
            BatchSize::SmallInput,
        );
    });
}

fn affect(criterion: &mut Criterion) {
    let benchmark = ProjectBenchmark::new(affect_project());
    bench_project(&benchmark, criterion);
}

criterion_group!(project, affect);

criterion_main!(project);
