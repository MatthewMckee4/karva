use karva_benchmark::{
    criterion::{Criterion, criterion_group, criterion_main},
    walltime::{ProjectBenchmark, bench_project},
};

fn affect(criterion: &mut Criterion) {
    use karva_test::real_world_projects::affect_project;

    let benchmark = ProjectBenchmark::new(affect_project());

    bench_project(&benchmark, criterion);
}

criterion_group!(project, affect);

criterion_main!(project);
