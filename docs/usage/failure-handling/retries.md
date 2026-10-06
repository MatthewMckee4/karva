# Retries

Use retries for known intermittent failures, such as unreliable network calls.

## Enabling retries

Pass `--retry N` to retry each failed test up to `N` more times:

```bash
uv run karva test --retry 3
```

```toml
[tool.karva.profile.default.test]
retry = 3
```

A test that passes on any attempt is treated as a pass. The first failed attempt is reported as `TRY 1 FAIL`; the test is only marked as failing once every attempt has been exhausted.

Set `flaky-result = "fail"` when retries should expose flaky tests without allowing them to pass the run:

```toml
[tool.karva.profile.default.test]
retry = 2
flaky-result = "pass"

[tool.karva.profile.ci.test]
flaky-result = "fail"
```

The equivalent command-line override is:

```bash
uv run karva test --retry 2 --flaky-result fail
```

`pass` remains the default. With `fail`, Karva keeps the test's `FLAKY` status and attempt history but exits non-zero, marks JSON and JSONL reports as failed, and adds a JUnit failure by default. `--flaky-result` does not enable retries by itself. `KARVA_FLAKY_RESULT` provides the same override through the environment.

To see the per-attempt lines in the run output, raise the status level:

```bash
uv run karva test --retry 3 --status-level=retry --final-status-level=retry
```

The summary includes an `N retried` count.

## Per-test retry overrides

Profile-level `retry` applies to every test. To grant a flakier subset more attempts (or fewer) without changing the global default, define one or more `[[profile.<name>.overrides]]` entries. Each entry pairs a [filter expression](../../configuration/configuration.md) with one or more option fields; the first matching override wins.

```toml
[profile.default.test]
retry = 1

[[profile.default.overrides]]
filter = "tag(network)"
retries = 5

[[profile.default.overrides]]
filter = "tag(unit)"
retries = 0
```

In this example tests tagged `network` retry up to five times, tests tagged `unit` never retry, and everything else falls back to `retry = 1`. Overrides defined in a named profile (`[[profile.ci.overrides]]`) take precedence over those defined under `default`.

Overrides can also set `flaky-result` per test:

```toml
[profile.default.test]
flaky-result = "pass"

[[profile.default.overrides]]
filter = "tag(strict)"
flaky-result = "fail"
```

An explicit `--flaky-result` or `KARVA_FLAKY_RESULT` value disables all per-test `flaky-result` overrides, matching nextest's global override behavior.

The same `[[profile.<name>.overrides]]` block also supports `timeout`, `slow-timeout`, and `fail-slow` fields, mirroring the [profile-level timeout](../../configuration/configuration.md), slow-test threshold, and [fail-slow budget](fail-slow.md). A matching override with a non-positive value disables the corresponding limit for that test, even when the profile sets one.

```toml
[profile.default.test]
timeout = 30.0

[[profile.default.overrides]]
filter = "tag(integration)"
timeout = 300.0
slow-timeout = 30.0
```

## Detecting attempts from inside a test

Tests can read `KARVA_ATTEMPT` (1-indexed) and `KARVA_TOTAL_ATTEMPTS` (`retries + 1`) from the environment:

```python
import os

def test_eventual_consistency():
    attempt = int(os.environ["KARVA_ATTEMPT"])
    if attempt == 1:
        # bail out fast on the first attempt
        ...
```

Both variables are always set, even when retries are disabled. See [Environment Variables](../../reference/env-vars.md) for the full list.

## When not to retry

Retries can hide regressions. Fix flaky tests where possible, and limit retries
to known infrastructure failures.
