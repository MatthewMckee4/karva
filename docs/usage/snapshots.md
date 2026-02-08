Snapshot testing captures the output of your code and stores it in a file. On subsequent runs, the output is compared against the stored snapshot. If the output changes, the test fails with a diff showing what changed.

This is useful for testing complex outputs like formatted strings, serialized data, or API responses without writing manual assertions.

## Basic Usage

Use `karva.assert_snapshot()` to capture a value as a snapshot.

```python title="test.py"
import karva

def test_greeting():
    karva.assert_snapshot("hello world")
```

The first time you run this test, it will fail and create a pending snapshot file at `snapshots/test__test_greeting.snap.new`. Accept it to create the baseline:

```bash
karva snapshot accept
```

On subsequent runs, the test passes as long as the output matches the stored snapshot.

## Formats

By default, values are stored as strings using `str()`. You can choose a different format with the `format` parameter.

### repr

```python title="test.py"
import karva

def test_data():
    karva.assert_snapshot({"a": 1}, format="repr")
```

Stores the value using Python's `repr()`.

### json

```python title="test.py"
import karva

def test_data():
    data = {"users": ["Alice", "Bob"], "count": 2}
    karva.assert_snapshot(data, format="json")
```

Stores the value as pretty-printed JSON with sorted keys and 2-space indentation. This is useful for dictionaries and nested structures where you want a readable, deterministic output.

## Named Snapshots

By default, each snapshot is named after the test function. If you need multiple snapshots in a single test, you can provide explicit names with the `name` parameter.

```python title="test.py"
import karva

def test_page():
    karva.assert_snapshot("<h1>Title</h1>", name="header")
    karva.assert_snapshot("<p>Body text</p>", name="body")
    karva.assert_snapshot("<footer>2024</footer>", name="footer")
```

This creates three separate snapshot files:

- `snapshots/test__test_page--header.snap`
- `snapshots/test__test_page--body.snap`
- `snapshots/test__test_page--footer.snap`

Without explicit names, multiple snapshots in the same test are numbered automatically (`test_page`, `test_page-2`, `test_page-3`).

## Snapshot Files

Snapshot files are stored in a `snapshots/` directory next to your test file. Each file uses YAML frontmatter to record metadata:

```text
---
source: test.py:5::test_greeting
---
hello world
```

The `source` field records the file, line number, and test name that produced the snapshot.

When a test produces a new or changed snapshot, a `.snap.new` file is created alongside the existing `.snap` file. This pending file must be explicitly accepted or rejected before the test will pass.

## Updating Snapshots

When you intentionally change the output of your code, use `--snapshot-update` to update all snapshots in place without creating pending files:

```bash
karva test --snapshot-update
```

This writes directly to `.snap` files and the tests pass immediately.

## CLI Commands

The `karva snapshot` subcommand manages pending snapshots.

### accept

Accept all pending snapshots, promoting `.snap.new` files to `.snap`:

```bash
karva snapshot accept
```

### reject

Reject all pending snapshots, deleting the `.snap.new` files:

```bash
karva snapshot reject
```

### pending

List all pending snapshots:

```bash
karva snapshot pending
```

### review

Interactively review each pending snapshot one at a time:

```bash
karva snapshot review
```

For each snapshot, you can:

- **a** -- accept (keep the new snapshot)
- **r** -- reject (retain the old snapshot)
- **s** -- skip (keep both for now)
- **i** -- toggle extended info display
- **d** -- toggle diff display

Use uppercase **A**, **R**, or **S** to apply the action to all remaining snapshots.

All commands accept optional path arguments to filter which snapshots are affected:

```bash
karva snapshot accept tests/api/
karva snapshot review tests/test_output.py
```

## Parametrized Tests

Snapshot testing works with parametrized tests. Each parameter combination gets its own snapshot file.

```python title="test.py"
import karva

@karva.tags.parametrize("name", ["Alice", "Bob"])
def test_greet(name):
    karva.assert_snapshot(f"Hello, {name}!")
```

This creates:

- `snapshots/test__test_greet(name=Alice).snap`
- `snapshots/test__test_greet(name=Bob).snap`

Named snapshots in parametrized tests combine both:

```python title="test.py"
import karva

@karva.tags.parametrize("lang", ["en", "fr"])
def test_translate(lang):
    karva.assert_snapshot(translate("hello", lang), name="greeting")
```

This creates:

- `snapshots/test__test_translate--greeting(lang=en).snap`
- `snapshots/test__test_translate--greeting(lang=fr).snap`
