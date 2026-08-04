# Randomizing test order

## Default scheduling

By default, Karva uses cached test durations as weights. It keeps small modules
together, splits large modules, and assigns each group or test to the lightest
worker. Tests without duration history have equal weight and are randomized
before assignment, so their worker buckets can vary between runs. This default
optimizes for load balance, not reproducibility.

Use `--shuffle` to replace that scheduler with fully seeded randomized
ordering:

```bash
uv run karva test --shuffle
```

Karva prints the generated seed before running tests:

```console
Random seed: 170938
```

Pass that seed to reproduce the same worker assignment, per-worker order, and
Python standard-library randomness:

```bash
uv run karva test --shuffle --random-seed 170938
```

The test set, worker count, and configuration must also match. Parallel workers
still finish independently, so their displayed completion order can vary even
when assignment and execution order are identical.

## Configuration

```toml
[profile.default.test]
shuffle = true
random-seed = 170938
```

`random-seed` does not enable shuffling by itself. It still seeds Python
randomness when shuffling is disabled. Leave it unset with `shuffle = true` to
generate a new seed for each invocation.

## Python randomness

When a random seed is set, Karva derives distinct deterministic seeds for each
test's function-scoped fixture setup, test call, and fixture teardown. It
reapplies the same phase seeds on retries, independent of worker count,
selection, partition, or ordering. The active phase seed is available to test
code as `KARVA_RANDOM_SEED`.

Calling `random.seed()` inside a fixture or test remains under user control for
the rest of that phase. Karva does not change `secrets`, `random.SystemRandom`,
or operating-system entropy. Support for third-party random generators is out
of scope.

Use the most recently generated seed again without copying it from output:

```bash
uv run karva test --shuffle --random-seed last
```

Karva stores that seed in `.karva_cache/random-seed.json`. `last` is a CLI-only
selector; configuration files accept integer seeds.

## Selection and scheduling

Karva applies test selection before seeded ordering. `--last-failed` narrows the
set first, and `--partition` computes its stable `slice` or `hash` selection
before shuffling that partition. Filtered-out tests do not execute, and retries
remain attached to the selected test instead of entering the shuffle again.

Seeded runs ignore cached duration scheduling. This prevents timing history
from changing an otherwise reproducible assignment. Runs without `--shuffle`
keep the normal duration-aware scheduler.

Watch mode chooses one generated or configured seed when the session starts and
reuses it for every rerun. File changes can still change the collected test set.

JSON reports include `random_seed` at the run level and `random_seeds` on each
executed test. JSONL reports include the same fields on `run_finished` and
`test` records.
