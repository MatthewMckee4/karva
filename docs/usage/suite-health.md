# Check suite health

Check Python source without importing modules or running tests:

```sh
uv run karva check
uv run karva check tests/test_example.py --output-format json
```

Karva reports test functions, methods named with the test prefix in any class,
and fixtures that a later definition can replace in the same scope. Each
`shadowed-definition` finding points to both definitions and explains which
binding wins. Rename or remove one definition. Class bindings themselves are
not checked.

Mutually exclusive branches and unreachable literal-boolean branches do not
conflict. When execution depends on dynamic conditions, the finding explicitly
says the definitions may shadow each other.

The command uses the current project's test prefix and accepts `--config-file`
and `--profile`. It checks Python files beneath the supplied paths, or the
configured `src.include` paths when no paths are supplied (otherwise `tests/`,
then the project root). JSON paths use `/`; locations include UTF-8 byte ranges
and 1-based Unicode line/column pairs, both with an exclusive end.

Exit codes are 0 for no findings, 1 for health findings, and 2 for a configuration
or filesystem error. Warnings fail this opt-in check. This check
does not change test collection or execution.

This check uses an error-recovering parser and does not validate Python syntax.
Mutually exclusive branches inside a loop are checked within one iteration;
redefinitions across different iterations are not detected.
