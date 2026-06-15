use divan::{Bencher, bench};

#[bench(sample_size = 2, sample_count = 1)]
fn karva_benchmark(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::SYNTHETIC_PROJECT);
}

#[bench(sample_size = 2, sample_count = 1)]
fn packaging(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::PACKAGING_PROJECT);
}

#[bench(sample_size = 2, sample_count = 1)]
fn parse(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::PARSE_PROJECT);
}

fn main() {
    divan::main();
}
