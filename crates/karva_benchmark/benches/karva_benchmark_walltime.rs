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

#[bench(sample_size = 2, sample_count = 1)]
fn h11(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::H11_PROJECT);
}

#[bench(sample_size = 2, sample_count = 1)]
fn markupsafe(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::MARKUPSAFE_PROJECT);
}

#[bench(sample_size = 2, sample_count = 1)]
fn sniffio(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::SNIFFIO_PROJECT);
}

#[bench(sample_size = 2, sample_count = 1)]
fn itsdangerous(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::ITSDANGEROUS_PROJECT);
}

#[bench(sample_size = 2, sample_count = 1)]
fn pyparsing(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::PYPARSING_PROJECT);
}

#[bench(sample_size = 2, sample_count = 1)]
fn blinker(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::BLINKER_PROJECT);
}

#[bench(sample_size = 2, sample_count = 1)]
fn jinja(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::JINJA_PROJECT);
}

#[bench(sample_size = 2, sample_count = 1)]
fn installer(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::INSTALLER_PROJECT);
}

#[bench(sample_size = 2, sample_count = 1)]
fn tomlkit(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::TOMLKIT_PROJECT);
}

#[bench(sample_size = 2, sample_count = 1)]
fn outcome(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::OUTCOME_PROJECT);
}

fn main() {
    divan::main();
}
