Karva provides a few built-in fixtures that can be used in your tests.

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
