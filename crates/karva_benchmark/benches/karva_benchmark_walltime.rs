use divan::{Bencher, bench};

#[bench(sample_size = 2, sample_count = 1)]
fn karva_benchmark_1(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::SYNTHETIC_PROJECT);
}

#[bench(sample_size = 2, sample_count = 1)]
fn requests(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::REQUESTS_PROJECT);
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

#[bench(sample_size = 2, sample_count = 1)]
fn pluggy(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::PLUGGY_PROJECT);
}

#[bench(sample_size = 2, sample_count = 1)]
fn werkzeug(bencher: Bencher) {
    karva_benchmark::bench_project(bencher, &karva_benchmark::WERKZEUG_PROJECT);
}

fn main() {
    divan::main();
}
