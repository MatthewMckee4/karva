# Mutation Testing

Use mutations to find behavior your tests do not check. Karva changes one piece
of application code in memory, runs tests in fresh workers, and records the
outcome. Run this from your project root or a directory below it:

```sh
uv run karva mutate run --source src
uv run karva mutate results
uv run karva mutate show <mutant>
```

`show` prints the exact diff, outcome, duration, and tests executed. IDs are
stable for unchanged source; commands also accept a unique ID prefix.

Improve a test, then rerun one mutant or a filtered set:

```sh
uv run karva mutate retest <mutant>
uv run karva mutate retest --outcome survived
```

To copy a mutation into your working tree deliberately:

```sh
uv run karva mutate apply <mutant>
```

Application requires the entire original file to match its recorded fingerprint.
Stale source and conflicting spans fail without changing the file.

## Sources and Filters

Sources default to `src` when present, otherwise the project root. Test files,
`tests` directories, virtual environments, and generated caches are excluded.
Configure source roots explicitly when application code lives elsewhere.

```toml
[tool.karva.profile.default.mutation]
sources = ["src"]
exclude = ["**/generated/**"]
operators = ["literal", "comparison", "boolean", "arithmetic", "control-flow", "return"]
exclude-symbols = ["*.generated"]
exclude-lines = ["# generated"]
```

`--source`, `--exclude`, and `--operator` replace their configured lists. Use
repeatable `--path` and `--symbol` globs to narrow candidates, and `--outcome` to
select prior results. Path globs are relative to the project root; symbols use
qualified names such as `Service.validate`.

Exclude an individual line with `# pragma: no mutate`. Use standalone
`# karva: mutate off` and `# karva: mutate on` comments around a block. A mutation
spanning any excluded line is omitted. These pragmas inside strings have no effect.

## Execution and Results

Karva first checks the unmodified suite and measures its wall time. A failing
baseline stops mutation execution and preserves the original test failures.
Each mutant gets a fresh interpreter; parallel workers never rewrite source files.
Python rejects candidates that cannot compile before any tests execute.

The default per-mutant timeout is ten times the measured baseline. Override it
with `--timeout` in seconds or `mutation.timeout`. Set `--workers` or
`mutation.workers` to control parallelism. Timeout diagnostics include the
mutant, requested tests, measured baseline, and budget.

Coverage identifies affected tests, which run first. Karva checks the full suite
before declaring a survivor when the affected tests pass; incomplete mappings
also fall back to the full suite. `--full-suite` bypasses affected-test ordering.

| Outcome | Meaning |
| --- | --- |
| `killed` | A test or collection check failed with the mutation. |
| `survived` | The full suite passed with the mutation. |
| `uncovered` | The full suite passed without importing the mutated source. |
| `timed-out` | Execution exceeded the mutation or test timeout. |
| `invalid` | Python could not compile the mutated source; tests did not run. |
| `errored` | Worker or controller failure prevented a trustworthy result. |
| `pending` | Work has not completed, including interrupted candidates. |

Exit status is `0` when all selected candidates are killed or invalid, `1` when
any survive, remain uncovered, or time out, and `2` for baseline failures,
infrastructure errors, invalid arguments, or incomplete runs.

Results are written atomically to `.karva/mutation/results.json` after each
candidate. Repeat `run` after interruption to reuse completed work. Source,
tests, project inputs, effective configuration, dependencies, interpreter,
environment, and executable changes invalidate cached results. Generated caches
and virtual environments are excluded from project input discovery; installed
dependencies are fingerprinted separately. Tests depending on external services
or other mutable state should use `--no-cache`.

Add `--json` to `run` or `results` for the versioned native result object.
`show --json` returns the record and diff. Both terminal and JSON reports use the
same stored outcomes. Reports can include earlier execution contexts after a
filtered retest; compare each record's `context_fingerprint` to the top-level
value before treating it as current.

Cache identity includes the complete worker environment, including terminal,
session, and CI variables. Opening another terminal or changing CI runs can
therefore invalidate otherwise unchanged results. This conservative policy avoids
reusing outcomes for tests that observe those variables.

`retest` selects recorded candidates with `--path`, `--symbol`, `--operator`, and
`--outcome`. Source roots and `--exclude` apply to discovery through `run`.
