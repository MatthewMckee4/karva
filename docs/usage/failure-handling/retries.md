# Retries

Some tests fail nondeterministically — a flaky network call, a timing-sensitive assertion, a race against a background task. Retrying lets the run continue without rewriting the test or quarantining it behind a tag.

## Enabling retries

Pass `--retry N` to retry each failed test up to `N` more times:

```bash
karva test --retry 3
```

```toml
[tool.karva.profile.default.test]
retry = 3
```

A test that passes on any attempt is treated as a pass. The first failed attempt is reported as `TRY 1 FAIL`; the test is only marked as failing once every attempt has been exhausted.

To see the per-attempt lines in the run output, raise the status level:

```bash
karva test --retry 3 --status-level=retry --final-status-level=retry
```

The summary line then includes a `N retried` counter so flake patterns are visible at a glance.

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

The same `[[profile.<name>.overrides]]` block also supports `timeout` and `slow-timeout` fields, mirroring the [profile-level timeout](../../configuration/configuration.md) and slow-test threshold. A matching override with a non-positive value disables the corresponding limit for that test, even when the profile sets one.

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

Retries hide regressions. Reach for them on tests that are flaky for known infrastructure reasons; do not blanket-enable them across the suite to suppress real failures. If a test is flaky for reasons you control, prefer fixing the root cause.
