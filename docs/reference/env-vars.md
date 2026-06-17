<!-- WARNING: This file is auto-generated (cargo run -p karva_dev generate-all). Update the doc comments on the env-var structs in 'crates/karva_static/src/lib.rs' if you want to change anything here. -->

# Environment Variables

This page lists every environment variable that Karva reads from the environment, plus the variables the worker exposes to running tests.

## Read by Karva

Variables Karva reads from the environment to influence its own behavior.

### `RAYON_NUM_THREADS`

This is a standard Rayon environment variable.

### `KARVA_MAX_PARALLELISM`

This is a standard Karva environment variable.

### `KARVA_CONFIG_FILE`

This is a standard Karva environment variable.

### `KARVA_LOG_PROFILE`

When set to a truthy boolish value, writes tracing profile output
to `tracing.folded`.

### `KARVA_SNAPSHOT_UPDATE`

When set to a truthy boolish value, snapshot assertions write
directly to `.snap` instead of creating `.snap.new` pending files.

## Set by the worker on tests

Variables the Karva worker writes into the test process so running test code can introspect the run, the worker, and its own attempt.

### `KARVA`

Always set to `"1"`. The cheapest signal that test code is running
under Karva, useful for fixtures and helpers that want to detect
the runner without heavier introspection.

### `KARVA_WORKER_ID`

0-indexed worker number. The canonical way to partition shared
resources (database names, ports, scratch directories) across
parallel workers without coordination.

### `KARVA_RUN_ID`

Unique identifier for a single `karva test` invocation, shared by
every worker. Encodes `<ms>-<uuid>`: a millisecond Unix timestamp
followed by a UUID v4. The timestamp prefix sorts chronologically
across runs; the UUID makes the id unique even when multiple jobs
start in the same millisecond. The same value names the run's
cache directory under `.karva_cache/run-<ms>-<uuid>`, which makes
correlating logs to cached artifacts straightforward.

### `KARVA_WORKSPACE_ROOT`

Absolute path to the directory Karva resolved as the project root.
Saves tests from re-deriving the root via `__file__` walking or
`os.getcwd()` heuristics.

### `KARVA_TEST_NAME`

Qualified name of the currently running test, e.g.
`pkg.module::test_foo(value=1)`. Updated before each attempt.

### `KARVA_ATTEMPT`

The 1-indexed attempt number for the currently running test. Always
set; `"1"` when no retries are configured.

### `KARVA_TOTAL_ATTEMPTS`

The total number of attempts allowed for the currently running test
(`retries + 1`). Always set.

### `KARVA_PROFILE`

Name of the active configuration profile, e.g. `"default"` or
whatever was passed to `--profile` / `KARVA_PROFILE`.

### `KARVA_TEST_THREADS`

Configured number of worker processes for this run. Mirrors
`--num-workers` (capped to the number of useful workers).

### `KARVA_VERSION`

Version of the running karva CLI.

