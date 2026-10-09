# Original Document Positions

Adapters that generate Python tests from documentation can attach an original
position with `karva.tags.source`:

```python title="test_generated.py"
import karva

source = karva.SourceDocument("docs/guide.rst", ">>> 1 + 1\n2\n")

@karva.tags.source(source.location(1, 1))
def test_example():
    assert 1 + 1 == 2
```

The tag also works on individual parameter cases through the existing `tags`
argument:

```python
@karva.tags.parametrize("actual,expected", [
    karva.param(2, 2, id="guide-example", tags=[karva.tags.source(source.location(1, 1))]),
])
def test_example(actual, expected):
    assert actual == expected
```

A parameter case's source tag overrides the function's default source tag.
Function tags take precedence over inherited module tags. Within one
scope, the first source tag wins; for stacked function decorators, this is the
innermost source decorator. Only one dimension of stacked parametrization may
provide source tags. Multiple dimensions with source tags are rejected because
the original position would be ambiguous.

Create one `SourceDocument` per decoded document. Locations share its text
without copying the whole document per test case. The path identifies the
original document, which need not exist on disk. Lines and Unicode character
columns start at 1. A column immediately after the last character is allowed,
and invalid coordinates raise `ValueError`.

Failures identify the original position while retaining the Python traceback.
JUnit cases include `file`, `line`, and `column` attributes, including skipped
cases, retries, and fixture errors. Source metadata does not change collection,
filtering, cache, or coverage identity: those still refer to the generated Python
test. It does not collect documents or replay earlier examples when selecting a
later example.
