The `timeout` tag fails a test if it runs longer than the given number of seconds. Use it to time-box individual tests rather than relying on a CI hard-kill.

## Basic Usage

```python title="test.py"
import karva
import time

@karva.tags.timeout(2.0)
def test_function():
    time.sleep(5)  # raises TimeoutError after 2 seconds
```

The threshold accepts fractional seconds (`@karva.tags.timeout(0.5)`).

## Configuring a default timeout

Use the `timeout` setting (or `--timeout=SECONDS` on the CLI) to apply the same hard limit to every test in the project:

```bash
karva test --timeout=120
```

```toml
[tool.karva.profile.default.test]
timeout = 120
```

A test-level `@karva.tags.timeout` always wins over the configured default, so individual tests can opt into a longer or shorter window.

## Sync vs async tests

Sync tests are submitted to a single-worker `concurrent.futures.ThreadPoolExecutor`. When the limit elapses, a `TimeoutError` is raised against the test and the worker thread is abandoned — Python has no safe way to interrupt arbitrary code, so any side effects already started will continue. If a test repeatedly times out and leaks resources, fix the test rather than the timeout.

Async tests are wrapped in `asyncio.wait_for`, which cancels the coroutine via `CancelledError` when the limit elapses.

## Fixtures

Fixture setup runs before the timeout starts, so a slow fixture does not count toward the limit. The clock starts when the test body begins executing.

## See also

- [Slow tests](../failure-handling/slow-tests.md) for `--slow-timeout`, which only flags slow tests rather than failing them.
- [Fail slow](../failure-handling/fail-slow.md) for `@karva.tags.fail_slow`, which fails a test that exceeds a duration budget without killing it mid-execution.
