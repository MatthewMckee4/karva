# Benchmarks

Use the built-in `benchmark` fixture to measure a synchronous callable. It
accepts positional and keyword arguments and returns the last result:

```python
import karva


@karva.tags.benchmark(group="parser")
def test_parse(benchmark):
    assert benchmark(int, "42", base=10) == 42
```

It also works as a decorator inside a test. The decorated name becomes the
callable's result:

```python
def test_parse(benchmark):
    @benchmark
    def result():
        return int("42")

    assert result == 42
```

Each fixture measures one callable. That callable must tolerate repeated
invocation. Ordinary fixtures run once per test attempt; their setup and
teardown are excluded from samples. Async callables are unsupported.
`pytest.mark.benchmark` accepts the same options as `karva.tags.benchmark`.
Module and class marks are inherited; local options take precedence.

## Measurement controls

Pass these keyword options to the benchmark tag:

| Option | Default | Meaning |
| --- | --- | --- |
| `group` | `None` | Comparison group shown in reports. |
| `warmup` | `1` | Untimed callable invocations before calibration; zero disables warmup. |
| `min_time` | `0.001` | Target seconds per calibration batch. |
| `max_time` | `0.1` | Soft time target across warmup, calibration, and measurement. |
| `min_rounds` | `5` | Minimum measurement rounds when adapting the round count. |
| `rounds` | `None` | Exact measurement rounds; overrides `min_rounds` and the time target. |
| `iterations` | `None` | Exact calls per round; otherwise calibrated by doubling the batch. |
| `disable_gc` | `False` | Disable cyclic garbage collection during sampling and restore it afterward. |

Time controls must be finite and positive, and `min_time` must not exceed
`max_time`. Counts must be positive integers, except `warmup`, which allows
zero. Minimum rounds take precedence over the soft time target. Karva finishes
each callable and batch; a slow callable can exceed that target. Use the
ordinary test timeout when you need a hard execution deadline.

For reproducible work counts, set `rounds`, `iterations`, and `warmup`
explicitly. For cheap correctness coverage, run:

```console
uv run karva test --benchmark-disable
```

This invokes each benchmark callable once and reports no timing statistics.
The same setting is available as `benchmark-disable = true` under
`[tool.karva.profile.default.test]` in `pyproject.toml`, or
`[profile.default.test]` in `karva.toml`.

## Results and isolation

Terminal output shows each test's group, minimum, median, mean, sample standard
deviation, warmup calls, calibration calls, rounds, iterations, total calls,
elapsed time, effective options, and stopping reason. Statistics describe
per-call round averages, rounded to nanoseconds. A single round has a standard
deviation of zero. Warmup and calibration do not enter the statistics.

Use `--result-output=results.json` for JSON, or add `--result-format=jsonl` for
JSONL. Each benchmark test has a `benchmarks` array containing the same data
plus the ordered `samples_ns`. Retried tests retain measurements for every
attempt. If a callable raises, Karva reports its exception and retains completed
samples and actual invocation counts. Hard worker termination cannot preserve
unfinished samples. `benchmark.stats` exposes the measurement inside the test.

When collection finds a `benchmark` reference in test or configuration syntax,
Karva runs the suite on one worker to avoid interference from other tests.
This is conservative: unrelated uses of that name can also select one worker.
If dynamic imports hide benchmark use from collection, run with
`--num-workers=1`; Karva rejects measurement on multiple workers. Disabled
measurement keeps normal parallel scheduling. Separate Karva invocations and
other programs can still contend for the same CPU.

The fixture supports callable and decorator use from `pytest-benchmark` suites.
It does not implement `benchmark.pedantic`, historical regression gating,
plugin hooks, or result uploads.
