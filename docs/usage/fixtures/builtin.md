Karva provides a set of built-in fixtures that can be used in your tests without any setup. They are all compatible with their pytest counterparts, so existing pytest tests can use them unchanged.

We will try to add more built-in fixtures from pytest in the future.

## Temporary Directory

This fixture provides the user with a `pathlib.Path` object that points to a temporary directory.

You can use any of the following fixture names:

- `tmp_path` (from pytest)
- `tmpdir` (from pytest)
- `temp_path` (from karva)
- `temp_dir` (from karva)

```py title="test.py"
def test_tmp_path(tmp_path):
    assert tmp_path.is_dir()
```

## Temporary Directory Factory

A session-scoped factory for creating temporary directories. Use this when you need to allocate a temporary directory from a `session`, `package`, or `module`-scoped fixture — `tmp_path` itself is function-scoped and cannot be consumed by longer-lived fixtures.

You can use any of the following fixture names:

- `tmp_path_factory` (from pytest) — returns `pathlib.Path` objects.
- `tmpdir_factory` (from pytest) — returns `py.path.local` objects.

The factory has two methods: `mktemp(name)` creates a fresh numbered subdirectory under the session's base temp directory, and `getbasetemp()` returns that base directory.

```py title="test.py"
import karva

@karva.fixture(scope="session")
def shared_dir(tmp_path_factory):
    d = tmp_path_factory.mktemp("shared")
    (d / "data.txt").write_text("hello")
    return d

def test_uses_shared_dir(shared_dir):
    assert (shared_dir / "data.txt").read_text() == "hello"
```

## Mock Environment

This fixture allows you to safely modify environment variables, and the system path during tests. All changes are automatically undone after the test completes.

You can use any of the following fixture names:

- `monkeypatch` (from pytest)

This fixture is compatible with pytest's `monkeypatch` fixture.

```py title="test.py"
def test_setattr(monkeypatch):
    import os
    monkeypatch.setattr(os, 'getcwd', lambda: '/fake/path')
    assert os.getcwd() == '/fake/path'

def test_setenv(monkeypatch):
    monkeypatch.setenv('MY_VAR', 'test_value')
    import os
    assert os.environ['MY_VAR'] == 'test_value'
```

The fixture provides all of these helper methods:

```py
monkeypatch.setattr(obj, name, value, raising=True)
monkeypatch.delattr(obj, name, raising=True)
monkeypatch.setitem(mapping, name, value)
monkeypatch.delitem(obj, name, raising=True)
monkeypatch.setenv(name, value, prepend=False)
monkeypatch.delenv(name, raising=True)
monkeypatch.syspath_prepend(path)
monkeypatch.chdir(path)
```

The raising parameter determines whether or not a `KeyError` or `AttributeError` is raised when the attribute or item does not exist when trying to set / delete it.

### Simple Example

Consider a scenario where you are working with user configuration and you need to mock their cache directory.

```py title="test.py"
from pathlib import Path


def get_cache_dir():
    """Returns the user's cache directory."""
    return Path.home() / ".cache"


def test_get_cache_dir(monkeypatch):
    monkeypatch.setattr(Path, "home", lambda: Path("/fake/home"))

    assert get_cache_dir() == Path("/fake/home/.cache")
```

### Reusing Mocks

we can share mocks across multiple functions without having to rerun the mocking functions by using fixture.

See this example where instead of requesting the `monkeypatch` fixture, we can reuse the `mock_response` fixture.

This lets us move the patching logic to another function and reuse the `mock_response` fixture across multiple tests.

```py
import karva
import requests


class MockResponse:
    def json(self):
        return {"mock_key": "mock_response"}


def get_json(url):
    """Takes a URL, and returns the JSON."""
    r = requests.get(url)
    return r.json()


@karva.fixture
def mock_response(monkeypatch):
    def mock_get(*args, **kwargs):
        return MockResponse()

    monkeypatch.setattr(requests, "get", mock_get)


def test_get_json(mock_response):
    result = get_json("https://fakeurl")
    assert result["mock_key"] == "mock_response"
```

### Mocking Environment Variables

If you are working with environment variables, you often need to modify them when testing.

See the example on how this could be useful.

```py
import os


def get_num_threads() -> int:
    username = os.getenv("NUM_THREADS")

    if username is None:
        return -1

    return int(username)


def test_get_num_threads(monkeypatch):
    monkeypatch.setenv("NUM_THREADS", "42")
    assert get_num_threads() == 42


def test_get_num_threads_default(monkeypatch):
    monkeypatch.delenv("NUM_THREADS", raising=False)
    assert get_num_threads() == -1
```

