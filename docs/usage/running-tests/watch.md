# Watch mode

`--watch` reruns tests when Python source files change:

```bash
uv run karva test --watch
```

## What triggers a re-run

Karva watches Python source files under the project root. Edits to `.py` files queue a fresh run; edits to other files are ignored.

The watcher debounces rapid saves so a single editor write does not produce multiple runs.

## Combining with other flags

Combine `--watch` with test selection:

Re-run only the tests that failed last time, then everything once they pass:

```bash
uv run karva test --watch --last-failed
```

Watch one test:

```bash
uv run karva test --watch -E 'test(/^pkg::test_login$/)'
```

To exit, press `Ctrl-C`.
