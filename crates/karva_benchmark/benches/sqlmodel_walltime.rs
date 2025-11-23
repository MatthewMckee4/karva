use divan::{Bencher, bench};
use karva_benchmark::walltime::{ProjectBenchmark, bench_project};
use karva_test::real_world_projects::SQLMODEL_PROJECT;

#[bench(sample_size = 1, sample_count = 3)]
fn sqlmodel(bencher: Bencher) {
    let benchmark = ProjectBenchmark::new(SQLMODEL_PROJECT.clone());

    bench_project(bencher, &benchmark);
}

fn main() {
    divan::main();
}
