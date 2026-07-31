# Filtering tests

Karva selects which tests to run with **filter expressions**, a small
language inspired by [nextest's filtersets]. A single `-E` / `--filter`
flag composes name matching, tag matching, and boolean logic into one
expression — there are no separate `--tag` or `--match` flags.

```bash
karva test -E 'tag(slow)'
karva test -E 'test(/^auth::/) & not tag(flaky)'
karva test -E '(tag(fast) | tag(unit)) - tag(flaky)'
```

## Adding custom tags

Any attribute under `karva.tags` that is not a built-in tag creates a custom
tag. No registration is required:

```python title="test_checkout.py"
import karva


@karva.tags.integration
def test_checkout():
    assert checkout()
```

Pytest marks are also recognized as custom tags, so
`@pytest.mark.integration` matches the same `tag(integration)` predicate.
Arguments attached to custom tags do not affect filtering; filters match the
tag name.

When `-E` is passed more than once, a test runs if it matches **any** of
the expressions (OR across flags):

```bash
karva test -E 'tag(slow)' -E 'tag(integration)'
```

Expressions are evaluated against every discovered test. A test runs if
and only if the expression evaluates to true for it; otherwise it is
skipped.

## Predicates

A filter expression is built from predicates combined with boolean
operators. Karva currently supports two predicates:

- `test(<matcher>)` — evaluated against the fully qualified test name,
  e.g. `mod::sub::test_login`.
- `tag(<matcher>)` — evaluated against each custom tag on the test;
  matches if *any* tag matches. Both `karva.tags.*` decorators and
  `pytest.mark.*` decorators contribute tags.

Unknown predicate names are a parse error. The error message will
suggest the valid names. If you expected one and got the other, make
sure you haven't misspelled `test`/`tag` or used an older nextest
predicate (`package`, `binary`, `platform`, etc.) — karva does not
currently implement those.

## Operators

Predicates can be combined with the following operators. All operators
have both a symbolic and a keyword form, pick whichever is clearer in
context:

- `&` or `and` — logical AND, e.g. `tag(slow) & test(~login)`.
- `|` or `or` — logical OR, e.g. `tag(slow) or tag(fast)`.
- `not` or `!` — logical NOT, e.g. `not tag(flaky)`.
- `-` — difference (and-not), e.g. `tag(slow) - tag(flaky)` is
  shorthand for `tag(slow) & not tag(flaky)`. Useful for subtracting
  flaky or platform-gated tests from a broader selection.
- `( … )` — grouping, e.g. `(tag(a) or tag(b)) and tag(c)`.

### Operator precedence

From tightest-binding to loosest:

1. Grouping with parentheses
1. `not` / `!`
1. `&` / `and` and `-`
1. `|` / `or`

A few worked examples:

```text
tag(a) | tag(b) & tag(c)    ≡  tag(a) | (tag(b) & tag(c))
not tag(a) & tag(b)         ≡  (not tag(a)) & tag(b)
tag(a) - tag(b) | tag(c)    ≡  (tag(a) - tag(b)) | tag(c)
tag(a) & tag(b) - tag(c)    ≡  tag(a) & (tag(b) - tag(c))
```

When in doubt, parenthesize.

## Matchers

A matcher describes how a predicate's argument is compared against the
value it is evaluated over (a test name or a tag name). There are four
matcher kinds, distinguished by a single-character prefix:

- `=foo` — **exact**: the value must equal the pattern exactly.
- `~foo` — **substring**: the pattern must appear anywhere in the value.
- `/foo/` — **regex**: the value must match the [Rust regex]. Regex uses
  partial matching — anchor with `^` and `$` for a full match.
- `#foo` — **glob**: the value must match the [glob pattern]. `*`
  matches any run of characters, `?` matches a single character, and
  `[...]` is a character class.
- No prefix — **default**: see below.

Example expressions:

```text
test(=mod::test_login)        # exact test name
test(~login)                  # any test whose name contains "login"
test(/^mod::test_log/)        # regex — tests in mod:: starting with test_log
test(/test_add\(x=1\)/)       # regex — a parametrized test case
test(#*_login_*)              # glob — names with _login_ somewhere in them
tag(slow)                     # tag exactly named "slow"
tag(~slo)                     # any tag containing "slo"
tag(#py3*)                    # any tag matching the glob "py3*"
```

### Default matchers

When you omit the prefix, the matcher kind depends on the predicate:

- `test(foo)` defaults to **substring**. `test(login)` is the same as
  `test(~login)`. This matches how `cargo nextest` behaves and is what
  people usually want when typing something quick.
- `tag(foo)` defaults to **exact**. `tag(slow)` is the same as
  `tag(=slow)`. Tags are short identifiers, so partial matches almost
  always hit more than you want.

If you write tooling that constructs filter expressions programmatically,
always use an explicit prefix rather than relying on the default — it's
clearer to read and won't surprise you if the default ever changes.

### Matcher bodies

A matcher body is either a bare identifier, a quoted string, or a
delimited regex:

- **Bare identifiers** may contain letters, digits, `_`, `.`, `:`, and
  the glob metacharacters `*`, `?`, `[`, `]`, `{`, `}`, `^`, `$`. Most
  test names and tag names fit without quoting:
  `test(=mod::sub::test_login)` works as-is because `:` is permitted
  inside an identifier.
- **Quoted strings** (`"..."`) allow any character, including spaces
  and operator characters, so use them when a tag or test name contains
  something like a hyphen, a space, or a parenthesis:
  `tag(="my-nightly tag")`.
- **Regex literals** (`/.../`) accept the full [Rust regex] syntax.
  Note that `/` is the delimiter, so a literal `/` inside a regex must
  be escaped as `\/`.

The keywords `test`, `tag`, `and`, `or`, and `not` are **not** reserved
inside a matcher body — `tag(test)` correctly matches a tag literally
named `test`, and `tag(and)` matches a tag named `and`. The outer parser
only treats them as keywords at the top level of an expression.

### Escape sequences

Karva uses a deliberately minimal escape scheme so that regex
metacharacters round-trip without double-backslashing:

- Inside a regex literal `/ … /`, only `\/` is processed (to embed a
  literal `/`). All other backslash sequences are passed through to the
  regex engine unchanged, so you can write `test(/\d+/)` without
  doubling the backslash.
- Inside a quoted string `" … "`, only `\"` is processed (to embed a
  literal `"`). Again, other backslashes are preserved verbatim.
- Bare identifiers have no escape syntax at all — if your tag or test
  name needs characters the identifier rules don't allow, quote it.
- For literal glob metacharacters, use the bracket escape from
  [globset] — `#[*]` matches a literal `*` character, `#[?]` matches a
  literal `?`, and so on.

## Migration from `-t` and `-m`

Older releases of karva exposed separate `-t` / `--tag` and `-m` /
`--match` flags. Both have been removed and replaced by `-E` /
`--filter`. The new syntax is a strict superset — every old invocation
has a direct translation:

- `-t slow` becomes `-E 'tag(slow)'`
- `-t 'not slow'` becomes `-E 'not tag(slow)'`
- `-t 'slow and integration'` becomes `-E 'tag(slow) & tag(integration)'`
- `-t 'slow or integration'` becomes `-E 'tag(slow) or tag(integration)'`
- `-t '(slow or fast) and not flaky'` becomes `-E '(tag(slow) or tag(fast)) - tag(flaky)'`
- `-m auth` becomes `-E 'test(/auth/)'`
- `-m '^test::test_login'` becomes `-E 'test(/^test::test_login/)'`
- `-m 'slow|fast'` becomes `-E 'test(/slow|fast/)'`
- `-t slow -m auth` becomes `-E 'tag(slow) & test(/auth/)'`

Multiple `-E` flags keep the same OR-across-flags semantics that
multiple `-t` or `-m` flags used to have, so `-t a -t b` becomes
`-E 'tag(a)' -E 'tag(b)'` and not `-E 'tag(a) | tag(b)'` (though those
two are equivalent).

On top of the old capabilities, the new DSL adds substring, exact, and
glob matchers — previously only regex matching was possible for test
names, and only exact matching for tags.

## When nothing matches

With the default `--no-tests=auto`, collecting no tests fails the run when no
filter was supplied, but a valid filter that matches nothing exits
successfully. This makes optional CI shards and local filters easy to compose
without hiding an empty suite.

Use `--no-tests=pass`, `--no-tests=warn`, or `--no-tests=fail` to choose the
behavior explicitly. The same setting is available as `no-tests` in the
profile's `test` configuration.

## Grammar

For reference, the full grammar:

```text
filterset   ::= or_expr
or_expr     ::= and_expr (('|' | 'or') and_expr)*
and_expr    ::= unary_expr (('&' | 'and' | '-') unary_expr)*
unary_expr  ::= ('!' | 'not') unary_expr | atom
atom        ::= '(' or_expr ')' | predicate
predicate   ::= ('test' | 'tag') '(' matcher ')'
matcher     ::= '=' body      # exact
              | '~' body      # substring
              | '#' body      # glob
              | regex
              | body          # default (substring for test, exact for tag)
body        ::= identifier | string
regex       ::= '/' … '/'       # `\/` escapes a literal '/'
string      ::= '"' … '"'       # `\"` escapes a literal '"'
identifier  ::= [A-Za-z0-9_.:*?\[\]{}^$]+
```

[glob pattern]: https://docs.rs/globset/latest/globset/#syntax
[globset]: https://docs.rs/globset/latest/globset/#syntax
[nextest's filtersets]: https://nexte.st/docs/filtersets/reference/
[rust regex]: https://docs.rs/regex/latest/regex/#syntax
