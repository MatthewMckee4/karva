# Cache

Karva keeps only reusable history on disk: per-test durations for parallel scheduling and the test names used by `--last-failed`. Worker test selections, live state, and complete results travel over loopback IPC and remain in memory.

The cache lives in `.karva_cache` under the project root. Coverage runs also write per-worker coverage artifacts there.

## Re-running just the failures

`--last-failed` (or `--lf`) restricts the run to whichever tests failed in the previous invocation:

```bash
uv run karva test --last-failed
```

A typical fix-it-up loop:

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

Two `karva cache` subcommands manage cache contents directly:

```bash
uv run karva cache prune  # keep only the newest coverage/legacy run directory
uv run karva cache clean  # remove the cache directory entirely
```

`prune` preserves reusable history. Reach for `clean` if the cache gets corrupted or after a cache-format change.
