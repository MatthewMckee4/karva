use karva_benchmark::{
    criterion::{Criterion, criterion_group, criterion_main},
    walltime::{ProjectBenchmark, bench_project},
};

fn sqlmodel(criterion: &mut Criterion) {
    use karva_test::real_world_projects::sqlmodel_project;

    let benchmark = ProjectBenchmark::new(sqlmodel_project());

    bench_project(&benchmark, criterion);
}

criterion_group!(project, sqlmodel);

criterion_main!(project);
