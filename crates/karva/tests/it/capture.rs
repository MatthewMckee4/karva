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
        &format!(
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
        &format!(
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

#[test]
fn captured_native_output_is_hidden_for_passing_tests() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import subprocess
import sys


def test_output():
    print("python stdout")
    os.write(1, b"native stdout\n")
    os.write(2, b"native stderr\n")
    subprocess.run([sys.executable, "-c", "print('child stdout')"], check=True)
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_output
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn captured_native_output_includes_setup_call_and_teardown() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import ctypes
import os
import subprocess
import sys
import karva


@karva.fixture
def output():
    os.write(1, b"setup stdout\n")
    yield
    os.write(2, b"teardown stderr\n")


def test_output(output):
    print("python stdout")
    os.write(1, b"native stdout\n")
    os.write(2, b"native stderr\n")
    subprocess.run([sys.executable, "-c", "print('child stdout')"], check=True)
    libc = ctypes.CDLL("ucrtbase" if os.name == "nt" else None)
    libc.puts(b"C stdout")
    raise ValueError("intentional failure")
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_output(output=None)

    failures:

    test::test_output(output=None):

    error[test-failure]: Test `test_output` failed
      --> test.py:16:5
       |
    16 | def test_output(output):
       |     ^^^^^^^^^^^
    info: Test ran with arguments:
    info:   `output`: `None`
    info: Test failed here
      --> test.py:23:5
       |
    23 |     raise ValueError("intentional failure")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: intentional failure

    captured stdout:
    setup stdout
    python stdout
    native stdout
    child stdout
    C stdout
    captured stderr:
    native stderr
    teardown stderr

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn captured_native_bytes_decode_without_crashing() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os


def test_output():
    os.write(1, b"invalid: \xff\n")
    raise ValueError("intentional failure")
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_output

    failures:

    test::test_output:

    error[test-failure]: Test `test_output` failed
     --> test.py:5:5
      |
    5 | def test_output():
      |     ^^^^^^^^^^^
    info: Test failed here
     --> test.py:7:5
      |
    7 |     raise ValueError("intentional failure")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: intentional failure

    captured stdout:
    invalid: �

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[rstest]
fn captured_output_is_bounded(
    #[values(
        "os.write(1, data)",
        "sys.stdout.write(data.decode())",
        "sys.stdout.buffer.write(data)"
    )]
    write: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
import os
import sys


def test_output():
    data = b"HEAD" + b"x" * 200 + b"TAIL"
    {write}
    raise ValueError("intentional failure")
"#
        ),
    );
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(
        r"(?m)^(\d+ \| )    (?:os.write|sys.stdout).*",
        "$1    [write output]",
    );
    let _scope = settings.bind_to_scope();
    assert_cmd_snapshot!(context.command_no_parallel().arg("--max-output-bytes=32"), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_output

    failures:

    test::test_output:

    error[test-failure]: Test `test_output` failed
     --> test.py:6:5
      |
    6 | def test_output():
      |     ^^^^^^^^^^^
    info: Test failed here
     --> test.py:9:5
      |
    9 |     raise ValueError("intentional failure")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: intentional failure

    captured stdout:
    HEADxxxxxxxxxxxx
    ... captured output truncated: 176 of 208 bytes omitted (max-output-bytes=32) ...
    xxxxxxxxxxxxTAIL

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    WARN captured stdout exceeded max-output-bytes: requested=208 bytes, allowed=32 bytes; retained beginning and end
    "#);
}

#[test]
fn captured_output_does_not_wait_for_a_leaked_child() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import subprocess
import sys

child = None


def test_child():
    global child
    child = subprocess.Popen([
        sys.executable, "-c", "import sys; sys.stdin.read()",
    ], stdin=subprocess.PIPE)


def test_release_child():
    child.communicate()
    assert child.returncode == 0
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_child
            PASS [TIME] test::test_release_child
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn captured_output_is_shared_by_machine_reports(#[values("json", "jsonl")] format: &str) {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.terminal]
max-output-bytes = 32

[profile.default.junit]
path = "results.xml"
store-failure-output = true
"#,
        ),
        (
            "test.py",
            r#"
import os


def test_output():
    os.write(1, b"HEAD" + b"x" * 200 + b"TAIL")
    os.write(2, b"native stderr\n")
    raise ValueError("intentional failure")
"#,
        ),
    ]);
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--result-output=results.json", &format!("--result-format={format}"),
    ]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_output

    failures:

    test::test_output:

    error[test-failure]: Test `test_output` failed
     --> test.py:5:5
      |
    5 | def test_output():
      |     ^^^^^^^^^^^
    info: Test failed here
     --> test.py:8:5
      |
    8 |     raise ValueError("intentional failure")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: intentional failure

    captured stdout:
    HEADxxxxxxxxxxxx
    ... captured output truncated: 176 of 208 bytes omitted (max-output-bytes=32) ...
    xxxxxxxxxxxxTAIL
    captured stderr:
    native stderr

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    WARN captured stdout exceeded max-output-bytes: requested=208 bytes, allowed=32 bytes; retained beginning and end
    "#);
    let content = context.read_file("results.json");
    let report: serde_json::Value = serde_json::from_str(if format == "jsonl" {
        content.lines().next().expect("test record")
    } else {
        &content
    })
    .expect("valid result report");
    let test = if format == "json" {
        &report["tests"][0]
    } else {
        &report
    };
    let output = &test["captured_output"];
    let retained = "HEADxxxxxxxxxxxx\n... captured output truncated: 176 of 208 bytes omitted (max-output-bytes=32) ...\nxxxxxxxxxxxxTAIL";
    assert_eq!(output["stdout"], retained);
    assert_eq!(output["stderr"], "native stderr\n");
    let xml = context.read_file("results.xml");
    assert!(xml.contains(retained));
    assert!(xml.contains("<system-err>native stderr\n</system-err>"));
}

