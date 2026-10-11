//! Yield-fixture cleanup across function, module, package, and session scopes.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: workload sizes and repetitions are measured in crates/karva_benchmark/measurements.md.
pub(super) const CASES: usize = 16_384;
const FIXTURES: usize = 8;

/// Counts live function fixtures so missed cleanup fails the next test and scope finalizers.
pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let mut source = String::from("import karva\n\n_state = {\"active\": 0, \"completed\": 0}\n\n");
    for (scope, dependency) in [
        ("session", ""),
        ("package", "session_state"),
        ("module", "package_state"),
    ] {
        let state = if dependency.is_empty() {
            "_state"
        } else {
            dependency
        };
        writeln!(
            source,
            "@karva.fixture(scope=\"{scope}\")\ndef {scope}_state({dependency}):\n    state = {state}\n    yield state\n    assert state[\"active\"] == 0\n    assert state[\"completed\"] > 0\n"
        )?;
    }
    for fixture in 0..FIXTURES {
        writeln!(
            source,
            "@karva.fixture\ndef resource_{fixture}(module_state):\n    module_state[\"active\"] += 1\n    yield {fixture}\n    module_state[\"active\"] -= 1\n    module_state[\"completed\"] += 1\n"
        )?;
    }
    let arguments = (0..FIXTURES)
        .map(|fixture| format!("resource_{fixture}"))
        .collect::<Vec<_>>()
        .join(", ");
    for case in 0..CASES {
        writeln!(
            source,
            "def test_teardown_{case}({arguments}):\n    assert _state[\"active\"] == {FIXTURES}\n    assert resource_0 + resource_{} == {}\n",
            FIXTURES - 1,
            FIXTURES - 1
        )?;
    }
    fs::write(tests.join("test_teardown.py"), source)
        .context("Failed to write generated fixture teardown tests")
}
