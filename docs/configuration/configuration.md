<!-- WARNING: This file is auto-generated (cargo run -p karva_dev generate-all). Update the doc comments on 'Config' and 'Options' in 'crates/karva_metadata/src/options/' if you want to change anything here. -->

# Configuration

Karva is configured through `karva.toml` (or the `[tool.karva]` table in `pyproject.toml`). All option groups live under a `[profile.<name>]` section; see [Profiles](profiles.md) for how to define and select profiles.

The reference below documents every project and profile field. Profile examples target the implicit `default` profile.

File-level configuration: a collection of named profiles.

Every option group lives inside `[profile.<name>]`. The implicit `default`
profile is always available; named profiles inherit from it and can
override individual fields.

## `required-version`

`SemVer` requirement that the running karva binary must satisfy.

When set, karva refuses to run if the installed version does not
match the requirement. This is useful in CI and for shared
repositories where every developer should be on a known-good
version.

**Default value**: `null`

**Type**: `string`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva]
required-version = ">=0.5.0"
```

---

## `tags`

Project-wide custom tag registry. Each key is a tag name and each value
is an optional description for project documentation. Empty descriptions
are allowed.

Enable [`strict-tags`](#strict-tags) in a profile to reject custom tags
absent from this table.

**Default value**: `{}`

**Type**: `table`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.tags]
integration = "Uses an external service"
slow = ""
```

---

Configuration groups combined across defaults, profiles, environment, and CLI.

## `env`

Environment variables applied to test workers before Python imports any
test modules or fixtures. Strings always set values. Use
`{ value = "...", preserve = true }` to keep an existing value, or
`{ unset = true }` to remove one. Karva's own variables are reserved.

**Default value**: `{}`

**Type**: `table`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.env]
APP_ENV = "test"
CACHE_DIR = { value = ".cache/tests", preserve = true }
LIVE_API_TOKEN = { unset = true }
```

---

## `coverage`

Controls measured Python sources and coverage report generation.

### `append`

Add a test run to compatible native data instead of replacing it.

**Default value**: `false`

**Type**: `true | false`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
append = true
```

---

### `branch`

Whether to measure branch coverage in addition to line coverage.

**Default value**: `false`

**Type**: `true | false`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
branch = true
```

---

### `context`

Static context component attached to every observation in the run.

**Default value**: `null`

**Type**: `str`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
context = "python=3.14"
```

---

### `contexts`

Include execution attributed to contexts matching these regular expressions.

**Default value**: `null`

**Type**: `list[str]`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
contexts = ["python=3\\.14", "test_checkout"]
```

---

### `data-file`

Native coverage artifact read and written by coverage commands.

Relative paths are resolved from the project root.

**Default value**: `.karva/coverage/data.json`

**Type**: `path`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
data-file = ".karva/coverage/data.json"
```

---

### `exclude-lines`

Regular expressions excluding matching source lines or whole clauses.

**Default value**: `null`

**Type**: `list[str]`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
exclude-lines = ["if TYPE_CHECKING:"]
```

---

### `fail-under`

Minimum total coverage percentage required for the run to succeed.

When set, the test command exits with a non-zero status if the
reported `TOTAL` coverage is below this value, even when every test
passed. Has no effect when tests already failed (the exit code is
already non-zero).

**Default value**: `null`

**Type**: `float (0..=100)`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
fail-under = 90
```

---

### `include`

Include only coverage report files matching these globs.

Globs are matched against the project-relative file path shown in the
coverage report, such as `src/package/module.py`. When unset, all files
under the configured coverage sources are included unless omitted.

**Default value**: `null`

**Type**: `list[str]`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
include = ["src/**"]
```

---

### `omit`

Exclude coverage report files matching these globs.

Globs are matched against the project-relative file path shown in the
coverage report, such as `src/package/module.py`. Omit filters are
applied after include filters.

**Default value**: `null`

**Type**: `list[str]`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
omit = ["**/migrations/*"]
```

---

### `partial-branches`

Regular expressions marking intentionally partial branch lines.

**Default value**: `null`

**Type**: `list[str]`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
partial-branches = ["if platform.system"]
```

---

### `path-aliases`

Ordered `FROM=TO` path mappings applied when native artifacts are read.

Use aliases to relocate absolute sources collected outside the project
or artifacts produced under a different CI checkout layout.

**Default value**: `null`

**Type**: `list[str]`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
path-aliases = ["/workspace=.", "C:/repo=."]
```

---

### `precision`

Decimal places shown in coverage percentages.

**Default value**: `0`

