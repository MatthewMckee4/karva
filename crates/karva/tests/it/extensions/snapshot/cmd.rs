use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_cmd_snapshot_basic_echo() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_echo():
    cmd = karva.Command(sys.executable).args(["-c", "print('hello world')"])
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_echo ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_echo.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:7::test_echo
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    hello world
    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_creates_snap_new() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import sys

def test_echo():
    cmd = karva.Command(sys.executable).args(['-c', 'print(42)'])
    karva.assert_cmd_snapshot(cmd)
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_echo ... FAILED

    diagnostics:

    error[test-failure]: Test `test_echo` failed
     --> test.py:5:5
      |
    3 | import sys
    4 |
    5 | def test_echo():
      |     ^^^^^^^^^
    6 |     cmd = karva.Command(sys.executable).args(['-c', 'print(42)'])
    7 |     karva.assert_cmd_snapshot(cmd)
      |
    info: Test failed here
     --> test.py:7:5
      |
    5 | def test_echo():
    6 |     cmd = karva.Command(sys.executable).args(['-c', 'print(42)'])
    7 |     karva.assert_cmd_snapshot(cmd)
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: New snapshot for 'test_echo'.
          Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.
          Pending file: <temp_dir>/snapshots/test__test_echo.snap.new

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_echo.snap.new");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:7::test_echo
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    42
    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_nonzero_exit_code() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_failing_cmd():
    cmd = karva.Command(sys.executable).args(["-c", "import sys; print('oops', file=sys.stderr); sys.exit(1)"])
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_failing_cmd ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_failing_cmd.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:7::test_failing_cmd
    ---
    success: false
    exit_code: 1
    ----- stdout -----
    ----- stderr -----
    oops
    ");
}

#[test]
fn test_cmd_snapshot_stderr() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_stderr():
    cmd = karva.Command(sys.executable).args(["-c", "import sys; print('out'); print('err', file=sys.stderr)"])
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_stderr ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_stderr.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:7::test_stderr
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    out
    ----- stderr -----
    err
    ");
}

#[test]
fn test_cmd_snapshot_named() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_named_cmd():
    cmd = karva.Command(sys.executable).args(["-c", "print('named')"])
    karva.assert_cmd_snapshot(cmd, name="my_cmd")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_named_cmd ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_named_cmd--my_cmd.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:7::test_named_cmd
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    named
    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_with_stdin() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_stdin():
    cmd = (
        karva.Command(sys.executable)
        .args(["-c", "import sys; print(sys.stdin.read().strip())"])
        .stdin("hello from stdin")
    )
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_stdin ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_stdin.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:11::test_stdin
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    hello from stdin
    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_with_filters() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_filtered():
    with karva.snapshot_settings(filters=[
        (r"\d+\.\d+\.\d+", "[VERSION]"),
    ]):
        cmd = karva.Command(sys.executable).arg("--version")
        karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_filtered ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_filtered.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:10::test_filtered
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    Python [VERSION]
    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_with_env() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_env():
    cmd = (
        karva.Command(sys.executable)
        .args(["-c", "import os; print(os.environ['MY_VAR'])"])
        .env("MY_VAR", "hello")
    )
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_env ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_env.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:11::test_env
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    hello
    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_inline_matching() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_inline():
    cmd = karva.Command(sys.executable).args(["-c", "print('hi')"])
    karva.assert_cmd_snapshot(cmd, inline="""\
        success: true
        exit_code: 0
        ----- stdout -----
        hi
        ----- stderr -----
    """)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_inline ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_inline_empty_creates_value() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_inline_empty():
    cmd = karva.Command(sys.executable).args(["-c", "print('created')"])
    karva.assert_cmd_snapshot(cmd, inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_inline_empty ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"

    import karva
    import sys

    def test_inline_empty():
        cmd = karva.Command(sys.executable).args(["-c", "print('created')"])
        karva.assert_cmd_snapshot(cmd, inline="""/
            success: true
            exit_code: 0
            ----- stdout -----
            created
            ----- stderr -----
        """)
    "#);
}

