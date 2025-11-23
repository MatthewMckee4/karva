use divan::{Bencher, bench};
use karva_benchmark::walltime::{ProjectBenchmark, bench_project};
use karva_test::real_world_projects::AFFECT_PROJECT;

#[bench(sample_size = 2, sample_count = 3)]
fn affect(bencher: Bencher) {
    let benchmark = ProjectBenchmark::new(AFFECT_PROJECT.clone());

    bench_project(bencher, &benchmark);
}

fn main() {
    divan::main();
}
