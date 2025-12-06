use divan::{Bencher, bench};
use karva_benchmark::walltime::{ProjectBenchmark, bench_project, warmup_project};
use karva_projects::real_world_projects::SQLMODEL_PROJECT;

#[bench(sample_size = 3, sample_count = 4)]
fn sqlmodel(bencher: Bencher) {
    let benchmark = ProjectBenchmark::new(SQLMODEL_PROJECT.clone());

    bench_project(bencher, &benchmark);
}

fn main() {
    let benchmark = ProjectBenchmark::new(SQLMODEL_PROJECT.clone());

    warmup_project(&benchmark);

    divan::main();
}
