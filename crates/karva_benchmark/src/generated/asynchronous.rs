//! Coroutine tests with coroutine and async-generator fixtures, without timed waits.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: workload sizes and repetitions are measured in crates/karva_benchmark/measurements.md.
pub(super) const CASES: usize = 4_096;

/// Exercises awaiting test bodies, fixture setup, and generator cleanup on every test.
pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let mut source = String::from(
        r#"import karva

_state = {"active": 0}

async def immediate(value):
    return value

@karva.fixture(scope="module")
def state():
    return _state

@karva.fixture
async def async_value():
    return await immediate(1)

@karva.fixture
async def async_resource(state):
    state["active"] += 1
    yield await immediate(2)
    await immediate(None)
    state["active"] -= 1

"#,
    );
    for case in 0..CASES {
        writeln!(
            source,
            "async def test_async_{case}(async_value, async_resource):\n    assert _state[\"active\"] == 1\n    assert await immediate(async_value + async_resource) == 3\n"
        )?;
    }
    fs::write(tests.join("test_async.py"), source)
        .context("Failed to write generated asynchronous tests")
}