#[test]
fn test_cmd_snapshot_inline_mismatch() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_inline_wrong():
    cmd = karva.Command(sys.executable).args(["-c", "print('actual')"])
    karva.assert_cmd_snapshot(cmd, inline="wrong value")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_inline_wrong ... FAILED

    diagnostics:

    error[test-failure]: Test `test_inline_wrong` failed
     --> test.py:5:5
      |
    3 | import sys
    4 |
    5 | def test_inline_wrong():
      |     ^^^^^^^^^^^^^^^^^
    6 |     cmd = karva.Command(sys.executable).args(["-c", "print('actual')"])
    7 |     karva.assert_cmd_snapshot(cmd, inline="wrong value")
      |
    info: Test failed here
     --> test.py:7:5
      |
    5 | def test_inline_wrong():
    6 |     cmd = karva.Command(sys.executable).args(["-c", "print('actual')"])
    7 |     karva.assert_cmd_snapshot(cmd, inline="wrong value")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Inline snapshot mismatch for 'test_inline_wrong'.
          ────────────┬───────────────────────────
              1       │ -wrong value
                    1 │ +success: true
                    2 │ +exit_code: 0
                    3 │ +----- stdout -----
                    4 │ +actual
                    5 │ +----- stderr -----
          ────────────┴───────────────────────────

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
}

