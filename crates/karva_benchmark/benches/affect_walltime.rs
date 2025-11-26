use divan::{Bencher, bench};
use karva_benchmark::walltime::{ProjectBenchmark, bench_project, warmup_project};
use karva_test::real_world_projects::AFFECT_PROJECT;

#[bench(sample_size = 4, sample_count = 5)]
fn affect(bencher: Bencher) {
    let benchmark = ProjectBenchmark::new(AFFECT_PROJECT.clone());

    bench_project(bencher, &benchmark);
}

fn main() {
    let benchmark = ProjectBenchmark::new(AFFECT_PROJECT.clone());

    warmup_project(&benchmark);

    divan::main();
}
