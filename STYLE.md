# Style Guide

This guide covers user-facing text in Karva documentation, CLI output, issue
templates, and release notes.

## General

- Write `Karva` for the project and `karva` for the executable or package name.
- Use direct, concrete language. Prefer "run `karva test`" over "it is possible
  to run `karva test`".
- Use backticks for commands, flags, environment variables, file paths, package
  names, and code expressions.
- Avoid bare URLs in prose. Prefer descriptive links.
- Wrap Markdown at 100 characters unless the file is generated.
- Use language tags on fenced code blocks.
- Prefer "test runner" or "test framework" consistently; avoid introducing new
  product terms unless they appear in the docs already.

## Documentation

- Start from the user's task, then add details.
- Put common workflows before edge cases.
- Keep generated reference pages generated. Update the source and regenerate the
  docs instead of hand-editing generated output.
- Use `console` fences when showing commands and their output. Use `bash` only
  for shell scripts.
- Include command output only when the exact output matters.
- Use `karva test` examples with small test files or paths a reader can adapt.
- Link to reference pages from guides when the reader needs the complete option
  list.

## CLI Output

- Error messages should say what failed and include the relevant path, flag, or
  test name when available.
- Hints should be actionable and formatted as `hint: <message>`.
- Output must still make sense without color.
- Write machine-readable data to stdout. Write diagnostics, progress, and
  warnings to stderr.
- Keep one-line status messages short enough to scan in parallel test output.

## Terminology

- Use "fixture", not "setup helper", for Karva fixture APIs.
- Use "tag", not "marker", for Karva tags unless comparing directly with
  pytest marks.
- Use "worker process" for the subprocess that executes tests.
- Use "cache directory" for Karva's on-disk result and duration storage.
- Use "snapshot", not "golden file", for Karva snapshot-testing output.