#[test]
fn test_cmd_snapshot_mismatch_existing() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_change():
    cmd = karva.Command(sys.executable).args(["-c", "print('first')"])
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    let content = context.read_file("snapshots/test__test_change.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:7::test_change
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    first
    ----- stderr -----
    ");

    context.write_file(
        "test.py",
        r#"
import karva
import sys

def test_change():
    cmd = karva.Command(sys.executable).args(["-c", "print('second')"])
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_change ... FAILED

    diagnostics:

    error[test-failure]: Test `test_change` failed
     --> test.py:5:5
      |
    3 | import sys
    4 |
    5 | def test_change():
      |     ^^^^^^^^^^^
    6 |     cmd = karva.Command(sys.executable).args(["-c", "print('second')"])
    7 |     karva.assert_cmd_snapshot(cmd)
      |
    info: Test failed here
     --> test.py:7:5
      |
    5 | def test_change():
    6 |     cmd = karva.Command(sys.executable).args(["-c", "print('second')"])
    7 |     karva.assert_cmd_snapshot(cmd)
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Snapshot mismatch for 'test_change'.
          Snapshot file: <temp_dir>/snapshots/test__test_change.snap
          ────────────┬───────────────────────────
              1     1 │  success: true
              2     2 │  exit_code: 0
              3     3 │  ----- stdout -----
              4       │ -first
                    4 │ +second
              5     5 │  ----- stderr -----
          ────────────┴───────────────────────────

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
}

#[test]
fn test_cmd_snapshot_nonexistent_command() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_bad_cmd():
    cmd = karva.Command('nonexistent_program_xyz_12345')
    karva.assert_cmd_snapshot(cmd)
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_bad_cmd ... FAILED

    diagnostics:

    error[test-failure]: Test `test_bad_cmd` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_bad_cmd():
      |     ^^^^^^^^^^^^
    5 |     cmd = karva.Command('nonexistent_program_xyz_12345')
    6 |     karva.assert_cmd_snapshot(cmd)
      |
    info: Test failed here
     --> test.py:6:5
      |
    4 | def test_bad_cmd():
    5 |     cmd = karva.Command('nonexistent_program_xyz_12345')
    6 |     karva.assert_cmd_snapshot(cmd)
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Failed to run command: No such file or directory (os error 2)

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_multiple_with_allow_duplicates() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_multi():
    with karva.snapshot_settings(allow_duplicates=True):
        cmd1 = karva.Command(sys.executable).args(["-c", "print('first')"])
        karva.assert_cmd_snapshot(cmd1)
        cmd2 = karva.Command(sys.executable).args(["-c", "print('second')"])
        karva.assert_cmd_snapshot(cmd2)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_multi ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content_0 = context.read_file("snapshots/test__test_multi-0.snap");
    insta::assert_snapshot!(content_0, @"
    ---
    source: test.py:8::test_multi
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    first
    ----- stderr -----
    ");

    let content_1 = context.read_file("snapshots/test__test_multi-1.snap");
    insta::assert_snapshot!(content_1, @r"
    ---
    source: test.py:10::test_multi
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    second
    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_current_dir() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys
import os

def test_cwd():
    cmd = (
        karva.Command(sys.executable)
        .args(["-c", "import os; print(os.getcwd())"])
        .current_dir("/tmp")
    )
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_cwd ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_cwd.snap");
    let content = content.replace("/private/tmp", "/tmp");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:12::test_cwd
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    /tmp
    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_envs_multiple() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_envs():
    cmd = (
        karva.Command(sys.executable)
        .args(["-c", "import os; print(os.environ['A'], os.environ['B'])"])
        .envs({"A": "one", "B": "two"})
    )
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_envs ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_envs.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:11::test_envs
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    one two
    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_multiline_output() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_multiline():
    code = "for i in range(3): print(f'line {i}')"
    cmd = karva.Command(sys.executable).args(["-c", code])
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_multiline ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_multiline.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:8::test_multiline
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    line 0
    line 1
    line 2
    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_exit_code_42() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_exit42():
    cmd = karva.Command(sys.executable).args(["-c", "import sys; sys.exit(42)"])
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_exit42 ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_exit42.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:7::test_exit42
    ---
    success: false
    exit_code: 42
    ----- stdout -----
    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_builder_chaining() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_chained():
    cmd = (
        karva.Command(sys.executable)
        .arg("-c")
        .arg("import os; print(os.environ.get('X', 'none'))")
        .env("X", "chained")
    )
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_chained ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_chained.snap");
    insta::assert_snapshot!(content, @"
    ---
    source: test.py:12::test_chained
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    chained
    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_both_stdout_and_stderr_with_failure() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_both():
    cmd = karva.Command(sys.executable).args(["-c", "import sys; print('out'); print('err', file=sys.stderr); sys.exit(2)"])
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_both ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_both.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:7::test_both
    ---
    success: false
    exit_code: 2
    ----- stdout -----
    out
    ----- stderr -----
    err
    ");
}

#[test]
fn test_cmd_snapshot_empty_output() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_silent():
    cmd = karva.Command(sys.executable).args(["-c", "pass"])
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_silent ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_silent.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:7::test_silent
    ---
    success: true
    exit_code: 0
    ----- stdout -----
    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_matches_after_update() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_stable():
    cmd = karva.Command(sys.executable).args(["-c", "print('stable')"])
    karva.assert_cmd_snapshot(cmd)
        "#,
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_stable ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_cmd_snapshot_inline_and_name_exclusive() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import sys

def test_both_args():
    cmd = karva.Command(sys.executable).args(["-c", "print('x')"])
    karva.assert_cmd_snapshot(cmd, inline="x", name="y")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_both_args ... FAILED

    diagnostics:

    error[test-failure]: Test `test_both_args` failed
     --> test.py:5:5
      |
    3 | import sys
    4 |
    5 | def test_both_args():
      |     ^^^^^^^^^^^^^^
    6 |     cmd = karva.Command(sys.executable).args(["-c", "print('x')"])
    7 |     karva.assert_cmd_snapshot(cmd, inline="x", name="y")
      |
    info: Test failed here
     --> test.py:7:5
      |
    5 | def test_both_args():
    6 |     cmd = karva.Command(sys.executable).args(["-c", "print('x')"])
    7 |     karva.assert_cmd_snapshot(cmd, inline="x", name="y")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: assert_snapshot() cannot use both 'inline' and 'name' arguments

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
}