#[test]
fn captured_output_drains_writes_larger_than_a_pipe_buffer() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os


def test_output():
    data = b"x" * (1024 * 1024)
    for _ in range(64):
        os.write(1, data)
    os.write(1, b"TAIL")
    raise ValueError("intentional failure")
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().arg("--max-output-bytes=32"), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_output

    failures:

    test::test_output:

    error[test-failure]: Test `test_output` failed
     --> test.py:5:5
      |
    5 | def test_output():
      |     ^^^^^^^^^^^
    info: Test failed here
      --> test.py:10:5
       |
    10 |     raise ValueError("intentional failure")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: intentional failure

    captured stdout:
    xxxxxxxxxxxxxxxx
    ... captured output truncated: 67108836 of 67108868 bytes omitted (max-output-bytes=32) ...
    xxxxxxxxxxxxTAIL

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    WARN captured stdout exceeded max-output-bytes: requested=67108868 bytes, allowed=32 bytes; retained beginning and end
    "#);
}

#[test]
fn captured_output_remains_available_at_a_native_deadline() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import karva


@karva.tags.timeout(0.1)
def test_output():
    os.write(1, b"before timeout\n")
    os.write(2, b"native stderr\n")
    while True:
        pass
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_output

    failures:

    test::test_output:

    error[test-failure]: Test `test_output` failed
     --> test.py:7:5
      |
    7 | def test_output():
      |     ^^^^^^^^^^^
    info: Test exceeded timeout of 0.1 seconds
    info: Worker terminated at the deadline; fixture teardown could not be guaranteed.

    captured stdout:
    before timeout
    captured stderr:
    native stderr

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn captured_output_is_isolated_between_retries() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os


def test_retry():
    attempt = os.environ["KARVA_ATTEMPT"]
    os.write(1, f"attempt {attempt}\n".encode())
    if attempt == "1":
        raise ValueError("retry me")
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--retry=1", "--result-output=results.json",
    ]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_retry
      TRY 2 PASS [TIME] test::test_retry
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry

    ----- stderr -----
    ");
    let report: serde_json::Value =
        serde_json::from_str(&context.read_file("results.json")).expect("valid report");
    let attempts = &report["tests"][0]["attempts"];
    assert_eq!(attempts[0]["captured_output"]["stdout"], "attempt 1\n");
    assert_eq!(attempts[1]["captured_output"]["stdout"], "attempt 2\n");
}

#[test]
fn zero_output_budget_is_rejected() {
    let context = TestContext::with_file("test.py", "def test_pass(): pass");
    assert_cmd_snapshot!(context.command().arg("--max-output-bytes=0"), @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    error: invalid value '0' for '--max-output-bytes <MAX_OUTPUT_BYTES>': number would be zero for non-zero type

    For more information, try '--help'.
    ");
}

#[rstest]
fn captured_broader_fixture_output_is_hidden(
    #[values("session", "package", "module")] scope: &str,
) {
    let context = TestContext::with_files([
        (
            "conftest.py",
            format!(
                r#"
import os
import karva


@karva.fixture(scope="{scope}", auto_use=True)
def output():
    os.write(1, b"scope setup\n")
    yield
    os.write(2, b"scope teardown\n")
"#
            )
            .as_str(),
        ),
        ("test.py", "def test_pass(): pass"),
    ]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_pass
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn captured_broader_fixture_failures_keep_output(#[values("setup", "teardown")] phase: &str) {
    let body = if phase == "setup" {
        "raise ValueError('scope failure')"
    } else {
        "yield\n    os.write(1, b'teardown stdout\\n')\n    os.write(2, b'teardown stderr\\n')\n    raise ValueError('scope failure')"
    };
    let context = TestContext::with_files([
        (
            "conftest.py",
            format!(
                r#"
import os
import karva


@karva.fixture(scope="session", auto_use=True)
def output():
    os.write(1, b"scope stdout\n")
    os.write(2, b"scope stderr\n")
    {body}
"#
            )
            .as_str(),
        ),
        ("test.py", "def test_pass(): pass"),
    ]);
    if phase == "setup" {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test::test_pass

        failures:

        test::test_pass (uses auto-use fixture `output`):

        error[fixture-failure]: Fixture `output` failed
         --> conftest.py:7:5
          |
        7 | def output():
          |     ^^^^^^
        info: Fixture failed here
          --> conftest.py:10:5
           |
        10 |     raise ValueError('scope failure')
           |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: scope failure
        info: captured stdout:
              scope stdout
        info: captured stderr:
              scope stderr

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        ");
    } else {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_pass

        diagnostics:

        error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `output`
         --> conftest.py:7:5
          |
        7 | def output():
          |     ^^^^^^
        info: Failed to reset fixture: scope failure
        info: captured stdout:
              teardown stdout
        info: captured stderr:
              teardown stderr

        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn no_capture_forwards_native_output_with_status_disabled() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os


def test_output():
    os.write(1, b"native stdout\n")
    os.write(2, b"native stderr\n")
"#,
    );
    assert_cmd_snapshot!(context.command().args(["--no-capture", "--status-level=none", "--final-status-level=none"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    native stdout

    ----- stderr -----
    native stderr
    ");
}
