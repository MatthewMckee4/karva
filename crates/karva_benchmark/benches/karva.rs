use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use karva_benchmark::{
    LARGE_LIST_COMPREHENSION, LARGE_SUMMATION, MATH, STRING_CONCATENATION, TRUE_ASSERTIONS,
    TestCase,
};
use karva_core::{
    diagnostics::DiagnosticWriter,
    path::{PythonTestPath, SystemPathBuf},
    project::Project,
    runner::Runner,
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

fn benchmark_test_runner(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("test_runner");

    let cwd = SystemPathBuf::from_path_buf(
        PathBuf::from(file!())
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("resources"),
    )
    .unwrap();

    for case in create_test_cases() {
        group.throughput(Throughput::Bytes(case.code().len() as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(case.name()),
            &case,
            |b, case| {
                b.iter(|| {
                    let diagnostics = DiagnosticWriter::default();
                    let project = Project::new(
                        cwd.clone(),
                        [PythonTestPath::File(
                            SystemPathBuf::from_path_buf(case.path()).unwrap(),
                        )]
                        .into(),
                        "test".to_string(),
                    );
                    let mut runner = Runner::new(&project, diagnostics);
                    let runner_result = runner.run();
                    assert!(runner_result.passed());
                });
            },
        );
    }

    group.finish();
}

criterion_group!(test_runner, benchmark_test_runner);
criterion_main!(test_runner);
