# Generated workload measurements

The five workloads added on 2026-10-11 target failure diagnostics, yield-fixture
cleanup, coroutine execution, snapshot mismatches, and Python output capture.
The PR benchmark matrix runs them automatically. Failures, snapshot diffs, and
captured output use full diagnostics; fixture teardown and async tests retain
concise output. Every run must produce its expected test totals and exit code. Full diagnostic
comparisons preserve each test’s source context and captured output associations.

## Workload sizes and repetition counts

| Project | Workload | Median wall time | Peak RSS | Runs per wheel | Paired A/A 99% interval |
| --- | --- | ---: | ---: | ---: | ---: |
| `karva-failures` | 4,096 failures, alternating local assertions and an eight-helper chain | 3.541 s | 49.5 MiB | 21 | -3.14% to +4.59% |
| `karva-fixture-teardown` | 16,384 tests using eight function yield fixtures and module/package/session yield fixtures | 0.361 s | 111.7 MiB | 21 | -3.21% to +3.04% |
| `karva-async` | 4,096 coroutine tests using a coroutine fixture and an async-generator fixture | 0.820 s | 53.0 MiB | 31 | -1.02% to +4.67% |
| `karva-snapshot-diffs` | 4,096 mismatches of a 2,048-line inline snapshot, changing its middle line | 2.561 s | 49.0 MiB | 7 | -1.59% to +2.43% |
| `karva-captured-output` | 4,096 tests, half failing, writing eight 260-byte chunks to each Python stream | 1.429 s | 60.5 MiB | 7 | -3.47% to +3.08% |

The failure workload's seven-run interval reached +9.95%; fifteen runs still
reached the existing 5% material-change threshold. Async remained above that
threshold with twenty-one runs (+5.98%), so it receives thirty-one. Seven runs
already put snapshot diffs and captured output below the threshold. Teardown
keeps the inexpensive existing twenty-one-run tier for additional noise margin.
Each count applies separately to the baseline and candidate wheels.

These receipts come from workload-equivalent Python prototypes on an Apple M4
Pro running arm64 macOS, Python 3.13.14, Rust 1.98.1, and a release wheel built
from `55a7266c`. The Rust-generated projects were verified against the
prototype Python ASTs and run through the harness warm-up and expected-outcome
validation. They size the workloads and establish initial repetition counts;
Linux CI remains the authoritative comparison environment. Async awaits immediate
coroutines without sleeps or external I/O. Teardown and async tests check live
fixture counts, so missed cleanup causes failures. Nonempty inline expectations
keep snapshot mismatches from creating pending files or rewriting test source.

## Sampling and reproduction

Each project was warmed with one worker. Its cache was restored before every
four-worker measurement, as in the comparison harness. Both A/A subjects used
the same wheel, with alternating first/second roles. Twenty-one pairs were
recorded for each workload; async sampling was extended to thirty-one pairs.
All samples, including timing spikes, were included. Reported wall time and RSS
are medians of those samples.

For each proposed repetition count, resample whole A/A pairs with replacement,
calculate the percentage change between the two subject medians, and take the
central 99% interval. Calibration evaluated seven, fifteen,
twenty-one, and thirty-one repetitions using Python's `random.Random(0)`,
10,000 resamples, and sorted indices `50` and `-51`. Pairing retains the shared
runner conditions instead of treating adjacent measurements as independent.
This is a noise estimate from the measured sample, not a guarantee for every
machine; remeasure when runner conditions or workload shapes change.

On Linux, the normal harness can repeat an A/A comparison using the same wheel
for both subjects. For example, set `PROJECT`, `ITERATIONS`, and `KARVA_WHEEL`
to the desired project, its matrix count, and an optimized wheel:

```sh
cargo build -p karva_benchmark --bin karva-benchmark --release
target/release/karva-benchmark compare \
  --baseline-label A --candidate-label A \
  --baseline-wheel "$KARVA_WHEEL" --candidate-wheel "$KARVA_WHEEL" \
  --project "$PROJECT" --iterations "$ITERATIONS" \
  --output-wall-time-json /tmp/karva-aa-time.json \
  --output-wall-time-markdown /tmp/karva-aa-time.md \
  --output-memory-json /tmp/karva-aa-memory.json \
  --output-memory-markdown /tmp/karva-aa-memory.md
```
