# Native coverage format

Karva stores durable coverage data as deterministic, versioned JSON. This is
the internal interchange format used by collection, combination, and reporting.
It is not coverage.py's SQLite format and it is not the exported
`coverage.json` report.

Version 1 records the producing Karva version, collection mode, project and
source roots, an optional run context, and files keyed by normalized
project-relative path. Each file records a stable source-content fingerprint,
executable, excluded, and executed lines, line contexts, and optional possible
and executed branch arcs with their contexts. The fingerprint is lowercase
hexadecimal fixed-key SipHash-128/1-3 over the source bytes.

Objects use lexicographically sorted keys. Sets serialize as sorted arrays, so
equivalent coverage data produces identical bytes. Writers replace artifacts
atomically. Readers accept version 1 only and report the artifact path, found
version, and supported version when the schema is incompatible.

Before reporting, Karva compares each stored source fingerprint with the
current source bytes. Changed or missing sources fail reporting instead of
silently producing results from stale code.
