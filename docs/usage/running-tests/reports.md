# Test reports

Karva can write JUnit XML for CI systems or native JSON/JSONL for custom tools.
Report paths are resolved from the project root, and missing parent directories
are created automatically.

## JUnit XML

Configure JUnit output in a profile:

```toml
[tool.karva.profile.ci.junit]
path = "reports/test-results.xml"
report-name = "karva-tests"
store-failure-output = true
store-success-output = false
flaky-fail-status = "failure"
```

```bash
uv run karva test --profile ci
```

JUnit suites are grouped by Python module. Assertion failures use `<failure>`,
fixture and collection errors use `<error>`, and skipped tests use `<skipped>`.
Retry history is preserved with `flakyFailure` and `rerunFailure` elements.
Captured stdout and stderr are included according to the two `store-*-output`
settings.

When `flaky-result = "fail"`, `flaky-fail-status = "failure"` adds a regular
`failure` element as well as preserving `flakyFailure`. Set it to `success` to
keep the JUnit testcase successful while Karva still exits non-zero. It can be
overridden for matching tests:

```toml
[[tool.karva.profile.ci.overrides]]
filter = "tag(strict)"

[tool.karva.profile.ci.overrides.junit]
flaky-fail-status = "failure"
```

## JSON

`--result-output` writes one JSON document by default:

```bash
uv run karva test --result-output reports/results.json
```

The document contains a schema version, final run status, elapsed time,
aggregate statistics, and one record per test. Test records include qualified
names, status, duration, skip reason, captured output, diagnostics, and retry
attempts when applicable. Collection and import diagnostics that do not belong
to one test are stored separately as run diagnostics.

Result output is plain text without ANSI styling, regardless of terminal color
settings.

## JSONL

Use JSONL when a consumer should process records independently:

```bash
uv run karva test \
  --result-output reports/results.jsonl \
  --result-format jsonl
```

Karva writes a `test` record for each test, a `run_diagnostic` record for each
unattached collection or import diagnostic, then one `run_finished` record
with final status and aggregate statistics. Every line includes the schema
version and record type.
