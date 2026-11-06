# Check suite health

Check Python source without importing modules or running tests:

```sh
uv run karva check
uv run karva check tests/test_example.py --output-format json
```

Karva reports test functions, test methods, and fixtures that a later definition
can replace in the same scope. Each `shadowed-definition` finding points to both
definitions and explains which binding wins. Rename or remove one definition.

Mutually exclusive branches of one `if` statement and unreachable literal-boolean
branches do not conflict. When execution depends on dynamic conditions, the
finding explicitly says the definitions may shadow each other.

The command uses the current project's test prefix and accepts `--config-file`
and `--profile`. It checks Python files beneath the supplied paths, or the
current project when no paths are supplied. JSON locations are UTF-8 byte ranges
with an exclusive end. Exit codes are 0 for no findings, 1 for health findings,
and 2 for a configuration or filesystem error. This check does not change test
collection or execution.
