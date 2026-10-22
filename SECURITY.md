# Security Policy

Karva is a Python test framework. By design, it imports and executes Python test
files, fixtures, and the code those tests exercise.

Running an untrusted test suite is therefore equivalent to running untrusted
Python code. Arbitrary behavior caused by test code, fixture setup or teardown,
imports during discovery, subprocesses, native extensions, or the code under test
is not considered a vulnerability in Karva.

If you think Karva can make one of those areas clearer or harder to misuse,
please open a feature request instead.

Please report vulnerabilities in Karva itself privately by emailing
<matthewmckee04@yahoo.co.uk>. Include the affected version, a minimal
reproduction, and the impact you expect.

Security fixes target the latest released version and the `main` branch.

Use a maintained Python release with current security patches. Package
compatibility with an older Python version does not extend that interpreter's
security support.

The [October 2026 dependency review](docs/security-review.md) records the
advisories checked, fixes, and remaining operational responsibilities.
