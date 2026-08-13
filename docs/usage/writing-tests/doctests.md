# Doctests

Karva can run Python's standard-library doctest examples from module, function,
class, and method docstrings. Doctest collection is opt-in:

```bash
uv run karva test --doctest-modules
```

Enable it for a profile in `karva.toml`:

```toml
[profile.default.test]
doctest-modules = true
```

Each docstring containing examples becomes one Karva test case. Examples in one
docstring share their doctest namespace, so they can build on values created by
earlier examples. Karva gives each case a stable `doctest:` ID, such as
`doctest:@module`, `doctest:add`, or `doctest:Calculator.multiply`; use that ID
when filtering a run:

```bash
uv run karva test tests/calculator.py::doctest:add
```

Karva uses Python's standard-library doctest parser and runner, including
standard directives such as `# doctest: +SKIP`, `ELLIPSIS`, and
`NORMALIZE_WHITESPACE`:

```python
def add(left, right):
    """Add two numbers.

    >>> add(1, 2)
    3
    >>> add(1, 2)  # doctest: +SKIP
    999
    """
    return left + right
```

This MVP collects source-defined module docstrings, functions and classes
declared directly at module scope, and members declared directly on those
classes, including nested classes. The documented object must remain visible
under its source name after the module is imported; otherwise, Karva reports
that case as skipped. Dynamic docstrings, definitions inside control flow, and
`__test__` entries are not collected.

Karva does not collect `.txt` doctest files or provide pytest's other doctest
collection and option flags. Use standard-library directives inside examples
instead.
