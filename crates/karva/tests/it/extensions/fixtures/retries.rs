use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn retry_recreates_function_fixtures_and_keeps_broader_scopes() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os

import karva

current_auto = None
current_used = None
teardowns = []

@karva.fixture(scope="module")
def shared():
    return []

@karva.fixture
def values():
    value = []
    yield value
    teardowns.append("values")

@karva.fixture(auto_use=True)
def automatic():
    global current_auto
    current_auto = []
    yield
    teardowns.append("automatic")

@karva.fixture
def used():
    global current_used
    current_used = []
    yield
    teardowns.append("used")

@karva.tags.use_fixtures("used")
@karva.tags.parametrize("case", [1])
def test_retry(values, shared, case):
    assert values == []
    assert current_auto == []
    assert current_used == []
    if os.environ["KARVA_ATTEMPT"] == "2":
        assert teardowns == ["values", "automatic", "used"]
        assert shared == [1]
    values.append(case)
    current_auto.append(case)
    current_used.append(case)
    shared.append(case)
    assert os.environ["KARVA_ATTEMPT"] == "2"
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_retry(values=[], shared=[], case=1)
      TRY 2 PASS [TIME] test::test_retry(values=[], shared=[], case=1)
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry(values=[], shared=[], case=1)

    ----- stderr -----
    ");
}

#[test]
fn retry_recreates_async_generator_fixtures() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os

import karva

teardowns = 0

@karva.fixture
async def values():
    global teardowns
    yield []
    teardowns += 1

async def test_retry(values):
    assert values == []
    if os.environ["KARVA_ATTEMPT"] == "2":
        assert teardowns == 1
    values.append(1)
    assert os.environ["KARVA_ATTEMPT"] == "2"
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_retry(values=[])
      TRY 2 PASS [TIME] test::test_retry(values=[])
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry(values=[])

    ----- stderr -----
    ");
}

#[test]
fn retry_handles_fixture_setup_and_teardown_failures() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os

import karva

setups = 0
cleanups = []

@karva.fixture
def setup_once():
    global setups
    setups += 1
    if setups == 1:
        raise RuntimeError("setup failed")

def test_setup(setup_once):
    assert setups == 2

@karva.fixture
def remaining_cleanup():
    yield
    cleanups.append("remaining")

@karva.fixture
def broken_teardown(remaining_cleanup):
    yield
    if os.environ["KARVA_ATTEMPT"] == "1":
        raise RuntimeError("teardown failed")

def test_teardown(broken_teardown):
    if os.environ["KARVA_ATTEMPT"] == "2":
        assert cleanups == ["remaining"]
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
     TRY 1 ERROR [TIME] test::test_setup
      TRY 2 PASS [TIME] test::test_setup
     TRY 1 ERROR [TIME] test::test_teardown(broken_teardown=None)
      TRY 2 PASS [TIME] test::test_teardown(broken_teardown=None)
    ────────────
         Summary [TIME] 2 tests run: 2 passed (2 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_setup
       FLAKY 2/2 [TIME] test::test_teardown(broken_teardown=None)

    ----- stderr -----
    ");
}

#[test]
fn retry_recreates_cleanup_aware_builtin_fixtures() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import logging
import warnings

def test_retry(capsys, caplog, monkeypatch, recwarn):
    if os.environ["KARVA_ATTEMPT"] == "2":
        assert os.environ.get("RETRY_VALUE") is None
        assert capsys.readouterr().out == ""
        assert caplog.records == []
        assert len(recwarn) == 0
    monkeypatch.setenv("RETRY_VALUE", "set")
    logging.warning("attempt log")
    warnings.warn("attempt warning")
    print("attempt output")
    assert len(recwarn) == 1
    assert capsys.readouterr().out == "attempt output\n"
    assert os.environ["KARVA_ATTEMPT"] == "2"
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_retry(capsys, caplog, monkeypatch, recwarn)
      TRY 2 PASS [TIME] test::test_retry(capsys, caplog, monkeypatch, recwarn)
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry(capsys, caplog, monkeypatch, recwarn)

    ----- stderr -----
    ");
}
