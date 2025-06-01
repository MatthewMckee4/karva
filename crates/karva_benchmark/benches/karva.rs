use anyhow::{Context, anyhow};
use karva_benchmark::{
    LARGE_LIST_COMPREHENSION, LARGE_SUMMATION, MATH, STRING_CONCATENATION, TRUE_ASSERTIONS,
    TestCase,
    criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main},
};
use karva_core::{diagnostic::MainDiagnosticWriter, runner::Runner};
use karva_project::{
    path::{SystemPath, SystemPathBuf},
    project::Project,
};

fn create_test_cases() -> Vec<TestCase> {
    vec![
        TestCase::fast(TRUE_ASSERTIONS.clone()),
        TestCase::fast(MATH.clone()),
        TestCase::normal(STRING_CONCATENATION.clone()),
        TestCase::normal(LARGE_SUMMATION.clone()),
        TestCase::slow(LARGE_LIST_COMPREHENSION.clone()),
    ]
}

fn benchmark_karva(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("karva");

    let cwd = {
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
                    let mut diagnostics = MainDiagnosticWriter::default();
                    let project = Project::new(
                        cwd.clone(),
                        [SystemPath::absolute(
                            SystemPathBuf::from_path_buf(case.path()).unwrap(),
                            &cwd,
                        )
                        .as_str()
                        .to_string()]
                        .to_vec(),
                        "test".to_string(),
                    );
                    let mut runner = Runner::new(&project, &mut diagnostics);
                    let runner_result = runner.run();
                    assert!(runner_result.passed());
                });
            },
        );
    }

    group.finish();
}

criterion_group!(karva, benchmark_karva);
criterion_main!(karva);
