# Failing fast

Karva runs the whole suite by default. Use `--fail-fast` or `--max-fail` to
stop after failures.

## Stopping after the first failure

`--fail-fast` stops scheduling new tests once any test fails:

```bash
uv run karva test --fail-fast
```

This is equivalent to `--max-fail=1`.

## Stopping after N failures

`--max-fail=N` is the general form: stop scheduling new tests once `N` have failed.

```bash
uv run karva test --max-fail=3
```

Tests already running may finish, so parallel runs can report more than `N`
failures.

## Configuration

```toml
[tool.karva.profile.default.test]
max-fail = 3
```

`fail-fast = true` is accepted as an alias for `max-fail = 1`. When both are set, `max-fail` wins.

## Forcing the suite to run

`--no-fail-fast` clears any `fail-fast` or `max-fail` value set in configuration and runs the entire suite:

```bash
uv run karva test --no-fail-fast
```

If both flags are passed, `--max-fail` takes precedence.