**Type**: `non-negative integer`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
precision = 2
```

---

### `report`

Coverage report type.

`term` (default) prints a compact terminal table.
`term-missing` extends it with a `Missing` column listing the
uncovered line numbers per file. `none` persists native data only.

**Default value**: `term`

**Type**: `none | term | term-missing | xml | json | html | lcov`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
report = "term-missing"
```

---

### `report-path`

Optional output path for machine-readable coverage reports.

For XML, JSON, and LCOV reports, this controls the output file. For HTML,
it controls the output directory. If omitted, karva writes to
`coverage.xml`, `coverage.json`, `coverage.lcov`, or `htmlcov/` in the
project root.

**Default value**: `null`

**Type**: `path`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
report-path = "build/coverage.xml"
```

---

### `sources`

Source paths or importable Python names to measure coverage for.

Equivalent to passing `--cov=<source>` on the command line; may be
listed multiple times. An empty entry (`""`) measures the current
working directory, matching pytest-cov's bare `--cov`.

**Default value**: `null`

**Type**: `list[str]`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
sources = ["src"]
```

---

## `junit`

Controls `JUnit` XML output, captured streams, and flaky-test representation.

### `flaky-fail-status`

How flaky tests configured to fail are represented in `JUnit`.

**Default value**: `failure`

**Type**: `failure | success`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.junit]
flaky-fail-status = "success"
```

---

### `path`

Output path for the `JUnit` XML report.

When unset, no `JUnit` report is written.

**Default value**: `null`

**Type**: `path`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.junit]
path = "reports/test-results.xml"
```

---

### `report-name`

Name of the top-level `JUnit` test suite collection.

**Default value**: `"karva-tests"`

**Type**: `string`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.junit]
report-name = "karva-tests"
```

---

### `store-failure-output`

Whether to include captured stdout and stderr for failing tests.

**Default value**: `true`

**Type**: `true | false`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.junit]
store-failure-output = true
```

---

### `store-success-output`

Whether to include captured stdout and stderr for passing tests.

**Default value**: `false`

**Type**: `true | false`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.junit]
store-success-output = true
```

---

## `src`

Controls test-path discovery and whether filesystem ignore rules are honored.

### `include`

A list of files and directories to check.
Including a file or directory will make it so that it (and its contents)
are tested.
When unset, Karva checks the `tests` directory if it exists, otherwise
it checks the project root.

- `tests` matches a directory named `tests`
- `tests/test.py` matches a file named `test.py` in the `tests` directory

**Default value**: `null`

**Type**: `list[str]`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.src]
include = ["tests"]
```

---

### `respect-ignore-files`

Whether to automatically exclude files that are ignored by `.ignore`,
`.gitignore`, `.git/info/exclude`, and global `gitignore` files.
Enabled by default.

**Default value**: `true`

**Type**: `bool`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.src]
respect-ignore-files = false
```

---

## `terminal`

Controls diagnostic formatting, captured output, and displayed test statuses.

### `final-status-level`

Test summary information to display at the end of the run.

Modeled after `cargo-nextest`'s `--final-status-level`. Levels are
cumulative in the same way as [`status_level`](#status-level).

Defaults to `pass`.

**Default value**: `pass`

**Type**: `none | fail | retry | slow | pass | skip | all`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.terminal]
final-status-level = "fail"
```

---

### `output-format`

The format to use for printing diagnostic messages.

Defaults to `full`.

**Default value**: `full`

**Type**: `full | concise`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.terminal]
output-format = "concise"
```

---

### `show-python-output`

Whether to show the python output.

This is the output the `print` goes to etc.

**Default value**: `true`

**Type**: `true | false`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.terminal]
show-python-output = false
```

---

### `status-level`

Test result statuses to display during the run.

Modeled after `cargo-nextest`'s `--status-level`. Levels are
cumulative: `pass` shows passing and failed tests, `skip` adds
skipped tests on top, and so on. `retry` and `slow` are accepted
for forward-compatibility but currently behave like `fail`.

Defaults to `pass`.

**Default value**: `pass`

**Type**: `none | fail | retry | slow | pass | skip | all`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.terminal]
status-level = "fail"
```

---

## `test`

Controls test selection, retries, timeouts, and failure policies.

### `fail-fast`

Whether to stop at the first test failure.

This is a legacy alias for [`max_fail`](#max-fail): `true`
corresponds to `max-fail = 1` and `false` leaves the limit unset.
When both are set, `max-fail` takes precedence.

Defaults to `false`.

**Default value**: `false`

**Type**: `true | false`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
fail-fast = true
```

---

### `fail-slow`

Duration budget (in seconds) for a test's full lifecycle (fixture
setup, the test call, and fixture teardown).

