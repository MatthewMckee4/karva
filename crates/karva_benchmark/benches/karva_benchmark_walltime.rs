use divan::{Bencher, bench};
use karva_benchmark::walltime::{ProjectBenchmark, bench_project, warmup_project};
use karva_projects::real_world_projects::KARVA_BENCHMARK_PROJECT;

#[bench(sample_size = 3, sample_count = 3)]
fn karva_benchmark(bencher: Bencher) {
    let benchmark = ProjectBenchmark::new(KARVA_BENCHMARK_PROJECT.clone());

    bench_project(bencher, &benchmark);
}

fn main() {
    let benchmark = ProjectBenchmark::new(KARVA_BENCHMARK_PROJECT.clone());

    warmup_project(&benchmark);

    divan::main();
}
