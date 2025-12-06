use divan::{Bencher, bench};
use karva_benchmark::walltime::{ProjectBenchmark, bench_project};
use karva_projects::real_world_projects::TYPER_PROJECT;

#[bench(sample_size = 1, sample_count = 3)]
fn typer(bencher: Bencher) {
    let benchmark = ProjectBenchmark::new(TYPER_PROJECT.clone());

    bench_project(bencher, &benchmark);
}

fn main() {
    divan::main();
}