See the [pytest documentation](https://docs.pytest.org/en/6.2.x/monkeypatch.html) for more information.

## Capturing Log Records

The `caplog` fixture captures log records emitted during a test. It is function-scoped and resets between tests, so each test sees a clean slate.

Use `caplog.at_level(level)` as a context manager to enable capture at a given level for a block, or `caplog.set_level(level)` to enable capture for the remainder of the test. Captured records are exposed as `caplog.records` (a list of `logging.LogRecord`), `caplog.messages` (the formatted messages only), `caplog.record_tuples` (tuples of `(logger_name, levelno, message)`), and `caplog.text` (the full formatted text). Call `caplog.clear()` to drop any records captured so far.

```py title="test.py"
import logging


def test_caplog_records(caplog):
    with caplog.at_level(logging.WARNING):
        logging.warning("something happened")

    assert len(caplog.records) == 1
    assert caplog.records[0].levelname == "WARNING"
    assert caplog.records[0].getMessage() == "something happened"
    assert "something happened" in caplog.text
```

```py title="test.py"
import logging


def test_caplog_messages(caplog):
    caplog.set_level(logging.INFO)
    logging.info("first")
    logging.info("second")

    assert caplog.messages == ["first", "second"]
    assert caplog.record_tuples == [
        ("root", logging.INFO, "first"),
        ("root", logging.INFO, "second"),
    ]
```

## Capturing Standard Output and Standard Error

The `capsys` fixture captures writes to `sys.stdout` and `sys.stderr` at the Python level. Call `capsys.readouterr()` to retrieve everything written since the last call; the return value has `.out` and `.err` string attributes, and the buffers are reset after each read.

```py title="test.py"
def test_capsys_stdout(capsys):
    print("hello")
    captured = capsys.readouterr()
    assert captured.out == "hello\n"
    assert captured.err == ""
```

```py title="test.py"
import sys


def test_capsys_stderr(capsys):
    print("error message", file=sys.stderr)
    captured = capsys.readouterr()
    assert captured.out == ""
    assert captured.err == "error message\n"
```

Log messages emitted during the test are also routed through the captured streams, so you can assert on them from `captured.err`:

```py title="test.py"
import logging


def test_capsys_captures_logging(capsys):
    logging.warning("something went wrong")
    captured = capsys.readouterr()
    assert "something went wrong" in captured.err
```

Use `capsys.disabled()` as a context manager to temporarily restore the real `sys.stdout` and `sys.stderr` inside a test — anything written while capture is disabled goes straight to the terminal instead of being captured:

```py title="test.py"
def test_capsys_disabled(capsys):
    with capsys.disabled():
        print("this goes to the real stdout")

    print("this is captured")
    captured = capsys.readouterr()
    assert "this is captured" in captured.out
```

## Capturing File Descriptors

The `capfd` fixture is identical in shape to `capsys`, but it captures output at the file-descriptor level (file descriptors 1 and 2). Use `capfd` when the code under test writes directly to the underlying file descriptors — for example via a C extension or a subprocess — rather than through Python's `sys.stdout`/`sys.stderr` objects.

```py title="test.py"
def test_capfd_stdout(capfd):
    print("hello from capfd")
    captured = capfd.readouterr()
    assert captured.out == "hello from capfd\n"
    assert captured.err == ""
```

```py title="test.py"
import sys


def test_capfd_stderr(capfd):
    print("error output", file=sys.stderr)
    captured = capfd.readouterr()
    assert captured.err == "error output\n"
```

## Binary Capture

`capsysbinary` and `capfdbinary` behave like `capsys` and `capfd`, but `readouterr()` returns `bytes` instead of `str`. Reach for them when you need to assert on raw bytes or when the code under test writes binary data directly to the output streams.

```py title="test.py"
def test_capsysbinary_stdout(capsysbinary):
    print("hello bytes")
    captured = capsysbinary.readouterr()
    assert captured.out == b"hello bytes\n"
    assert captured.err == b""
```

```py title="test.py"
import sys


def test_capfdbinary_stderr(capfdbinary):
    print("error fd bytes", file=sys.stderr)
    captured = capfdbinary.readouterr()
    assert captured.err == b"error fd bytes\n"
```

## Capturing Warnings

Use `karva.warns()` to assert that a block emits a warning. Pass `match` to
match its message with a regular expression. The context manager records every
warning emitted by the block.

```py title="test.py"
import warnings

import karva


def test_deprecation():
    with karva.warns(DeprecationWarning, match="use new_api") as recorded:
        warnings.warn("use new_api instead", DeprecationWarning)

    assert len(recorded) == 1
```

`karva.deprecated_call()` accepts `DeprecationWarning`,
`PendingDeprecationWarning`, and `FutureWarning`.

```py title="test.py"
import warnings

import karva


def test_old_entrypoint():
    with karva.deprecated_call(match="entrypoint"):
        warnings.warn("old entrypoint", FutureWarning)
```

The `recwarn` fixture captures every warning raised during the test. It behaves like a list of `warnings.WarningMessage` objects — you can index into it, iterate it, and take its length.

```py title="test.py"
import warnings


def test_recwarn_captures(recwarn):
    warnings.warn("deprecated", DeprecationWarning)

    assert len(recwarn) == 1
    assert recwarn[0].category is DeprecationWarning
```

Use `recwarn.pop(category)` to remove and return the first warning matching a given category — it raises `AssertionError` if no matching warning was recorded. Call `recwarn.clear()` to drop everything captured so far.

```py title="test.py"
import warnings


def test_recwarn_pop(recwarn):
    warnings.warn("deprecated", DeprecationWarning)
    warnings.warn("runtime issue", RuntimeWarning)

    w = recwarn.pop(DeprecationWarning)
    assert issubclass(w.category, DeprecationWarning)
    assert "deprecated" in str(w.message)
    assert len(recwarn) == 1

    recwarn.clear()
    assert len(recwarn) == 0
```
