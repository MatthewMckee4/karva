# Run timeout

`--run-timeout` puts a wall-clock limit on a test run. Its deadline starts
before collection, so collection, partitioning, and worker startup consume the
same budget as test execution. Use it as a CI tripwire for a runaway suite; use
[`@karva.tags.timeout`](../tags/timeout.md) when one test needs its own limit.

Measure healthy CI runs first, choose a timeout that leaves room for normal
variance, and pass that measured value in seconds:

```bash
uv run karva test --run-timeout="$KARVA_CI_RUN_TIMEOUT"
```

The same limit can be stored as `run-timeout` under the profile's `test`
configuration.

When the deadline expires, Karva stops remaining workers and fails the run.
Completed test results are still reported. Configured report files are also
written: native JSON/JSONL marks the run as failed, while JUnit contains the
completed results. CI should therefore honor karva's process exit status rather
than inferring timeout success from JUnit alone.

## Worker shutdown

Karva first asks workers to terminate gracefully. Workers that remain alive
after `termination-grace-period` are force-killed. Configure the grace period
long enough for any process signal handlers that must finish.

The same shutdown sequence applies to `Ctrl-C` and fail-fast cancellation.
