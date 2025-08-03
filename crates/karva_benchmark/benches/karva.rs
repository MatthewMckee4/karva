use anyhow::Context;
use karva_benchmark::{
    FIXTURES, LARGE_LIST_COMPREHENSION, LARGE_SUMMATION, MATH, PARAMETRIZE, STRING_CONCATENATION,
    TRUE_ASSERTIONS, TestCase,
    criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main},
    real_world_projects::{InstalledProject, RealWorldProject},
};
use karva_core::{DummyReporter, TestRunner, testing::setup_module};
use karva_project::{
    path::{SystemPathBuf, absolute},
    project::{Project, ProjectOptions},
    verbosity::VerbosityLevel,
};
use ruff_python_ast::PythonVersion;

fn create_test_cases() -> Vec<TestCase> {
    vec![
        TestCase::new(TRUE_ASSERTIONS.clone()),
        TestCase::new(MATH.clone()),
        TestCase::new(STRING_CONCATENATION.clone()),
        TestCase::new(LARGE_SUMMATION.clone()),
        TestCase::new(LARGE_LIST_COMPREHENSION.clone()),
        TestCase::new(FIXTURES.clone()),
        TestCase::new(PARAMETRIZE.clone()),
    ]
}

fn benchmark_karva(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("karva");

    group.sample_size(10);

    setup_module();

    let root = {
        let env_cwd = std::env::current_dir()
            .context("Failed to get the current working directory")
            .unwrap();
        env_cwd.parent().unwrap().parent().unwrap().to_path_buf()
    };

    for case in create_test_cases() {
        group.throughput(Throughput::Bytes(case.code().len() as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(case.name()),
            &case,
            |b, case| {
                b.iter(|| {
                    let cwd = absolute(case.path().parent().unwrap(), &root);
                    let project = Project::new(cwd.clone(), [absolute(case.name(), &cwd)].to_vec());
                    let runner_result = project.test_with_reporter(&mut DummyReporter);
                    assert!(runner_result.passed());
                });
            },
        );
    }

    group.finish();
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
        let result = project.test_with_reporter(&mut DummyReporter);

        assert!(result.stats().total() > 0, "{:#?}", result.diagnostics());
    }

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
    let benchmark = ProjectBenchmark::new(RealWorldProject {
        name: "affect",
        repository: "https://github.com/MatthewMckee4/affect",
        commit: "803cc916b492378a8ad8966e747cac3325e11b5f",
        paths: vec![SystemPathBuf::from("tests")],
        dependencies: vec!["pydantic", "pydantic-settings", "pytest"],
        python_version: PythonVersion::PY313,
    });

    bench_project(&benchmark, criterion);
}

criterion_group!(karva, benchmark_karva);
criterion_group!(project, affect);

criterion_main!(karva, project);
