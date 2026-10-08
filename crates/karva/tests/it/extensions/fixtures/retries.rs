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
fn retry_handles_fixture_setup_failure() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

setups = 0

@karva.fixture
def setup_once():
    global setups
    setups += 1
    if setups == 1:
        raise RuntimeError("setup failed")

def test_retry(setup_once):
    assert setups == 2
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
     TRY 1 ERROR [TIME] test::test_retry
      TRY 2 PASS [TIME] test::test_retry
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry

    ----- stderr -----
    ");
}

#[test]
fn retry_handles_fixture_teardown_failure() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import karva

cleanups = []

@karva.fixture
def resource():
    yield
    cleanups.append("resource")

@karva.fixture
def broken_teardown(resource):
    yield
    if os.environ["KARVA_ATTEMPT"] == "1":
        raise RuntimeError("teardown failed")

def test_retry(broken_teardown):
    if os.environ["KARVA_ATTEMPT"] == "2":
        assert cleanups == ["resource"]
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
     TRY 1 ERROR [TIME] test::test_retry(broken_teardown=None)
      TRY 2 PASS [TIME] test::test_retry(broken_teardown=None)
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry(broken_teardown=None)

    ----- stderr -----
    ");
}

#[test]
fn retry_recreates_monkeypatch() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os

def test_retry(monkeypatch):
    assert os.environ.get("RETRY_VALUE") is None
    monkeypatch.setenv("RETRY_VALUE", "set")
    assert os.environ["KARVA_ATTEMPT"] == "2"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_retry(monkeypatch)
      TRY 2 PASS [TIME] test::test_retry(monkeypatch)
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry(monkeypatch)

    ----- stderr -----
    ");
}

#[test]
fn retry_recreates_capsys() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os

def test_retry(capsys):
    assert capsys.readouterr().out == ""
    print("attempt output")
    assert os.environ["KARVA_ATTEMPT"] == "2"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_retry(capsys)
      TRY 2 PASS [TIME] test::test_retry(capsys)
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry(capsys)

    ----- stderr -----
    ");
}

#[test]
fn retry_recreates_caplog() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import logging

def test_retry(caplog):
    assert caplog.records == []
    logging.warning("attempt log")
    assert len(caplog.records) == 1
    assert os.environ["KARVA_ATTEMPT"] == "2"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_retry(caplog)
      TRY 2 PASS [TIME] test::test_retry(caplog)
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry(caplog)

    ----- stderr -----
    ");
}

#[test]
fn retry_recreates_recwarn() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import warnings

def test_retry(recwarn):
    assert len(recwarn) == 0
    warnings.warn("attempt warning")
    assert len(recwarn) == 1
    assert os.environ["KARVA_ATTEMPT"] == "2"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_retry(recwarn)
      TRY 2 PASS [TIME] test::test_retry(recwarn)
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry(recwarn)

    ----- stderr -----
    ");
}
