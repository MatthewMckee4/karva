Karva is available as [`karva`](https://pypi.org/project/karva/) on PyPI.

Use karva directly with `uvx`:

```bash
uvx karva test
uvx karva version
```

Or install karva with `uv`, or `pip`:

```bash
# With uv.
uv tool install karva@latest

# Add karva to your project.
uv add --dev karva

# With pip.
pip install karva
```

Once installed, you can use karva to run your tests:

```bash
karva test
```

Or to get the version of karva you're using:

```bash
karva version
```
