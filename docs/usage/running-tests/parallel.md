# Parallel execution

Karva runs tests across multiple worker processes by default. Each worker is a separate Python interpreter, so tests are isolated from each other at the process level — a crash, signal, or interpreter-state mutation in one test cannot bleed into another.

## Worker count

The default is one worker per CPU core. Override with `-n` / `--num-workers`:

```bash
karva test -n 4
```

`--no-parallel` is a shorthand for `-n 1` and is what you want when debugging, attaching a debugger, or running tests that share resources you cannot partition:

```bash
karva test --no-parallel
```

## Parametrized tests

Karva can distribute cases from one parametrized test across workers when the
parameter values are written directly as a list or tuple literal:

```python
@karva.tags.parametrize("value", [1, 2, 3])
def test_value(value):
    pass
```

Values provided through a variable, comprehension, generator, or function call
cannot be counted before a worker imports the module. Karva keeps those cases
together on one worker:

```python
values = [1, 2, 3]

@karva.tags.parametrize("value", values)
def test_value(value):
    pass
```

All cases still run; only their distribution across workers changes.

## Partitioning shared resources

Workers do not coordinate. If your tests touch a shared resource — a database, a port, a temp directory — partition it on `KARVA_WORKER_ID` rather than locking:

```python
import os

@karva.fixture(scope="session")
def database_url():
    worker = os.environ["KARVA_WORKER_ID"]
    return f"postgresql:///test_{worker}"
```

`KARVA_WORKER_ID` is `0`-indexed and stable for the lifetime of the worker. See [Environment Variables](../../reference/env-vars.md) for the full list of variables the worker exposes to tests.

## Output capture

By default, stdout/stderr from a test is captured and emitted only when the test fails or when `--show-output` / `-s` is set. This keeps parallel output legible — without capture, output from concurrent tests would interleave on the terminal.

`--no-capture` disables capture entirely and forces a single worker, since uncaptured output from concurrent workers cannot safely interleave:

```bash
karva test --no-capture
```

Reach for `--no-capture` when debugging with `print` statements or attaching `pdb`. For ad-hoc inspection without giving up parallelism, prefer `-s` / `--show-output`, which keeps capture on but prints the captured output for every test.

## Splitting a run across CI jobs

`--partition slice:M/N` runs only slice `M` of `N` total slices. Tests are sorted by qualified name and distributed round-robin: test 1 to slice 1, test 2 to slice 2, ..., test `N+1` to slice 1, and so on. Running every `slice:1/N` through `slice:N/N` together covers every collected test exactly once.

```bash
karva test --partition slice:1/3
karva test --partition slice:2/3
karva test --partition slice:3/3
```

Slices are computed deterministically from the current test set, so the same revision splits the same way on every machine. Adding or removing tests can shift which slice a given test falls into, so this is less stable per-test than a hash-based scheme but does not need any historical data.

Use `hash:M/N` when stable assignment matters more than even slices. Each
qualified test name maps to one stable bucket, so adding or removing a test
does not move any other test:

```bash
uv run karva test --partition hash:1/3
uv run karva test --partition hash:2/3
uv run karva test --partition hash:3/3
```

Hash buckets can be uneven. Run every bucket together to cover the complete
suite.
