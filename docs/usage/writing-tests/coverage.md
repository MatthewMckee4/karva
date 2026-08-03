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

Pass a path or importable module or package name to limit measurement to specific source roots. Pass `--cov` multiple times to measure several:

```bash
uv run karva test --cov=src
uv run karva test --cov=example_package
uv run karva test --cov=pkg_a --cov=pkg_b
```

Equivalent configuration:

```toml
[tool.karva.profile.default.coverage]
sources = ["src"]
```

An empty entry (`""`) measures the cwd, matching `pytest-cov`'s bare `--cov`.

## Branch coverage

Pass `--cov-branch` to measure branch destinations as well as lines:

```bash
karva test --cov --cov-branch --cov-report=term-missing
```

```text
Name             Stmts   Miss   Branch   BrPart   Cover   Missing
──────────────────────────────────────────────────────────────────
test_branch.py       6      1        2        1     75%   5
──────────────────────────────────────────────────────────────────
TOTAL                6      1        2        1     75%
```

Branch coverage records line-to-line arcs for conditional control flow and compares them with statically possible branch destinations. The `Cover` percentage includes both statement and branch opportunities, matching coverage.py's branch coverage model. JSON, XML, and HTML reports include branch data when branch mode is enabled.

Equivalent configuration:

```toml
[tool.karva.profile.default.coverage]
sources = ["src"]
branch = true
```

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

Persisted native coverage data can produce the same report after the test run:

```bash
uv run karva coverage xml
uv run karva coverage xml --output build/coverage.xml
```

Cobertura structure, line and branch totals, project-relative class filenames, and XML escaping are supported external contracts for CI consumers.

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

Persisted native coverage data can be exported independently. Output is compact by default:

```bash
uv run karva coverage json
uv run karva coverage json --output build/coverage.json --pretty-print
uv run karva coverage json --show-contexts
```

Exported JSON is separate from Karva's native artifact. `meta.format` versions its documented schema; breaking field or semantic changes increment that number, while consumers must tolerate additive fields within a format version. Files contain executed, missing, and excluded lines, optional contexts, branch arcs when collected, and per-file summaries. `totals` contains aggregate line and branch metrics.

`uv run karva coverage lcov` writes a deterministic LCOV tracefile from persisted native data. Use `--output` to select another destination:

```bash
uv run karva coverage lcov
uv run karva coverage lcov --output build/coverage.lcov
```

LCOV `SF`, `DA`, `LF`, `LH`, `BRDA`, `BRF`, and `BRH` records are a supported external contract. Source paths are portable project-relative paths after configured path aliases are applied.

Combine native artifacts from CI shards before reporting:

```bash
uv run karva coverage combine artifacts/
uv run karva coverage combine shard-a.json shard-b.json
```

With no paths, `combine` discovers artifacts under `.karva/coverage/pending/`. Successfully consumed inputs are removed after the combined artifact is atomically replaced. Pass `--keep` to retain them or `--append` to include the existing combined artifact.

Delete combined native data and recognized pending shards without touching generated reports:

```bash
uv run karva coverage erase
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

Karva also excludes ellipsis-only placeholder bodies and `if TYPE_CHECKING:` or `if typing.TYPE_CHECKING:` clauses by default. These lines do not count as executable or missing.

Use `# pragma: no branch` when a conditional is executable but one destination is intentionally unreachable:

```python
if debug:  # pragma: no branch
    enable_diagnostics()
```

`while True` and literal boolean `if` conditions are recognised as structurally partial without a pragma. Configured regular expressions can mark project-specific branch lines the same way:

```toml
[tool.karva.profile.default.coverage]
partial-branches = ["if platform.system"]
```

Partial-branch rules suppress missing branch destinations only. The source line remains executable and measured.

## Source roots

Karva first treats each `--cov` value as a path relative to the project root. If that path does not exist, Karva resolves it as a module, regular package, or namespace package using the selected Python environment. Existing paths therefore take precedence over importable names. Modules without Python source and unresolved names produce an error naming both attempted interpretations.

Files under explicitly selected importable packages are measured even when they live in `site-packages` or a virtual environment. Broad path sources still skip nested `site-packages`, `dist-packages`, `.venv`, and `.tox` directories.

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
