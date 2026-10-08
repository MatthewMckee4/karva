# Contributing

## Before Starting

[`contributor-friendly`](https://github.com/MatthewMckee4/karva/issues?q=is%3Aissue%20state%3Aopen%20label%3Acontributor-friendly)
issues are ready for contributions.
[`bug`](https://github.com/MatthewMckee4/karva/issues?q=is%3Aissue%20state%3Aopen%20label%3Abug)
issues are also good candidates when the expected behavior is clear.

Comment before starting work so another contributor does not duplicate it and
the maintainer can confirm the issue is current. Discuss larger changes and
new features first; they can affect Karva's deliberately small scope and
maintenance burden.

Use [GitHub issues](https://github.com/MatthewMckee4/karva/issues/new) for bug
reports, feature proposals, and documentation problems.

## Documentation

Build the [Zensical](https://zensical.org/) documentation:

```sh
uv run --isolated --only-group docs zensical build
```

Run the documentation generator after changing configuration options, CLI
arguments, or environment variable definitions:

```sh
cargo dev generate-all
```

Files under `docs/reference/` and `docs/configuration/configuration.md` are
generated; their headers identify the source.

## Opening a Pull Request

Keep pull requests minimal and focused. Use the pull request template and link
relevant issues. Keep it draft while substantial work remains.

Write the summary and test plan as concise prose, not lists. If CI is the only
test plan, write `ci`. Keep commits focused with descriptive one-line subjects.
Do not mix formatter churn with logic changes or add AI tools as authors.

Use only the `internal` label when nothing changes for users and only `ci` for
CI performance changes. Reserve `performance` for user-facing improvements.

## Reviewing PR Benchmarks

Open the `PR Benchmarks` workflow run to review the complete wall-time, memory,
and diagnostic summary online. Changed test workloads are shown separately from
comparable performance results because their raw totals measure different work.

The `benchmark-reports` artifact contains rendered HTML and Markdown reports.
Its diagnostic report includes the normalized baseline and candidate output for
every project, including each passing, failing, errored, and skipped test. To
inspect the artifact locally, set `RUN_ID` to the workflow run ID:

```sh
gh run download "$RUN_ID" \
  --repo MatthewMckee4/karva \
  --name benchmark-reports \
  --dir benchmark-reports
```

Open `benchmark-reports/diagnostic-report.html` in a browser. The wall-time and
memory reports are beside it.

## Integration Tests

Put tests that cross Python, worker, or CLI boundaries in
`crates/karva/tests/it/`. Name each test after the observable behavior it proves
and keep its Python project small. Separate unrelated success, validation,
and cleanup scenarios so a failure identifies the broken contract.

Use parameterization when the same contract holds for each input. Use small
helpers to centralize repeated framework API or keyword spelling differences
so tests focus on behavior. Different fixture lifetimes and failure phases
deserve separate named tests.

A passing command alone does not prove fixture reuse or cleanup. Check setup
counts or object identity for caching, and inspect a file after the worker
exits for final teardown. For skipped or rejected tests, use a marker to prove
that the body did not run. Keep multiple Python tests together when their
relationship is the behavior under test, such as sharing a scoped fixture.

Snapshot exit code, stdout, and stderr for setup commands as well as the
command being exercised. Generate snapshots with Insta, review the changed
expectations, and rerun without snapshot updates before requesting review.
