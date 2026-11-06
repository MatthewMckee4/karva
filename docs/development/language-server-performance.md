# Language-server measurements

Build a debug server and measure the generated fixture-heavy workspace:

```sh
cargo build -p karva
uv run scripts/lsp_benchmark.py --server target/debug/karva \
  --modules 50 --tests-per-module 20 --iterations 3 --edits 10 \
  --output lsp-report.json
```

Each iteration starts a fresh stdio server. The workload has shared `conftest.py`
fixtures and repeated test modules. Reports retain the source hash, byte and test
counts, server version, platform, raw samples, and median latencies. Initialization,
opening a document through diagnostics, first and warm completion, definition and
workspace symbol requests, and edits through versioned diagnostics are separate
measurements. Memory is resident bytes sampled after opening, workspace indexing,
and edits, rather than an estimate of peak usage. OS filesystem caches are not
cleared; “first” means first in this server process.

The PR Benchmarks workflow runs the same workload against both comparison wheels
and uploads `language-server-benchmarks`. Compare reports only when workload
parameters and source hashes match. Use release wheels for production baselines;
debug runs are useful for local scheduling investigations.

The checked-in `lsp-baseline.json` records an initial debug run on macOS ARM64.
It is a measurement receipt, not a regression threshold. Collect repeated A/A
runs on the CI runner before setting latency or memory tripwires.
