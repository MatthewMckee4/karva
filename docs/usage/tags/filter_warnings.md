# Warning filters

Karva captures unconsumed Python warnings during module imports, collection,
fixture setup, test calls, and teardown. Passing tests still show a warning
summary with category, message, file, line, and emitted count. Warnings outside
an active test are reported at run level.

```sh
uv run karva test
uv run karva test -W error -W 'ignore:legacy API:DeprecationWarning:third_party'
uv run karva test --max-warnings=0
```

## Project policy

```toml
[profile.default.test]
warning-filters = ["default"]

[profile.ci.test]
warning-filters = [
    "error",
    "ignore:legacy API:DeprecationWarning:third_party",
]
max-warnings = 0
```

Filters use Python's `action:message:category:module:line` format. Trailing fields
can be omitted. Message and module fields are regular expressions; categories
are built-in warning names or importable dotted names. Supported actions are
`default`, `error`, `ignore`, `always` (or `all`), `module`, and `once`. Invalid filters fail
before test execution. Actions may be abbreviated, as in Python's `-W` syntax.

The default policy is `default`, which exposes categories that Python normally
hides. Within each collection or test-attempt window, Python decides whether a
warning is emitted: `default` emits once per message, category, and location;
`always` emits every occurrence. The summary groups identical category, message,
file, and line observations across workers. The warning budget counts emitted
observations before summary deduplication, including every retry. Ignored warnings,
warnings converted into exceptions, and warnings consumed by `recwarn`,
`karva.warns`, or `warnings.catch_warnings(record=True)` do not count.

Omitting `max-warnings` disables the budget. Exceeding the budget fails the run
after teardown completes, while `error` filters raise exceptions at the warning
site. Budget failures report both the configured limit and emitted count.

## Test-local policy

```python
import karva


@karva.tags.filter_warnings("ignore:legacy API:DeprecationWarning")
def test_compatibility_path():
    assert old_api() == 1
```

`pytest.mark.filterwarnings` is also supported, including module marks and
parameter-specific marks. Local filters apply throughout the test's fixture
setup, call, and function-scoped teardown. Broader-scoped auto-use setup and
teardown use the run policy when no test is active.

Later filters in a list take precedence. A named profile replaces the default
profile's filter list, CLI filters replace the selected profile's list, and local
tags override those filters. Stacked decorators are applied from the closest
decorator to the function outwards; function filters override module filters, and parameter
filters override function filters. Filters and warning registries are restored between tests,
parameter cases, and retries.

JSON reports contain a `warnings` array with optional `test` and `attempt`
attribution. JSONL reports emit `warning` records. JUnit retains warnings in
`system-err`, even for passing tests with success-output storage disabled.

See [Python's warning documentation](https://docs.python.org/3/library/warnings.html)
for filter and repeated-warning semantics.
