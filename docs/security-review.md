# Security dependency review: October 22, 2026

Reviewed Karva at `f6d6fe47`, its Rust lockfiles, Python development, documentation
and integration-test dependencies, and installation and publishing workflows.
Sources include upstream security advisories, the Python Security Developer in
Residence announcement, the Python Packaging Advisory Database through
`pip-audit`, and RustSec through `cargo-audit`. This is a dated dependency and
workflow review, not a penetration test or a guarantee that no vulnerabilities
remain.

## Findings and fixes

### Compromised emSign certificate authority

Karva pinned uv 0.13.0 in CI and PyPI publishing. Those installations trusted a
compromised certificate authority, allowing an attacker on the network path to
impersonate HTTPS services. [The uv advisory](https://github.com/astral-sh/uv/security/advisories/GHSA-hq85-5cpj-mr62)
requires uv 0.13.4 or newer. All setup-uv steps now explicitly select 0.13.5;
the project also rejects uv versions below 0.13.4 with an upgrade diagnostic.
Version 0.13.5 includes the Python download metadata needed for Python 3.15.1.
The ty pre-commit hook uses the [upstream security fix](https://github.com/astral-sh/ty-pre-commit/commit/61480a9eddba3de40a4cabde24c42e82f27e94f6),
which pins uv 0.13.4 in its isolated environment. The previous hook pinned uv
0.11.32, and the latest tagged hook, v0.0.90, still pins affected uv 0.13.3.

The same incident affects Python's bundled pip and `certifi`. CI now uses Python
3.15.1 for integration tests, and wheel and documentation workflows select
Python 3.14.9. These [expedited](https://www.python.org/downloads/release/python-3151/)
[security releases](https://www.python.org/downloads/release/python-3149/)
bundle pip 26.2.2 with `certifi` 2026.10.19. An unqualified minor version can use
an older interpreter from a runner's cache.

### pytest temporary-directory vulnerability

The integration requirements pinned pytest 9.0.2, affected by
[CVE-2025-71176](https://github.com/advisories/GHSA-6w46-j5rx-g56g).
Upgrade to the patched 9.0.3. Karva's adapted `TempPathFactory` already includes
the [upstream fix](https://github.com/pytest-dev/pytest/commit/95d8423bd24992deea5b9df32555fa1741679e2c):
it checks ownership and rejects symbolic links without following them where the
platform supports that operation. No change to Karva's fixture implementation
is needed for that advisory.

### Rust dependency advisories

RustSec identified crossbeam-epoch 0.9.18, affected by
[RUSTSEC-2026-0204](https://rustsec.org/advisories/RUSTSEC-2026-0204.html).
Update it to 0.9.20. It enters the runtime through `ignore` and crossbeam-deque;
the advisory concerns invalid pointer dereferences during formatting. This
review did not establish an exploitable formatting call in Karva.

RustSec also reported the rand 0.8.5 soundness warning
[RUSTSEC-2026-0097](https://rustsec.org/advisories/RUSTSEC-2026-0097.html).
Update it to 0.8.6. This dependency is used to generate parser lookup tables at
build time. The advisory requires specific logging and reseeding conditions;
updating removes the affected version without asserting those conditions occur
in Karva.

## Other advisories checked

The recent uv [Windows wheel traversal](https://github.com/astral-sh/uv/security/advisories/GHSA-2cv4-cqwr-gwf7),
[entry-point traversal](https://github.com/astral-sh/uv/security/advisories/GHSA-4gg8-gxpx-9rph),
and [RECORD deletion](https://github.com/astral-sh/uv/security/advisories/GHSA-pjjw-68hj-v9mw)
advisories affect versions older than the existing 0.13.0 pins. The new minimum
also excludes those versions. Earlier uv archive-parsing advisories were
checked against the upstream advisory list.

Neither PyO3 nor maturin had published advisories on their upstream security
pages at review time. Karva declares no Python runtime dependencies. Fresh
resolutions of all Python dependency groups and integration requirements were
audited for Python 3.10 and 3.15, including their transitive dependencies.
No known vulnerabilities remained in those resolutions after the pytest update.
Both Rust lockfiles were audited; the Zed lockfile needed no changes. The main
lockfile passed `cargo audit --deny warnings` after the two patch updates.

Workflow review confirmed full commit SHA pins for actions, restricted default
token permissions, PyPI trusted publishing, and validation of PR identity before
the privileged binary-size workflow posts artifact contents as a comment.
`pinact run` required no action updates.

## Operational follow-up and limits

Follow the [Python security announcement](https://discuss.python.org/t/information-for-python-users-regarding-emudhra-emsign-ca-certificate-security-incident/109576)
to update existing environments: uv to at least 0.13.4, pip to at least 26.2.2,
`certifi` to at least 2026.10.19, and sigstore to at least 4.5.1 where installed.
System and custom CA stores, existing virtual environments, container images,
and previously published Karva releases are not repaired by this repository
change. Rebuild or update them separately. Sigstore, requests, urllib3, and
`certifi` are not declared Karva runtime dependencies; installed tools can still
bundle their own copies.

Python 3.10 reached end of life in October 2026. Karva retains compatibility
testing for it; use a maintained interpreter for production workloads. Dropping
Python 3.10 compatibility is a separate product decision.

GitHub reported that Dependabot alerts are disabled for this repository, and
the code-scanning endpoint returned no analysis. Hosted findings were therefore
unavailable; these results come from local audits and published advisories.
Git-sourced dependencies, runner images, build containers, installed hook and
tool environments, and unpublished vulnerabilities are not exhaustively covered
by package-version audits. Python resolutions are fresh because `uv.lock` is
intentionally untracked; they do not describe every contributor's environment.
