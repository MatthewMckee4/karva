use codspeed_criterion_compat::BatchSize;
use karva_benchmark::{
    criterion::{Criterion, criterion_group, criterion_main},
    walltime::{ProjectBenchmark, bench_project},
};

fn typer(criterion: &mut Criterion) {
    use karva_test::real_world_projects::typer_project;

    let benchmark = ProjectBenchmark::new(typer_project());

    bench_project(&benchmark, criterion, BatchSize::NumIterations(10));
}

criterion_group!(project, typer);

criterion_main!(project);
