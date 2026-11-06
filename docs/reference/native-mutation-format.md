# Native Mutation Results

Karva stores resumable mutation results in `.karva/mutation/results.json`.
`uv run karva mutate results --json` prints the same versioned object, optionally
filtered by path, symbol, operator, or outcome.

`format_version` identifies the schema. Karva rejects unsupported versions before
reading records; delete incompatible data and run mutations again. Each completed
candidate atomically replaces the artifact, preserving earlier completed results
if a later worker fails or execution is interrupted.

| Run field | Meaning |
| --- | --- |
| `context_fingerprint` | Identity of the most recent execution context. |
| `configuration` | Effective mutation settings, profile, worker count, and explicit timeout. |
| `baseline_seconds` | Measured baseline wall time in seconds. |
| `records` | Map from deterministic mutant ID to its result. |

Each record contains `mutant`, `context_fingerprint`, `outcome`, `selected_tests`,
`duration_seconds`, `reason`, and an optional `diagnostic`. A record from an earlier
execution context remains inspectable after a filtered retest. Its fingerprint
must match the run fingerprint before its outcome can be considered current.

`mutant` contains the project-relative `/`-separated `path`, complete
`source_fingerprint`, inclusive `start` and exclusive `end` UTF-8 byte offsets,
one-based `line`, qualified `symbol`, `operator`, `original`, and `replacement`.
Its `id` binds those inputs deterministically. Offsets always refer to the
original source; overlapping candidates are independent edits.

Outcomes are `pending`, `killed`, `survived`, `uncovered`, `timed-out`, `invalid`,
and `errored`. See [Mutation Testing](../usage/writing-tests/mutation-testing.md)
for their meaning and exit statuses. A `pending` result is never reused as completed
work. `selected_tests` lists tests that actually entered execution, excluding tests
skipped by a selection filter.
