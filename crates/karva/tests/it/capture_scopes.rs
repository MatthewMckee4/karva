use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn successful_scoped_fixture_output_is_hidden_or_shown_as_requested() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import os
import karva

@karva.fixture(scope="session", auto_use=True)
def session_output():
    print("session setup python")
    os.write(1, b"session setup native\n")
    yield
    print("session teardown python")
    os.write(2, b"session teardown native\n")

@karva.fixture(scope="package", auto_use=True)
def package_output():
    print("package setup python")
    os.write(2, b"package setup native\n")
    yield
    print("package teardown python")
    os.write(1, b"package teardown native\n")
"#,
        ),
        (
            "test_scopes.py",
            r#"
import os
import karva

@karva.fixture(scope="module", auto_use=True)
def module_output():
    print("module setup python")
    os.write(1, b"module setup native\n")
    yield
    print("module teardown python")
    os.write(2, b"module teardown native\n")

def test_scoped_output_is_hidden():
    pass
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_scopes::test_scoped_output_is_hidden
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
    assert_cmd_snapshot!(context.command_no_parallel().arg("--show-output"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    session setup python
    session setup native
    package setup python
    module setup python
    module setup native
            PASS [TIME] test_scopes::test_scoped_output_is_hidden
    module teardown python
    package teardown python
    package teardown native
    session teardown python
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    package setup native
    module teardown native
    session teardown native
    ");
}

#[test]
fn scoped_fixture_setup_stderr_is_retained_when_setup_fails() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import os
import karva

@karva.fixture(scope="session", auto_use=True)
def first_session_output():
    print("partial setup succeeded")
    yield
    os.write(2, b"partial setup finalizer stderr marker\n")

@karva.fixture(scope="session", auto_use=True)
def broken_session_output():
    os.write(2, b"session setup stderr marker\n")
    raise RuntimeError("session setup failed")
"#,
        ),
        (
            "test_setup_failure.py",
            "def test_never_runs():\n    assert False\n",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test_setup_failure::test_never_runs

    failures:

    test_setup_failure::test_never_runs (uses auto-use fixture `broken_session_output`):

    error[fixture-failure]: Fixture `broken_session_output` failed
      --> conftest.py:12:5
       |
    12 | def broken_session_output():
       |     ^^^^^^^^^^^^^^^^^^^^^
    info: Fixture failed here
      --> conftest.py:14:5
       |
    14 |     raise RuntimeError("session setup failed")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: session setup failed

    info[fixture-output]: captured session fixture setup output
    info: captured fixture output
    captured stdout:
    partial setup succeeded
    captured stderr:
    session setup stderr marker
    partial setup finalizer stderr marker

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn scoped_fixture_setup_output_is_retained_for_body_failure() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import os
import karva

@karva.fixture(scope="session", auto_use=True)
def session_output():
    print("session setup body failure marker")
    os.write(1, b"session setup body failure native marker\n")
    yield
    print("session teardown body failure marker")
    os.write(2, b"session teardown body failure native marker\n")
"#,
        ),
        (
            "test_body_failure.py",
            "def test_body_fails():\n    assert False\n",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_body_failure::test_body_fails

    failures:

    test_body_failure::test_body_fails:

    error[test-failure]: Test `test_body_fails` failed
     --> test_body_failure.py:1:5
      |
    1 | def test_body_fails():
      |     ^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_body_failure.py:2:5
      |
    2 |     assert False
      |     ^^^^^^^^^^^^

    captured stdout:
    session setup body failure marker
    session setup body failure native marker

    diagnostics:

    info[fixture-output]: captured session fixture cleanup output
    info: captured fixture output
    captured stdout:
    session teardown body failure marker
    captured stderr:
    session teardown body failure native marker

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn scoped_fixture_teardown_output_is_retained_when_teardown_fails() {
    let context = TestContext::with_file(
        "test_teardown_failure.py",
        r#"
import os
import karva

@karva.fixture(scope="module", auto_use=True)
def broken_module_output():
    yield
    os.write(2, b"module teardown stderr marker\n")
    raise RuntimeError("module teardown failed")

def test_body_passes():
    pass
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_teardown_failure::test_body_passes

    diagnostics:

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `broken_module_output`
     --> test_teardown_failure.py:6:5
      |
    6 | def broken_module_output():
      |     ^^^^^^^^^^^^^^^^^^^^
    info: Failed to reset fixture: module teardown failed
    info: captured fixture cleanup output
    captured stderr:
    module teardown stderr marker

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}
