# Writing tests

Karva collects module-level Python functions whose names start with `test` by
default. Test files do not need a `test_` prefix: Karva searches every `.py`
file under the selected paths and ignores files that contain no tests.

```python title="tests/check_calculator.py"
def test_addition():
    assert 1 + 2 == 3
```

Run the whole project, a directory, a file, or one function:

```bash
uv run karva test
uv run karva test tests/
uv run karva test tests/check_calculator.py
uv run karva test tests/check_calculator.py::test_addition
```

Use `--test-prefix` or `test-function-prefix` in configuration when your suite
uses another naming convention. Karva does not collect class methods; see
[Project Non-Goals](../non-goals.md#class-based-tests).

## Project and path discovery

With no path arguments, Karva searches upward from the current directory for
`karva.toml` or a `[tool.karva]` table in `pyproject.toml`, then uses that
directory as the project root. If no Karva configuration exists, it uses the
nearest plain `pyproject.toml`, or the current directory when neither exists.
Discovery does not cross a `.git` boundary.

Karva respects `.gitignore` files by default. Pass `--no-ignore` to include
Git-ignored Python files, or set `respect-ignore-files = false` under the
profile's `src` configuration.

## Test results

A passing test returns `None`, either explicitly or by reaching the end of the
function. Returning any other value fails the test with a diagnostic; use an
assertion instead.

Generator test functions are rejected before their bodies run. Use
[`@karva.tags.parametrize`](../tags/parametrize.md) to create multiple cases.
Generator fixtures remain supported for setup and teardown.

## Async tests and fixtures

Async tests run without a plugin or marker:

```python title="tests/test_service.py"
import asyncio


async def test_service():
    result = await asyncio.sleep(0, result=42)
    assert result == 42
```

Fixtures may also be async functions or async generators. Sync and async tests
can consume either sync or async fixtures:

```python title="tests/test_service.py"
import karva


@karva.fixture
async def service():
    client = await create_client()
    yield client
    await client.close()


def test_sync_code_can_use_async_fixture(service):
    assert service.is_ready()


async def test_async_code_can_use_async_fixture(service):
    assert await service.healthcheck()
```

Async generator fixture teardown is awaited after its consumer finishes, just
like teardown after `yield` in a sync fixture.
