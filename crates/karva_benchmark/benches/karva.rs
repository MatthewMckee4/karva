use anyhow::{Context, anyhow};
use karva_benchmark::{
    FIXTURES, LARGE_LIST_COMPREHENSION, LARGE_SUMMATION, MATH, PARAMETRIZE, STRING_CONCATENATION,
    TRUE_ASSERTIONS, TestCase,
    criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main},
};
use karva_core::{diagnostic::reporter::DummyReporter, runner::TestRunner, testing::setup_module};
use karva_project::{
    path::{SystemPath, SystemPathBuf},
    project::Project,
};

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

    setup_module();

    let root = {
        let env_cwd = std::env::current_dir()
            .context("Failed to get the current working directory")
            .unwrap();
        let cwd = env_cwd.parent().unwrap().parent().unwrap();
        SystemPathBuf::from_path_buf(cwd.to_path_buf())
            .map_err(|path| {
                anyhow!(
                    "The current working directory `{}` contains non-Unicode characters. Karva only supports Unicode paths.",
                    path.display()
                )
            }).unwrap()
    };

    for case in create_test_cases() {
        group.throughput(Throughput::Bytes(case.code().len() as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(case.name()),
            &case,
            |b, case| {
                b.iter(|| {
                    let cwd = SystemPath::absolute(
                        SystemPathBuf::from_path_buf(case.path().parent().unwrap().to_path_buf())
                            .unwrap(),
                        &root,
                    );
                    let project = Project::new(
                        cwd.clone(),
                        [SystemPath::absolute(
                            SystemPathBuf::from_path_buf(case.name().into()).unwrap(),
                            &cwd,
                        )]
                        .to_vec(),
                    );
                    let runner_result = project.test_with_reporter(&mut DummyReporter);
                    assert!(runner_result.is_empty());
                });
            },
        );
    }

    group.finish();
}

criterion_group!(karva, benchmark_karva);
criterion_main!(karva);
