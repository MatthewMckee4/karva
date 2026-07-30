# Fail slow

A test can complete correctly while still taking longer than an agreed performance budget. `fail-slow` lets a test run to completion — including fixture teardown, so cleanup is never skipped — and then fails it if the full lifecycle took too long.

This is a coarse regression budget, not a benchmarking tool: it does not add statistical sampling or baseline comparisons.

## Basic usage

```python title="test.py"
import time

import karva

@karva.tags.fail_slow(0.25)
def test_index_lookup():
    time.sleep(0.5)  # fails: exceeded its 0.25s budget
```

The threshold accepts fractional seconds (`@karva.tags.fail_slow(0.05)`).

## Configuring a default budget

Use the `fail-slow` setting (or `--fail-slow=SECONDS` on the CLI) to apply the same budget to every test in the project:

```bash
uv run karva test --fail-slow=1.0
```

```toml
[tool.karva.profile.default.test]
fail-slow = 1.0
```

A test-level `@karva.tags.fail_slow` always wins over the configured default, and per-test [overrides](retries.md#per-test-retry-overrides) win over the profile setting — the same precedence order used by `timeout` and `slow-timeout`.

## What counts toward the budget

The budget covers the test's entire lifecycle: fixture setup, the test call, and fixture teardown. Unlike [`@karva.tags.timeout`](../tags/timeout.md), which kills a test mid-execution, `fail-slow` never interrupts a running test — it lets setup, the call, and teardown all finish, then compares the total duration against the budget.

Only work performed within an individual test attempt counts toward its budget. Module, package, and session cleanup happens outside any single attempt and is not assigned to a test. Function-scoped fixtures are recreated for each retry.

If a test already fails for another reason (an assertion, a fixture error, a teardown error) and also exceeds its budget, both are reported: the original failure stays the primary cause, and the exceeded budget is noted alongside it.

## Retries

Each attempt is checked independently after its full lifecycle finishes. Exceeding the budget is an ordinary retryable failure: a later attempt that stays within budget makes the test flaky. Time from earlier attempts never counts against a later attempt's budget.

## See also

- [Timeout](../tags/timeout.md) for `@karva.tags.timeout`, which kills a test mid-execution instead of letting it finish.
- [Slow tests](slow-tests.md) for `--slow-timeout`, which only flags slow tests rather than failing them.
