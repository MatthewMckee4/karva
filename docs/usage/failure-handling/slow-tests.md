# Slow tests

A handful of slow tests can dominate wall-clock time long before they dominate the failure rate. Karva surfaces them in two complementary ways: a threshold-based `SLOW` status during the run, and a `--durations` ranking after it.

## Flagging slow tests during a run

`--slow-timeout=SECONDS` flags every test that runs for longer than the given duration:

```bash
karva test --slow-timeout=2.0
```

```toml
[tool.karva.profile.default.test]
slow-timeout = 2.0
```

The threshold accepts fractional seconds (`--slow-timeout=0.5`). Slow tests get a dedicated `SLOW` status line and are counted in the run summary.

The `SLOW` line is gated behind `--status-level=slow` (or higher); the summary slow count appears once `--final-status-level=slow` is set:

```bash
karva test --slow-timeout=2.0 --status-level=slow --final-status-level=slow
```

Slow detection is purely informational — it does not fail the run or kill the test. To time-box a test instead, use [`@karva.tags.timeout`](../tags/timeout.md). To fail a test that exceeds a duration budget without killing it mid-execution, use [`fail-slow`](fail-slow.md).

## Ranking the slowest tests

`--durations=N` prints the `N` slowest tests after the run completes, regardless of whether `--slow-timeout` is set:

```bash
karva test --durations=10
```

Use this when investigating a slow CI job: it answers "which tests are eating the budget?" without requiring you to guess at a sensible threshold first.