When set, a test is allowed to run to completion — including
teardown, so cleanup is never skipped — and is then reported as a
failure if the full lifecycle took longer than this budget. Tests
can override the limit individually with
[`@karva.tags.fail_slow`](https://docs.karva.dev/usage/failure-handling/fail-slow/),
which takes precedence over the configured default.

This is distinct from [`timeout`](#timeout), which kills a test
mid-execution, and [`slow_timeout`](#slow-timeout), which is purely
informational and never fails a test.

Defaults to unset, which disables budget checking unless a tag is
applied to the test.

**Default value**: `null`

**Type**: `float (seconds)`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
fail-slow = 0.25
```

---

### `flaky-result`

Whether tests that pass only after a retry should fail the run.

**Default value**: `pass`

**Type**: `pass | fail`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
flaky-result = "fail"
```

---

### `max-fail`

Stop scheduling new tests once this many tests have failed.

Accepts a positive integer. Omitting the field (the default) lets
every test run regardless of how many fail. Setting `max-fail = 1`
is equivalent to the legacy `fail-fast = true`.

When both [`fail_fast`](#fail-fast) and `max-fail` are set,
`max-fail` takes precedence.

**Default value**: `unlimited`

**Type**: `positive integer`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
max-fail = 3
```

---

### `no-tests`

Configures behavior when no tests are found to run.

`auto` (the default) fails when no filter expressions were given, and
passes silently when filters were given. Use `fail` to always fail,
`warn` to always warn, or `pass` to always succeed silently.

**Default value**: `auto`

**Type**: `auto | pass | warn | fail`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
no-tests = "warn"
```

---

### `random-seed`

Seed used to randomize test order when [`shuffle`](#shuffle) is enabled.

When omitted, Karva generates and prints a seed for the run. Setting a
seed does not enable shuffling by itself.

**Default value**: `null`

**Type**: `u64`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
random-seed = 170938
```

---

### `retry`

When set, we will retry failed tests up to this number of times.

**Default value**: `0`

**Type**: `u32`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
retry = 3
```

---

### `run-timeout`

Wall-clock limit (in seconds) for the entire run.

When the run takes longer than this duration, karva stops the
remaining workers and exits with a failure status. This is a safety
net for CI to bound runaway suites; it does not affect individual
test results that already completed.

Defaults to unset, which lets the run take as long as it needs.

**Default value**: `null`

**Type**: `float (seconds)`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
run-timeout = 1800.0
```

---

### `shuffle`

Use seeded randomized ordering instead of duration-aware scheduling.

Defaults to `false`. When enabled, Karva prints the seed used for the
run so the same order can be reproduced.

**Default value**: `false`

**Type**: `true | false`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
shuffle = true
```

---

### `slow-timeout`

Threshold (in seconds) after which a test is flagged as slow.

When set, tests that take longer than this duration are reported with
a `SLOW` status line and counted in the run summary. The `SLOW` line
is gated on `--status-level=slow` (or higher); the summary always
shows the slow count when `--final-status-level=slow` is set.

Defaults to unset, which disables slow-test detection.

**Default value**: `null`

**Type**: `float (seconds)`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
slow-timeout = 60.0
```

---

### `strict-tags`

Reject custom tags that are absent from the project-wide `[tags]` registry.
Built-in Karva tags and pytest marks remain available without registration.

**Default value**: `false`

**Type**: `true | false`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
strict-tags = true
```

---

### `termination-grace-period`

Grace period (in seconds) between graceful worker termination and
force-kill.

Karva uses this when stopping workers because of Ctrl+C, fail-fast, or
`run-timeout`. Set to `0` to send the force-kill signal immediately
after the graceful termination signal.

Defaults to 10 seconds.

**Default value**: `10.0`

**Type**: `float (seconds)`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
termination-grace-period = 10.0
```

---

### `test-function-prefix`

The prefix to use for test functions.

Defaults to `test`.

**Default value**: `test`

**Type**: `string`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
test-function-prefix = "test"
```

---

### `timeout`

Hard per-test timeout (in seconds).

When set, every test that runs longer than this duration is killed
and reported as a failure. Tests can override the limit individually
with [`@karva.tags.timeout`](https://docs.karva.dev/usage/tags/timeout/),
which takes precedence over the configured default.

Defaults to unset, which disables hard timeouts unless a tag is
applied to the test.

**Default value**: `null`

**Type**: `float (seconds)`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
timeout = 120.0
```

---

### `try-import-fixtures`

When set, we will try to import functions in each test file as well as parsing the ast to find them.

This is often slower, so it is not recommended for most projects.

**Default value**: `false`

**Type**: `true | false`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.test]
try-import-fixtures = true
```

---

