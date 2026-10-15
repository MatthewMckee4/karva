use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[rstest]
fn captured_stdin_rejects_python_reads(
    #[values(
        "input()",
        "sys.stdin.read()",
        "sys.stdin.readline()",
        "sys.stdin.readlines()",
        "next(sys.stdin)",
        "sys.stdin.buffer.read()",
        "sys.stdin.buffer.readline()",
        "next(sys.stdin.buffer)",
        "sys.stdin.buffer.readlines()",
        "sys.stdin.buffer.readinto(bytearray(1))"
    )]
    expression: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        format!(
            r#"
import sys


def test_read():
    try:
        {expression}
    except RuntimeError as error:
        assert str(error) == "stdin is unavailable while test output is captured"
    else:
        raise AssertionError("stdin read was allowed")
"#
        ),
    );
    assert_cmd_snapshot!(context.command_no_parallel().pass_stdin("real input\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_read
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn captured_stdin_gives_native_and_child_reads_eof() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import subprocess
import sys


def test_read():
    assert os.read(0, 1) == b""
    child = subprocess.run(
        [sys.executable, "-c", "import sys; print(repr(sys.stdin.buffer.read()))"],
        stdout=subprocess.PIPE, check=True,
    )
    assert child.stdout.strip() == b"b''"
    explicit = subprocess.run(
        [sys.executable, "-c", "import sys; print(sys.stdin.read())"],
        input=b"explicit input", stdout=subprocess.PIPE, check=True,
    )
    assert explicit.stdout.strip() == b"explicit input"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().pass_stdin("real input\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_read
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn uncaptured_stdin_remains_available(
    #[values("input()", "os.read(0, 64).decode().strip()")] expression: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        format!(
            r#"
import os


def test_read():
    assert {expression} == "real input"
"#
        ),
    );
    assert_cmd_snapshot!(
        context
            .command()
            .arg("--no-capture")
            .pass_stdin("real input\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_read
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn captured_stdin_covers_fixtures_and_retries() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import sys
import karva

original_stdin = sys.stdin
attempts = 0


def assert_guarded():
    assert sys.stdin is not original_stdin
    assert os.read(0, 1) == b""
    try:
        sys.stdin.read()
    except RuntimeError as error:
        assert str(error) == "stdin is unavailable while test output is captured"
    else:
        raise AssertionError("stdin read was allowed")


@karva.fixture(scope="session")
def session():
    yield
    assert_guarded()


@karva.fixture
def guarded(session):
    assert_guarded()
    yield
    assert_guarded()


def test_retry(guarded):
    global attempts
    attempts += 1
    assert_guarded()
    assert attempts == 2


def test_after_retry(guarded):
    assert_guarded()
"#,
    );
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("--retry=1")
            .pass_stdin("real input\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
      TRY 1 FAIL [TIME] test::test_retry(guarded=None)
      TRY 2 PASS [TIME] test::test_retry(guarded=None)
            PASS [TIME] test::test_after_retry(guarded=None)
    ────────────
         Summary [TIME] 2 tests run: 2 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry(guarded=None)

    ----- stderr -----
    "
    );
}
