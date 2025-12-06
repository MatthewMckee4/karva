use divan::{bench, Bencher};
use karva_benchmark::walltime::{bench_project, warmup_project, ProjectBenchmark};
use karva_projects::real_world_projects::AFFECT_PROJECT;

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
