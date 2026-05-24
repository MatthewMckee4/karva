<!-- WARNING: This file is auto-generated (cargo dev generate-all). Update the doc comments on the 'Options' struct in 'crates/karva_project/src/metadata/options.rs' if you want to change anything here. -->

# Configuration

Karva is configured through `karva.toml` (or the `[tool.karva]` table in `pyproject.toml`). All option groups live under a `[profile.<name>]` section; see [Profiles](profiles.md) for how to define and select profiles.

The reference below documents every field supported inside a profile. Examples target the implicit `default` profile.

## `required-version`

A SemVer requirement that the running karva binary must satisfy.

If the installed karva version does not match the requirement, karva exits with a clear error before running any tests. This prevents confusing failures when CI or other contributors run with an older version that does not support features used elsewhere in the configuration.

`required-version` is a top-level field, not part of any profile.

**Default value**: `null`

**Type**: SemVer requirement (`string`)

**Example usage** (`karva.toml`):

```toml
required-version = ">=0.5.0"
```

The same field in `pyproject.toml` lives under `[tool.karva]`:

```toml
[tool.karva]
required-version = ">=0.5.0"
```

---

## `coverage`

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

### `report`

Coverage terminal report type.

`term` (default) prints a compact terminal table.
`term-missing` extends it with a `Missing` column listing the
uncovered line numbers per file.

**Default value**: `term`

**Type**: `term | term-missing`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.coverage]
report = "term-missing"
```

---

### `sources`

Source paths to measure coverage for.

Equivalent to passing `--cov=<path>` on the command line; may be
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

## `src`

### `include`

A list of files and directories to check.
Including a file or directory will make it so that it (and its contents)
are tested.

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

### `show-progress`

Live progress display rendered while tests run.

`none` (the default) leaves output untouched. `counter` prints a
`N/M tests` line on stderr that refreshes periodically. `bar` shows
a visual progress bar with completion stats. The display is rendered
on stderr so it does not interfere with per-test result lines on
stdout (gated by [`status_level`](#status-level)).

Defaults to `none`.

**Default value**: `none`

**Type**: `none | counter | bar`

**Example usage** (`pyproject.toml`):

```toml
[tool.karva.profile.default.terminal]
show-progress = "bar"
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

