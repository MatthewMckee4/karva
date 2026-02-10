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

## JSON Snapshots

For structured data like dicts and lists, use `karva.assert_json_snapshot()` for readable, deterministic output. It serializes values using `json.dumps(value, sort_keys=True, indent=2)`.

```python title="test.py"
import karva

def test_data():
    data = {"users": ["Alice", "Bob"], "count": 2}
    karva.assert_json_snapshot(data)
```

The snapshot stores:

```json
{
  "count": 2,
  "users": [
    "Alice",
    "Bob"
  ]
}
```

`assert_json_snapshot` supports all the same features as `assert_snapshot`: inline snapshots, `--snapshot-update`, filters via `snapshot_settings`, and the pending/accept workflow.

```python title="test.py"
import karva

def test_inline():
    karva.assert_json_snapshot({"a": 1}, inline='{\n  "a": 1\n}')
```

If the value is not JSON-serializable (e.g., a custom object without a default serializer), Python's `json` module raises a `TypeError`.

## Named Snapshots

By default, each snapshot is named after the test function. If a test contains more than one unnamed `assert_snapshot()` call, karva raises an error:

```text
Multiple unnamed snapshots in one test. Use 'name=' for each,
or wrap in 'karva.snapshot_settings(allow_duplicates=True)'
```

Use the `name` parameter to give each snapshot a distinct name:

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

Alternatively, wrap the calls in `snapshot_settings(allow_duplicates=True)` to opt in to auto-numbered unnamed snapshots (`test_page`, `test_page-2`, `test_page-3`):

```python title="test.py"
import karva

def test_page():
    with karva.snapshot_settings(allow_duplicates=True):
        karva.assert_snapshot("<h1>Title</h1>")
        karva.assert_snapshot("<p>Body text</p>")
        karva.assert_snapshot("<footer>2024</footer>")
```

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

## Filters

Snapshot output often contains non-deterministic values like timestamps, UUIDs, or file paths that change between runs. Use `karva.snapshot_settings()` to replace these with stable placeholders before comparison.

```python title="test.py"
import karva

def test_api_response():
    with karva.snapshot_settings(filters=[
        (r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}", "[timestamp]"),
        (r"[0-9a-f-]{36}", "[uuid]"),
    ]):
        karva.assert_snapshot(get_response())
```

Each filter is a `(regex_pattern, replacement)` tuple. Filters are applied sequentially to the serialized value before it is compared or stored in the snapshot file.

### Multiple Filters

When multiple filters are provided, they are applied in order. Earlier filters may affect what later filters see:

```python title="test.py"
import karva

def test_log_entry():
    with karva.snapshot_settings(filters=[
        (r"\d{4}-\d{2}-\d{2}", "[date]"),
        (r"\d+ms", "[duration]"),
    ]):
        karva.assert_snapshot("2024-01-15: request completed in 42ms")
```

The stored snapshot will contain: `[date]: request completed in [duration]`.

### Nested Settings

Settings can be nested. Inner filters are appended to outer filters, so all filters from the entire stack apply:

```python title="test.py"
import karva

def test_complex_output():
    with karva.snapshot_settings(filters=[(r"\d+ms", "[duration]")]):
        with karva.snapshot_settings(filters=[(r"/tmp/\S+", "[path]")]):
            karva.assert_snapshot("took 42ms at /tmp/abc123")
```

The stored snapshot will contain: `took [duration] at [path]`.

### Inline Snapshots

Filters also work with inline snapshots. The filtered value is what gets compared and stored:

```python title="test.py"
import karva

def test_inline_filtered():
    with karva.snapshot_settings(filters=[(r"\d{4}-\d{2}-\d{2}", "[date]")]):
        karva.assert_snapshot("event on 2024-01-15", inline="event on [date]")
```
