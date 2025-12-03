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
