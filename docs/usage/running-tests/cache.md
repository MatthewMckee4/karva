# Cache

Karva caches test durations for scheduling and failed test names for
`--last-failed`.

The cache lives in `.karva_cache` under the project root. Coverage runs also write per-worker coverage artifacts there.

## Re-running just the failures

`--last-failed` (or `--lf`) restricts the run to whichever tests failed in the previous invocation:

```bash
uv run karva test --last-failed
```

After fixing failures, run the full suite again:

```bash
uv run karva test                # see the failures
uv run karva test --last-failed  # iterate on just those
uv run karva test                # confirm the full suite passes again
```

Combine with `--watch` to keep iterating until they all pass:

```bash
uv run karva test --watch --last-failed
```

If the last run had no failures, `--last-failed` runs nothing.

## Disabling the cache

`--no-cache` disables reading and writing reusable test history for the current run. Tests are scheduled without duration hints and `--last-failed` becomes a no-op. Coverage artifacts are still written when coverage is enabled.

```bash
uv run karva test --no-cache
```

## Managing the cache

Two `uv run karva cache` subcommands manage cache contents directly:

```bash
uv run karva cache prune  # keep only the newest coverage/legacy run directory
uv run karva cache clean  # remove the cache directory entirely
```

`prune` preserves test history. Use `clean` to reset it.
