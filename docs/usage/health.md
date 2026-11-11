# Suite health

Inspect tests and fixture wiring without importing test modules or running tests:

```sh
uv run karva health
uv run karva health tests/unit --format=json
```

Health uses the same project configuration, profiles, path selectors, test prefix,
and directory ignore rules as `uv run karva test`. Source analysis cannot establish
behavior created by imports, decorators, or other dynamic Python code. Unknown
behavior does not become a definite finding. Static inspection uses the latest
supported Python grammar and does not validate active-interpreter compatibility.

The Python package’s console-script launcher still performs normal interpreter
startup before Karva receives arguments. Health itself does not import test
modules or launch workers unless execution is explicitly requested.

Each finding includes a stable rule ID, source location, evidence, and suggested
action. The initial rules expose existing source diagnostics: `duplicate-fixture`,
`invalid-fixture`, `missing-fixture`, `fixture-cycle`, and `fixture-scope-mismatch`.
These describe concrete collection and fixture-resolution failures rather than a
suite quality score. Their integration cases are reproducible examples of the
failures they detect.

Suppress individual rules with `--ignore=fixture-cycle,missing-fixture`. Use paths
to limit the inspection. `--fail-on=error` is the default; `warning` also fails for
advisories, and `none` displays findings without failing. Configuration, collection,
and invocation errors still exit with code 2.

JSON output uses `schema_version: 1`. Findings are sorted by source location and
rule. Locations contain one-based lines and Unicode columns and zero-based UTF-8
byte offsets. Paths are relative to the project root with forward slashes.

`shadowed-definition` identifies a test or fixture replaced by a later function
in the same module or class. Both locations are shown. Definitions in mutually
exclusive branches are excluded; uncertain execution produces
`possibly-shadowed-definition` with warning severity. These checks cover code
that runtime collection can no longer see, including duplicate test methods.
