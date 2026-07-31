# Usage

These guides cover daily work after the [Tutorial](../get-started/tutorial.md).
Start with the section that matches what you need to do.

You can run many existing pytest tests with Karva, but full pytest compatibility
is not a goal. If you hit unsupported behavior, check the [Project
Non-Goals](non-goals.md) first. If it is not covered there, [open an
issue](https://github.com/MatthewMckee4/karva/issues/new).

## Running tests

Select tests, distribute work across workers, rerun on file changes, and reuse
cached results. Start with [Filtering tests](running-tests/filtering.md).

## Failure handling

Choose when a run stops, retry failures, and find slow tests. Start with
[Failing fast](failure-handling/fail-fast.md).

## Writing tests

Capture complex values, measure coverage, and use Karva's test helpers. Start
with [Snapshots](writing-tests/snapshots.md).

## Fixtures

Share setup and teardown between tests, and use built-ins for common context.
Start with [Fixtures](fixtures/fixtures.md).

## Tags

Parametrize inputs, skip cases, mark expected failures, and set per-test
timeouts. Start with [Parametrize](tags/parametrize.md).
