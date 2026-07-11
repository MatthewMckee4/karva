# Coverage

Karva measures line coverage natively. There is no plugin to install, no `.coveragerc`, and no separate `coverage` binary on the path — coverage is part of `karva test`.

The implementation runs in the test worker on top of `sys.monitoring` (Python 3.12+) or `sys.settrace` (older versions), records every executed line under the configured source roots, and prints a `Name / Stmts / Miss / Cover` table at the end of the run.

## Quick start

Pass `--cov` to measure the current working directory:

```bash
karva test --cov
```

```text
Name              Stmts   Miss   Cover
─────────────────────────────────────────
test_control.py      18      3     83%
─────────────────────────────────────────
TOTAL                18      3     83%
```

Pass a path to limit measurement to specific source roots, or pass `--cov` multiple times to measure several:

```bash
karva test --cov=src
karva test --cov=pkg_a --cov=pkg_b
```

Equivalent configuration:

```toml
[tool.karva.profile.default.coverage]
sources = ["src"]
```

An empty entry (`""`) measures the cwd, matching `pytest-cov`'s bare `--cov`.

## Reports

`--cov-report=term` (the default) prints the compact table above. `--cov-report=term-missing` adds a `Missing` column listing the uncovered line numbers per file:

```bash
karva test --cov --cov-report=term-missing
```

```text
Name              Stmts   Miss   Cover   Missing
────────────────────────────────────────────────
test_missing.py      10      4     60%   6, 9-11
────────────────────────────────────────────────
TOTAL                10      4     60%
```

`--cov-report=xml[:PATH]` writes Cobertura XML for CI integrations. If `PATH` is omitted, karva writes `coverage.xml` in the project root:

```bash
karva test --cov=src --cov-report=xml
karva test --cov=src --cov-report=xml:build/coverage.xml
```

Equivalent configuration:

```toml
[tool.karva.profile.ci.coverage]
sources = ["src"]
report = "xml"
report-path = "build/coverage.xml"
```

`--cov-report=json[:PATH]` writes a machine-readable JSON report. If `PATH` is omitted, karva writes `coverage.json` in the project root:

```bash
karva test --cov=src --cov-report=json
karva test --cov=src --cov-report=json:build/coverage.json
```

Pass `--cov-context=test` with JSON reports to include a `contexts` map from executed source lines to the qualified test names that covered them:

```bash
karva test --cov=src --cov-context=test --cov-report=json
```

`--cov-report=html[:DIR]` writes a simple browsable HTML report. If `DIR` is omitted, karva writes `htmlcov/` in the project root:

```bash
karva test --cov=src --cov-report=html
karva test --cov=src --cov-report=html:build/htmlcov
```

Files that were never imported during the run still appear, at `0%`, so dead modules under your source root show up rather than silently inflating the total.

## Filtering report files

Use `--cov-include` and `--cov-omit` to keep generated files, migrations, vendored code, or other non-target paths out of the report:

```bash
karva test --cov=src --cov-include='src/**' --cov-omit='**/migrations/*'
```

Globs match the project-relative file path shown in the coverage report. When include filters are set, only matching files are reported. Omit filters are applied after include filters.

Equivalent configuration:

```toml
[tool.karva.profile.default.coverage]
sources = ["src"]
include = ["src/**"]
omit = ["**/migrations/*"]
```

## Failing on low coverage

`--cov-fail-under=N` exits non-zero when total coverage drops below `N`, even if every test passed:

```bash
karva test --cov --cov-fail-under=90
```

`N` accepts any value in `0..=100`, fractional values included. The flag has no effect when tests already failed — the exit code is already non-zero in that case.

```toml
[tool.karva.profile.default.coverage]
fail-under = 90
```

## Disabling for a single run

`--no-cov` overrides any `--cov` flag and any `[coverage] sources` configured in `karva.toml`:

```bash
karva test --no-cov
```

Use it when iterating locally without editing config — for example, to skip the tracer overhead on a tight feedback loop while CI keeps coverage on.

## Excluding code

Append `# pragma: no cover` to a line to exclude it from the executable-line set:

```python
def helper():
    if rare_condition():  # pragma: no cover
        return fallback()
    return main_path()
```

The pragma applies to the line it appears on. When placed on the head of a compound statement (`def`, `class`, `if`, `elif`, `else`, `except`, `match`, `case`, `with`, `for`, `while`, `try`), the entire body of that branch is excluded:

```python
def excluded():  # pragma: no cover
    do_thing()
    do_other_thing()
```

The match is case-insensitive (`# PRAGMA: NO COVER` works) and is only recognised inside an actual comment — the literal text inside a string is not a directive.

## Source roots

Every `--cov` value is canonicalised to an absolute path. A file is included in the report if its path lives under at least one source root and does not contain any of `site-packages`, `dist-packages`, `.venv`, or `.tox` — installed third-party code is filtered automatically.

## Parallel runs

Each worker writes its own JSON file. After the run, the main process unions the per-file line sets and produces a single report. No coordination flag is required; coverage works the same with `--no-parallel` or with `-n 16`.

## CI integration

A typical CI invocation pins a minimum and prints the missing lines:

```bash
karva test --cov=src --cov-report=term-missing --cov-fail-under=85
```

For XML-consuming tools such as SonarQube or Codecov:

```bash
karva test --cov=src --cov-report=xml:build/coverage.xml --cov-fail-under=85
```

For machine-readable JSON or a browsable HTML summary:

```bash
karva test --cov=src --cov-report=json:build/coverage.json --cov-fail-under=85
karva test --cov=src --cov-report=html:build/htmlcov --cov-fail-under=85
```

Or, equivalently, in `pyproject.toml`:

```toml
[tool.karva.profile.ci.coverage]
sources = ["src"]
report = "term-missing"
fail-under = 85
```

```bash
karva test --profile ci
```
