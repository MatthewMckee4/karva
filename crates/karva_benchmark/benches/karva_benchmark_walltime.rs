use divan::{Bencher, bench};
use karva_benchmark::real_world_projects::KARVA_BENCHMARK_PROJECT;
use karva_benchmark::walltime::{ProjectBenchmark, bench_project, warmup_project};

#[bench(sample_size = 2, sample_count = 2)]
fn karva_benchmark(bencher: Bencher) {
    let benchmark = ProjectBenchmark::new(KARVA_BENCHMARK_PROJECT.clone());

    bench_project(bencher, &benchmark);
}

fn main() {
    let benchmark = ProjectBenchmark::new(KARVA_BENCHMARK_PROJECT.clone());

    warmup_project(&benchmark);

    divan::main();
}
